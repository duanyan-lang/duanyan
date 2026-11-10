//! duanyan debug - 交互式调试器
//!
//! 单步执行、断点、寄存器/内存查看、源码定位

use std::io::{self, Write, BufRead};
use std::collections::HashSet;
use crate::engine::{Engine, RunStop};
use crate::backend::Instruction;

/// 调试器状态
pub struct Debugger {
    engine: Engine,
    source_map: Vec<usize>,
    source_lines: Vec<String>,
    breakpoints: HashSet<usize>,
    filename: String,
}

impl Debugger {
    pub fn new(
        machine_code: Vec<u32>,
        instructions: &[Instruction],
        source_map: Vec<usize>,
        source: &str,
        filename: &str,
    ) -> Self {
        let mut engine = Engine::new(machine_code);
        // 初始化 SP
        // (Engine::run would do this, but we're doing manual stepping)

        Debugger {
            engine,
            source_map,
            source_lines: source.lines().map(|s| s.to_string()).collect(),
            breakpoints: HashSet::new(),
            filename: filename.to_string(),
        }
    }

    pub fn run_interactive(&mut self) {
        println!("🪤 Hardy 调试器已启动");
        println!("输入 help 查看命令列表\n");

        self.show_context();

        let stdin = io::stdin();
        loop {
            print!("duanyan> ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            if stdin.lock().read_line(&mut input).is_err() || input.trim().is_empty() {
                continue;
            }
            let cmd = input.trim();
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if parts.is_empty() { continue; }

            match parts[0] {
                "s" | "step" => self.cmd_step(),
                "c" | "continue" => self.cmd_continue(),
                "r" | "regs" => self.cmd_regs(),
                "reg" if parts.len() > 1 => self.cmd_reg(parts[1]),
                "m" | "mem" => self.cmd_mem(parts.get(1).copied()),
                "b" | "break" => self.cmd_break(parts.get(1).copied()),
                "d" | "delete" => self.cmd_delete(parts.get(1).copied()),
                "info" => self.cmd_info(),
                "asm" => self.cmd_asm(),
                "src" | "source" => self.cmd_source(),
                "h" | "help" => self.cmd_help(),
                "q" | "quit" => { println!("再见！"); break; }
                _ => println!("未知命令: {} (输入 help 查看帮助)", parts[0]),
            }
        }
    }

    fn cmd_step(&mut self) {
        if self.engine.halted() {
            println!("程序已终止");
            return;
        }
        match self.engine.step_once() {
            Ok(halted) => {
                if halted {
                    println!("\n⚡ 程序终止 ({} cycles)", self.engine.cycles());
                } else {
                    self.show_context();
                }
            }
            Err(e) => {
                let line = self.current_source_line();
                eprintln!("❌ 运行时错误 @ {}:{} - {}", self.filename, line, e);
            }
        }
    }

    fn cmd_continue(&mut self) {
        if self.engine.halted() {
            println!("程序已终止");
            return;
        }
        match self.engine.run_to_breakpoint(&self.breakpoints) {
            RunStop::Halted => {
                println!("\n⚡ 程序终止 ({} cycles)", self.engine.cycles());
            }
            RunStop::Breakpoint(pc) => {
                let line = self.source_line_for_pc(pc);
                println!("\n🎯 命中断点 @ PC={} ({}:{})", pc, self.filename, line);
                self.show_context();
            }
            RunStop::Error(e) => {
                let line = self.current_source_line();
                eprintln!("❌ 运行时错误 @ {}:{} - {}", self.filename, line, e);
            }
        }
    }

    fn cmd_regs(&self) {
        let regs = self.engine.regs();
        let names = ["zero", "ra", "sp", "a0", "a1", "a2", "a3", "a4",
                      "rv", "t0", "t1", "t2", "t3", "t4", "t5", "fp"];
        println!("┌─────────────────────────────────────────────────┐");
        println!("│  PC = {:<6}  cycles = {}                       │", self.engine.pc(), self.engine.cycles());
        println!("├─────────────────────────────────────────────────┤");
        for i in 0..16 {
            println!("│  x{:<2} ({:>4}) = {:<12}                      │", i, names[i], regs[i]);
        }
        println!("└─────────────────────────────────────────────────┘");
    }

    fn cmd_reg(&self, name: &str) {
        let idx = match name {
            "zero" => 0, "ra" => 1, "sp" => 2,
            "a0" => 3, "a1" => 4, "a2" => 5, "a3" => 6, "a4" => 7,
            "rv" => 8,
            "t0" => 9, "t1" => 10, "t2" => 11, "t3" => 12, "t4" => 13, "t5" => 14,
            "fp" => 15,
            _ => {
                if let Ok(n) = name.strip_prefix('x').unwrap_or(name).parse::<usize>() {
                    if n < 16 { n } else { eprintln!("寄存器索引超出范围 (0-15)"); return; }
                } else {
                    eprintln!("未知寄存器: {}", name); return;
                }
            }
        };
        println!("x{} = {} (0x{:X})", idx, self.engine.reg(idx), self.engine.reg(idx));
    }

    fn cmd_mem(&self, addr_str: Option<&str>) {
        let addr = addr_str.and_then(|s| {
            if s.starts_with("0x") || s.starts_with("0X") {
                i64::from_str_radix(&s[2..], 16).ok()
            } else {
                s.parse::<i64>().ok()
            }
        }).unwrap_or(0);

        println!("┌─ 内存 @ 0x{:X} ─────────────────────┐", addr);
        for i in 0..8 {
            let a = addr + i * 8;
            let val = self.engine.mem_get(a);
            println!("│ 0x{:04X}: {:<12} (0x{:016X})  │", a, val, val as u64);
        }
        println!("└──────────────────────────────────────────┘");
    }

    fn cmd_break(&mut self, arg: Option<&str>) {
        match arg {
            Some(s) => {
                // 可以是 PC 地址或 JS 行号
                if let Some(line_str) = s.strip_prefix(':') {
                    // :line → 在 JS 行号处设断点
                    if let Ok(line) = line_str.parse::<usize>() {
                        let pcs: Vec<usize> = self.source_map.iter()
                            .enumerate()
                            .filter(|(_, &l)| l == line)
                            .map(|(i, _)| i)
                            .collect();
                        if pcs.is_empty() {
                            println!("行 {} 没有对应指令", line);
                        } else {
                            for pc in &pcs {
                                self.breakpoints.insert(*pc);
                            }
                            println!("🎯 断点设置: 行 {} → PC {:?}", line, pcs);
                        }
                    }
                } else if let Ok(pc) = s.parse::<usize>() {
                    self.breakpoints.insert(pc);
                    println!("🎯 断点设置 @ PC={}", pc);
                } else {
                    println!("用法: break <pc> 或 break :<js行号>");
                }
            }
            None => {
                if self.breakpoints.is_empty() {
                    println!("无断点");
                } else {
                    let mut bps: Vec<_> = self.breakpoints.iter().collect();
                    bps.sort();
                    println!("断点列表:");
                    for pc in bps {
                        let line = self.source_line_for_pc(*pc);
                        println!("  PC={} → {}:{}", pc, self.filename, line);
                    }
                }
            }
        }
    }

    fn cmd_delete(&mut self, arg: Option<&str>) {
        match arg {
            Some(s) => {
                if let Ok(pc) = s.parse::<usize>() {
                    if self.breakpoints.remove(&pc) {
                        println!("断点已删除: PC={}", pc);
                    } else {
                        println!("PC={} 处没有断点", pc);
                    }
                }
            }
            None => {
                self.breakpoints.clear();
                println!("所有断点已清除");
            }
        }
    }

    fn cmd_info(&self) {
        println!("指令总数: {}", self.engine.instruction_count());
        println!("PC: {}, Cycles: {}", self.engine.pc(), self.engine.cycles());
        println!("Halted: {}", self.engine.halted());
        println!("断点数: {}", self.breakpoints.len());
        let output = self.engine.output();
        if !output.is_empty() {
            println!("输出: {:?}", output);
        }
    }

    fn cmd_asm(&self) {
        let pc = self.engine.pc();
        let start = if pc > 3 { pc - 3 } else { 0 };
        let end = std::cmp::min(pc + 8, self.engine.instruction_count());

        println!("┌─ 汇编 (PC={}) ────────────────────────┐", pc);
        for i in start..end {
            let marker = if i == pc { "→" } else { " " };
            let bp = if self.breakpoints.contains(&i) { "●" } else { " " };
            if let Some(word) = self.engine.get_instruction(i) {
                let line = self.source_line_for_pc(i);
                println!("│ {} {} {:04}: 0x{:08X}  L{:<3}              │",
                    marker, bp, i, word, line);
            }
        }
        println!("└──────────────────────────────────────────┘");
    }

    fn cmd_source(&self) {
        let line = self.current_source_line();
        let start = if line > 3 { line - 3 } else { 1 };
        let end = std::cmp::min(line + 4, self.source_lines.len() + 1);

        println!("┌─ {} ─────────────────────┐", self.filename);
        for i in start..=end {
            if i >= 1 && i <= self.source_lines.len() {
                let marker = if i == line { "→" } else { " " };
                println!("│ {} {:>3} │ {}", marker, i, self.source_lines[i - 1]);
            }
        }
        println!("└──────────────────────────────────────────┘");
    }

    fn cmd_help(&self) {
        println!("🪤 Hardy 调试器命令:");
        println!();
        println!("  s, step          单步执行一条指令");
        println!("  c, continue      继续执行到断点或终止");
        println!("  r, regs          查看所有寄存器");
        println!("  reg <name>       查看指定寄存器 (x0-x15, ra, sp, a0-a4, rv, t0-t5, fp)");
        println!("  m, mem [addr]    查看内存 (默认从 0 开始)");
        println!("  b, break <pc>    设置断点");
        println!("  b, break :<line> 在 JS 源码行号处设断点");
        println!("  d, delete [pc]   删除断点 (无参数清除全部)");
        println!("  info             程序状态概览");
        println!("  asm              查看汇编上下文");
        println!("  src, source      查看源码上下文");
        println!("  h, help          帮助");
        println!("  q, quit          退出");
    }

    // ===== 辅助函数 =====

    fn show_context(&self) {
        let pc = self.engine.pc();
        let line = self.current_source_line();
        if let Some(word) = self.engine.get_instruction(pc) {
            // 源码行
            if line >= 1 && line <= self.source_lines.len() {
                println!("  {}:{} │ {}", self.filename, line, self.source_lines[line - 1]);
            }
            // 汇编
            println!("  PC={:04} │ 0x{:08X}", pc, word);
        }
    }

    fn current_source_line(&self) -> usize {
        self.source_line_for_pc(self.engine.pc())
    }

    fn source_line_for_pc(&self, pc: usize) -> usize {
        if pc < self.source_map.len() {
            self.source_map[pc]
        } else {
            0
        }
    }
}
