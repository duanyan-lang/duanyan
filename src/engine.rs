//! COUNPRE64 虚拟执行引擎
//!
//! 直接在宿主机上执行 COUNPRE64 原生指令
//! UART STORE → 标准输出
//! HALT (JUMP 0) → 正常退出
//!
//! 这不是模拟器——这是 COUNPRE64 指令集的原生执行

use std::collections::HashMap;
use crate::hardware_sim::HardwareSimulator;

/// UART TX 地址（与 FPGA 一致）
const UART_TX_ADDR: i64 = 0x100;

/// 栈初始地址
const STACK_INIT: i64 = 0x800;

/// async 任务栈池：每任务独立栈区间大小（字节），位于主栈之上互不重叠。
/// FUNCTION_FRAME_SIZE=144，1024 字节约容 7 层嵌套调用帧。
const TASK_STACK_SIZE: i64 = 1024;
/// async 任务栈槽上限（静态池，与并发任务上限一致，零动态分配）
const TASK_STACK_SLOTS: usize = 16;

/// 最大执行指令数（防止死循环）
const MAX_CYCLES: u64 = 10_000_000;

/// COUNPRE64 虚拟执行引擎
pub struct Engine {
    regs: [i64; 16],
    pc: usize,
    memory: HashMap<i64, i64>,
    instructions: Vec<u32>,
    cycles: u64,
    output: String,
    halted: bool,
    pending_num: bool,  // 上一条 UART 输出是数字标记 0xFF
    /// 硬件模拟器 (Phase D)
    hw_sim: HardwareSimulator,
    /// 硬件模拟模式
    hw_mode: bool,
    // ===== 债2清偿: await 真悬停 (协作式任务调度器) =====
    /// 虚拟时钟 (ms)，YIELD_T 推进，确定性可复现
    vtime_ms: i64,
    /// SPAWN 派生的 async 任务 (静态上限 16, 零动态分配纲领不变)
    tasks: Vec<EngineTask>,
    /// 当前执行中的任务索引 (None = 主程序)
    current_task: Option<usize>,
    /// 挂起事件日志 (task_id, 挂起时刻 ms, 时长 ms)，task_id=0 表示主程序
    suspend_log: Vec<(usize, i64, i64)>,
    /// 任务栈槽占用位图（静态池，true=已占用），保证并发任务栈帧互不重叠
    stack_slots: [bool; TASK_STACK_SLOTS],
}

/// 引擎内 async 任务（SPAWN 派生，YIELD_T 挂起，TRET 结束）
struct EngineTask {
    pc: usize,
    regs: [i64; 16],
    /// None = 可运行；Some(t) = 睡眠至虚拟时刻 t (ms)
    wake_at: Option<i64>,
    done: bool,
    /// 栈池槽号（任务结束时归还，供后续 SPAWN 复用）
    slot: usize,
}

/// 运行时错误类型
#[derive(Debug)]
pub enum RuntimeError {
    IllegalInstruction { pc: usize, word: u32 },
    MemoryOutOfRange { addr: i64 },
    InfiniteLoop { pc: usize, cycles: u64 },
    StackOverflow { sp: i64 },
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IllegalInstruction { pc, word } =>
                write!(f, "非法指令 @ PC={}: 0x{:08X}", pc, word),
            Self::MemoryOutOfRange { addr } =>
                write!(f, "内存越界: 地址 0x{:X}", addr),
            Self::InfiniteLoop { pc, cycles } =>
                write!(f, "疑似死循环 @ PC={} ({} cycles)", pc, cycles),
            Self::StackOverflow { sp } =>
                write!(f, "栈溢出: SP=0x{:X}", sp),
        }
    }
}

impl Engine {
    pub fn new(instructions: Vec<u32>) -> Self {
        Engine {
            regs: [0i64; 16],
            pc: 0,
            memory: HashMap::new(),
            instructions,
            cycles: 0,
            output: String::new(),
            halted: false,
            pending_num: false,
            hw_sim: HardwareSimulator::new(),
            hw_mode: false,
            vtime_ms: 0,
            tasks: Vec::new(),
            current_task: None,
            suspend_log: Vec::new(),
            stack_slots: [false; TASK_STACK_SLOTS],
        }
    }

    /// 启用硬件模拟模式
    pub fn set_hw_mode(&mut self, enabled: bool) {
        self.hw_mode = enabled;
    }

    /// 获取硬件模拟器引用（用于打印状态）
    pub fn hw_sim(&self) -> &HardwareSimulator {
        &self.hw_sim
    }

    /// 执行到 HALT，返回输出字符串和执行统计
    ///
    /// 债2清偿: 主程序 HALT 后，若存在 SPAWN 派生的 async 任务，
    /// 进入协作式调度器直到全部任务结束（确定性轮转 + 虚拟时钟）。
    pub fn run(&mut self) -> Result<(String, u64), String> {
        // 初始化 SP
        self.regs[2] = STACK_INIT;

        while !self.halted {
            if self.cycles >= MAX_CYCLES {
                return Err(format!("执行超限: {} 条指令（疑似死循环）", MAX_CYCLES));
            }
            if self.pc >= self.instructions.len() {
                break;
            }
            self.step();
            self.cycles += 1;
        }
        self.current_task = None;

        // 主程序结束 → 驱动 async 任务至全部完成
        self.run_task_scheduler()?;

        Ok((self.output.clone(), self.cycles))
    }

    /// 单步执行一条指令
    fn step(&mut self) {
        let word = self.instructions[self.pc];
        let opcode = (word >> 28) & 0xF;

        match opcode {
            0x0..=0x7 | 0xF => self.exec_rr(word, opcode),
            0x8..=0xE => self.exec_ri(word, opcode),
            _ => {} // 不可能到达（opcode 已被 & 0xF 限制）
        }
    }

    fn exec_rr(&mut self, word: u32, opcode: u32) {
        let rs1 = ((word >> 24) & 0xF) as usize;
        let rs2 = ((word >> 20) & 0xF) as usize;
        let rd  = ((word >> 16) & 0xF) as usize;
        let func = (word >> 12) & 0xF;

        let a = self.regs[rs1];
        let b = self.regs[rs2];

        // RET 指令：跳转到 rs1 寄存器中的地址
        if opcode == 0xF && func == 0x1 {
            self.pc = (self.regs[rs1] as usize) / 4;
            return;
        }

        // ===== 债2清偿: await 真悬停指令族 (opcode=0xF) =====
        if opcode == 0xF && func == 0x2 {
            // YIELD_T: 挂起当前任务至虚拟时钟 +a ms（a<=0 视为让出一次）
            let ms = if a > 0 { a } else { 0 };
            let tid = self.current_task.map(|i| i + 1).unwrap_or(0);
            self.suspend_log.push((tid, self.vtime_ms, ms));
            let wake_at = self.vtime_ms + ms;
            match self.current_task {
                Some(i) => {
                    // 保存现场后交出控制权：run_task 检测 current_task=None 返回，
                    // 调度器随后据 wake_at 到期时重新唤醒本任务。
                    self.tasks[i].pc = self.pc + 1;
                    self.tasks[i].regs = self.regs;
                    self.tasks[i].wake_at = Some(wake_at);
                    self.current_task = None;
                }
                None => {
                    // 主程序悬停：推进虚拟时钟后继续（主程序不与任务并发）
                    self.vtime_ms = wake_at;
                    self.pc += 1;
                }
            }
            return;
        }
        if opcode == 0xF && func == 0x3 {
            // TRET: async 任务正常结束
            self.halted = true;
            return;
        }
        if opcode == 0xF && func == 0x4 {
            // SPAWN: 派生 async 任务，入口地址 = regs[rs1]（字节地址）
            // 从静态栈池分配一个空闲槽：槽号在任务生命周期内唯一，
            // 保证挂起任务的栈帧不会被后启动任务覆盖（也不受 retain 索引漂移影响）。
            match (0..TASK_STACK_SLOTS).find(|&s| !self.stack_slots[s]) {
                Some(slot) => {
                    self.stack_slots[slot] = true;
                    let entry = self.regs[rs1] as usize / 4;
                    let mut regs = [0i64; 16];
                    regs[3] = self.regs[3]; // a0
                    regs[4] = self.regs[4]; // a1
                    regs[5] = self.regs[5]; // a2
                    regs[6] = self.regs[6]; // a3
                    regs[7] = self.regs[7]; // a4
                    // 任务栈基址 = 主栈之上第 slot 个区间，向下生长且互不重叠
                    regs[2] = STACK_INIT + (slot as i64 + 1) * TASK_STACK_SIZE;
                    self.tasks.push(EngineTask { pc: entry, regs, wake_at: None, done: false, slot });
                }
                None => {
                    eprintln!("SPAWN 失败: async 任务栈槽已耗尽（上限 {}）", TASK_STACK_SLOTS);
                }
            }
            self.pc += 1;
            return;
        }

        let result = match opcode {
            0x0 => match func {
                0x0 => a.wrapping_add(b),       // ADD
                0x9 => { let s = (b & 63) as u32; ((a as u64).rotate_left(s)) as i64 }, // ROL
                0xA => { let s = (b & 63) as u32; ((a as u64).rotate_right(s)) as i64 }, // ROR
                0xB => if a >= b { a } else { b },  // MAX
                0xC => if a <= b { a } else { b },  // MIN
                0xD => a.abs(),                      // ABS
                0xE => { let bytes = a.to_be_bytes(); i64::from_le_bytes(bytes) }, // SWAP
                0xF => if self.regs[rs2] != 0 { self.regs[rs1] } else { b }, // SEL
                _ => 0,
            },
            0x1 => a.wrapping_sub(b),              // SUB
            0x2 => a & b,                           // AND
            0x3 => a | b,                           // OR
            0x4 => a ^ b,                           // XOR
            0x5 => a.wrapping_shl((b & 63) as u32),// SHL
            0x6 => match func {
                // 位运算债清偿: SHR 家族按 func 分派
                // 0x0=逻辑右移(比较指令既有占用,语义不动)
                0x1 => a.wrapping_shr((b & 63) as u32),                     // SRA  算术右移 (>>)
                0x2 => (a as u64).wrapping_shr((b & 63) as u32) as i64,      // SRLU 无符号右移 (>>>)
                _ => (a as u64).wrapping_shr((b & 63) as u32) as i64,        // SHR  逻辑右移 (既有)
            },
            0x7 => match func {
                0x8  => a.wrapping_mul(b),          // MUL
                0x9  => {
                    let result = (a as i128) * (b as i128);
                    (result >> 64) as i64
                } // MULH
                0xC  => if b != 0 { a / b } else { 0 }, // DIV
                0xD  => if b != 0 { (a as u64 / b as u64) as i64 } else { 0 }, // DIVU
                0xE  => if b != 0 { a % b } else { 0 }, // REM
                0xF  => if b != 0 { ((a as u64) % (b as u64)) as i64 } else { 0 }, // REMU
                _ => 0,
            },
            _ => unreachable!(),
        };

        self.set_reg(rd, result);
        self.pc += 1;
    }

    fn exec_ri(&mut self, word: u32, opcode: u32) {
        let rs = ((word >> 24) & 0xF) as usize;
        let imm_raw = ((word >> 12) & 0xFFF) as i32;
        let imm = sign_extend_12(imm_raw) as i64;
        let rd = ((word >> 8) & 0xF) as usize;

        let cur_pc = self.pc as i64 * 4; // 当前指令的字节地址

        match opcode {
            0x8 => { // ADDI
                let result = self.regs[rs].wrapping_add(imm);
                self.set_reg(rd, result);
                self.pc += 1;
            }
            0x9 => { // ANDI
                let result = self.regs[rs] & imm;
                self.set_reg(rd, result);
                self.pc += 1;
            }
            0xA => { // ORI
                let result = self.regs[rs] | imm;
                self.set_reg(rd, result);
                self.pc += 1;
            }
            0xB => { // LOAD: rd = mem[rs + imm]
                let addr = self.regs[rs].wrapping_add(imm);
                let val = if self.hw_mode && addr >= 0x100 && addr < 0x800 {
                    // Phase D: 硬件地址范围委托给硬件模拟器
                    self.hw_sim.handle_load(addr)
                } else {
                    *self.memory.get(&addr).unwrap_or(&0)
                };
                self.set_reg(rd, val);
                self.pc += 1;
            }
            0xC => { // STORE: mem[rs + imm] = rd
                let addr = self.regs[rs].wrapping_add(imm);
                let val = self.regs[rd];
                if addr == UART_TX_ADDR {
                    // 数字标记协议：0xFF = 下一条是数字
                    if self.pending_num {
                        let s = val.to_string();
                        self.output.push_str(&s);
                        print!("{}", s);
                        self.pending_num = false;
                    } else {
                        let byte = (val & 0xFF) as u8;
                        if byte == 0xFF {
                            // 数字标记，不输出，等待下一个值
                            self.pending_num = true;
                        } else if byte >= 32 && byte < 127 || byte == 10 || byte == 13 || byte == 9 {
                            let ch = byte as char;
                            self.output.push(ch);
                            print!("{}", ch);
                        } else if val != 0 {
                            // 非可打印非标记值 → 当作数字
                            let s = val.to_string();
                            self.output.push_str(&s);
                            print!("{}", s);
                        }
                    }
                } else if self.hw_mode && addr >= 0x200 && addr < 0x800 {
                    // Phase D: 硬件地址范围委托给硬件模拟器
                    self.hw_sim.handle_store(addr, val);
                } else {
                    self.memory.insert(addr, val);
                }
                self.pc += 1;
            }
            0xD => { // BEZ: if rs == 0, PC = PC + imm
                if self.regs[rs] == 0 {
                    let target = cur_pc + imm;
                    if target < 0 {
                        self.halted = true;
                    } else {
                        self.pc = (target as usize) / 4;
                    }
                } else {
                    self.pc += 1;
                }
            }
            0xE => { // JUMP: rd = PC+4, PC = PC + imm
                let return_addr = cur_pc + 4;
                self.set_reg(rd, return_addr);
                let target = cur_pc + imm;
                if target == cur_pc {
                    self.halted = true; // HALT: 跳到自己
                } else if target < 0 {
                    self.halted = true;
                } else {
                    self.pc = (target as usize) / 4;
                }
            }
            _ => unreachable!(),
        }
    }

    fn set_reg(&mut self, rd: usize, val: i64) {
        if rd != 0 {
            self.regs[rd] = val;
        }
    }

    // =========================================================================
    // 债2清偿: 协作式 async 任务调度器
    //
    // 语义约定 (v1):
    //   - 主程序顺序执行至 HALT，期间 SPAWN 只登记不并发（确定性优先）
    //   - 主程序结束后轮转驱动任务：就绪即跑，YIELD_T 挂起，TRET 结束
    //   - 全部任务睡眠时，虚拟时钟直接推进到最早唤醒点（零空转）
    //   - 任务切换点只在 YIELD_T/TRET，切换时保存/恢复 pc+全部寄存器
    // =========================================================================

    /// 运行单个任务时间片：直到 TRET 完成 / YIELD_T 挂起 / 跑飞
    fn run_task(&mut self, idx: usize) {
        self.current_task = Some(idx);
        self.pc = self.tasks[idx].pc;
        self.regs = self.tasks[idx].regs;
        self.halted = false;
        let mut slice_cycles = 0u64;
        while !self.halted && slice_cycles < MAX_CYCLES {
            if self.pc >= self.instructions.len() {
                break; // 跑飞视同结束
            }
            self.step();
            slice_cycles += 1;
            self.cycles += 1;
            // YIELD_T 已把现场写回 tasks[idx] 并置 current_task=None → 交还调度器
            if self.current_task.is_none() {
                return;
            }
        }
        // 走到这里 = TRET(halted) 或跑飞 → 任务完成，归还栈池槽
        self.tasks[idx].done = true;
        let slot = self.tasks[idx].slot;
        self.stack_slots[slot] = false;
        self.current_task = None;
    }

    /// 主程序 HALT 后的任务调度循环
    fn run_task_scheduler(&mut self) -> Result<(), String> {
        let mut guard = 0u64;
        loop {
            guard += 1;
            if guard > 10_000 {
                return Err("async 调度超限：任务始终未结束（疑似死循环）".to_string());
            }
            self.tasks.retain(|t| !t.done);
            if self.tasks.is_empty() {
                return Ok(());
            }
            // 唤醒到期任务
            for t in self.tasks.iter_mut() {
                if let Some(w) = t.wake_at {
                    if w <= self.vtime_ms {
                        t.wake_at = None;
                    }
                }
            }
            // 轮转执行所有就绪任务（每轮每任务至多一个时间片）
            let mut ran = false;
            for i in 0..self.tasks.len() {
                if self.tasks[i].wake_at.is_none() {
                    self.run_task(i);
                    ran = true;
                    if self.cycles >= MAX_CYCLES {
                        return Err(format!("执行超限: {} 条指令（async 任务疑似死循环）", MAX_CYCLES));
                    }
                }
            }
            if ran {
                continue;
            }
            // 全员睡眠 → 虚拟时钟推进到最早唤醒点
            let next_wake = self.tasks.iter().filter_map(|t| t.wake_at).min();
            match next_wake {
                Some(w) => self.vtime_ms = w,
                None => return Ok(()), // 防御：无可运行且无唤醒点
            }
        }
    }

    // ===== 债2清偿: 公开访问器 =====
    /// 虚拟时钟 (ms)
    pub fn vtime(&self) -> i64 { self.vtime_ms }
    /// 挂起事件日志 (task_id, 挂起时刻 ms, 时长 ms)，task_id=0 为主程序
    pub fn suspend_log(&self) -> &[(usize, i64, i64)] { &self.suspend_log }
    /// 尚未结束的 async 任务数
    pub fn active_task_count(&self) -> usize { self.tasks.iter().filter(|t| !t.done).count() }

    // ===== 调试接口 =====
    pub fn pc(&self) -> usize { self.pc }
    pub fn regs(&self) -> &[i64; 16] { &self.regs }
    pub fn reg(&self, idx: usize) -> i64 { self.regs[idx] }
    pub fn cycles(&self) -> u64 { self.cycles }
    pub fn halted(&self) -> bool { self.halted }
    pub fn output(&self) -> &str { &self.output }
    pub fn instruction_count(&self) -> usize { self.instructions.len() }
    pub fn get_instruction(&self, idx: usize) -> Option<u32> {
        self.instructions.get(idx).copied()
    }
    pub fn mem_get(&self, addr: i64) -> i64 {
        *self.memory.get(&addr).unwrap_or(&0)
    }
    pub fn mem_set(&mut self, addr: i64, val: i64) {
        self.memory.insert(addr, val);
    }

    /// 单步执行一条指令，返回是否 halt
    pub fn step_once(&mut self) -> Result<bool, RuntimeError> {
        if self.halted || self.pc >= self.instructions.len() {
            return Ok(true);
        }
        self.step();
        self.cycles += 1;
        Ok(self.halted)
    }

    /// 运行到 halt 或断点
    pub fn run_to_breakpoint(&mut self, breakpoints: &std::collections::HashSet<usize>) -> RunStop {
        loop {
            if self.halted || self.pc >= self.instructions.len() {
                return RunStop::Halted;
            }
            if breakpoints.contains(&self.pc) && self.cycles > 0 {
                return RunStop::Breakpoint(self.pc);
            }
            if self.cycles >= MAX_CYCLES {
                return RunStop::Error(RuntimeError::InfiniteLoop {
                    pc: self.pc,
                    cycles: self.cycles,
                });
            }
            self.step();
            self.cycles += 1;
        }
    }

    /// Phase v1.5: 带事件循环的执行
    ///
    /// 执行主程序后，进入事件循环处理定时器回调和异步任务。
    /// 当没有待处理事件时退出。
    pub fn run_with_event_loop(
        &mut self,
        timer_manager: &mut crate::stdlib::TimerManager,
        async_scheduler: &mut crate::stdlib::AsyncScheduler,
    ) -> Result<(String, u64), String> {
        // 1. 执行主程序直到 HALT
        self.regs[2] = STACK_INIT;
        while !self.halted {
            if self.cycles >= MAX_CYCLES {
                return Err(format!("执行超限: {} 条指令（疑似死循环）", MAX_CYCLES));
            }
            if self.pc >= self.instructions.len() {
                break;
            }
            self.step();
            self.cycles += 1;
        }

        // 2. 事件循环
        let mut loop_iterations = 0u64;
        let max_loop_iterations = 100_000u64; // 防止无限事件循环

        loop {
            loop_iterations += 1;
            if loop_iterations >= max_loop_iterations {
                break; // 安全退出
            }

            // 2a. 推进定时器
            let callbacks = timer_manager.tick(1);

            // 2b. 检查异步任务
            let completed = async_scheduler.poll_all();

            // 2c. 如果没有待处理事件，退出
            if callbacks.is_empty() && async_scheduler.is_empty() {
                break;
            }

            // 2d. 执行定时器回调
            for cb_pc in callbacks {
                let saved_pc = self.pc;
                let saved_halted = self.halted;
                self.pc = cb_pc;
                self.halted = false;
                // 执行回调直到 RET 或 HALT
                let mut cb_cycles = 0u64;
                while !self.halted && cb_cycles < 10000 {
                    if self.pc >= self.instructions.len() {
                        break;
                    }
                    self.step();
                    cb_cycles += 1;
                    self.cycles += 1;
                }
                self.pc = saved_pc;
                self.halted = saved_halted;
            }

            // 2e. 执行异步任务的 poll 函数
            let poll_addrs = async_scheduler.get_poll_addrs();
            for (poll_addr, _ctx_addr) in poll_addrs {
                let saved_pc = self.pc;
                let saved_halted = self.halted;
                self.pc = poll_addr;
                self.halted = false;
                let mut poll_cycles = 0u64;
                while !self.halted && poll_cycles < 10000 {
                    if self.pc >= self.instructions.len() {
                        break;
                    }
                    self.step();
                    poll_cycles += 1;
                    self.cycles += 1;
                }
                self.pc = saved_pc;
                self.halted = saved_halted;
            }
        }

        Ok((self.output.clone(), self.cycles))
    }
}


/// 运行停止原因
#[derive(Debug)]
pub enum RunStop {
    Halted,
    Breakpoint(usize),
    Error(RuntimeError),
}

/// 12-bit 符号扩展
fn sign_extend_12(imm: i32) -> i32 {
    if imm & 0x800 != 0 {
        imm | !0xFFF_i32
    } else {
        imm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_rr(opcode: u8, func: u8, rs1: u8, rs2: u8, rd: u8) -> u32 {
        ((opcode as u32 & 0xF) << 28)
            | ((rs1 as u32 & 0xF) << 24)
            | ((rs2 as u32 & 0xF) << 20)
            | ((rd as u32 & 0xF) << 16)
            | ((func as u32 & 0xF) << 12)
    }

    fn encode_ri(opcode: u8, rs: u8, rd: u8, imm: i16) -> u32 {
        ((opcode as u32 & 0xF) << 28)
            | ((rs as u32 & 0xF) << 24)
            | ((imm as u32 & 0xFFF) << 12)
            | ((rd as u32 & 0xF) << 8)
    }

    #[test]
    fn test_addi_and_halt() {
        // ADDI x3, x0, 42 → x3 = 42
        // JUMP x0, 0       → HALT
        let instrs = vec![
            encode_ri(0x8, 0, 3, 42),
            encode_ri(0xE, 0, 0, 0),
        ];
        let mut eng = Engine::new(instrs);
        let (out, cycles) = eng.run().unwrap();
        assert_eq!(out, "");
        assert_eq!(eng.regs[3], 42);
        assert_eq!(cycles, 2);
    }

    #[test]
    fn test_uart_output() {
        // ADDI x9, x0, 72 ('H')
        // NOP
        // STORE x9, 256(x0) → UART output
        // JUMP 0 → HALT
        let instrs = vec![
            encode_ri(0x8, 0, 9, 72),
            encode_rr(0x0, 0x0, 0, 0, 0),
            encode_ri(0xC, 0, 9, 0x100),
            encode_ri(0xE, 0, 0, 0),
        ];
        let mut eng = Engine::new(instrs);
        let (out, _) = eng.run().unwrap();
        assert_eq!(out, "H");
    }

    #[test]
    fn test_function_call_return() {
        // JUMP x1, +8    → CALL function at addr 8 (PC=0)
        // HALT           → (addr 4)
        // ADDI x8, x0, 99→ function body: rv = 99
        // RET x1         → return to caller
        let instrs = vec![
            encode_ri(0xE, 0, 1, 8),          // JUMP x1, +8 → addr 8
            encode_ri(0xE, 0, 0, 0),          // HALT
            encode_ri(0x8, 0, 8, 99),         // ADDI x8, x0, 99
            encode_rr(0xF, 0x1, 1, 0, 0),     // RET x1
        ];
        let mut eng = Engine::new(instrs);
        let _ = eng.run().unwrap();
        assert_eq!(eng.regs[8], 99);
    }

    #[test]
    fn test_multiply() {
        // ADDI x3, x0, 6
        // ADDI x4, x0, 7
        // MUL x8, x3, x4  → x8 = 42
        // JUMP 0
        let instrs = vec![
            encode_ri(0x8, 0, 3, 6),
            encode_ri(0x8, 0, 4, 7),
            encode_rr(0x7, 0x8, 3, 4, 8),     // MUL x8, x3, x4
            encode_ri(0xE, 0, 0, 0),
        ];
        let mut eng = Engine::new(instrs);
        let _ = eng.run().unwrap();
        assert_eq!(eng.regs[8], 42);
    }
}
