//! COUNPRE64 虚拟执行引擎
//!
//! 直接在宿主机上执行 COUNPRE64 原生指令
//! UART STORE → 标准输出
//! HALT (JUMP 0) → 正常退出
//!
//! 这不是模拟器——这是 COUNPRE64 指令集的原生执行

use std::collections::HashMap;

/// UART TX 地址（与 FPGA 一致）
const UART_TX_ADDR: i64 = 0x100;

/// 栈初始地址
const STACK_INIT: i64 = 0x800;

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
        }
    }

    /// 执行到 HALT，返回输出字符串和执行统计
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
            0x6 => (a as u64).wrapping_shr((b & 63) as u32) as i64, // SHR
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
                let val = *self.memory.get(&addr).unwrap_or(&0);
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
