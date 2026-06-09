//! COUNPRE64 后端
//!
//! 指令集定义 + .hex 输出（对齐 FPGA 实际编码）
//! Phase 3: 添加 RET 指令支持

use std::fs;
use std::io::Write;

/// COUNPRE64 指令（对齐 FPGA decoder_v2.v）
#[derive(Debug, Clone)]
pub enum Instruction {
    /// RR型: OP[31:28] RS1[27:24] RS2[23:20] RD[19:16] FUNC[15:12] RESV[11:8] MODE[7:4] SIGN[3:0]
    RR {
        opcode: u8,  // 4-bit, OP[3]=0
        func: u8,    // 4-bit FUNC
        rs1: u8,     // 4-bit 源寄存器1
        rs2: u8,     // 4-bit 源寄存器2
        rd: u8,      // 4-bit 目标寄存器
    },
    /// RI型: OP[31:28] RS[27:24] IMM[23:12] RD[11:8] MODE[7:4] SIGN[3:0]
    RI {
        opcode: u8,  // 4-bit, OP[3]=1
        rs: u8,      // 4-bit 源寄存器
        rd: u8,      // 4-bit 目标寄存器
        imm: i16,    // 12-bit 立即数 (有符号)
    },
}

impl Instruction {
    /// 编码为 32 位机器码
    pub fn encode(&self) -> u32 {
        match self {
            Instruction::RR { opcode, func, rs1, rs2, rd } => {
                let op = (*opcode as u32) & 0xF;
                let r1 = (*rs1 as u32) & 0xF;
                let r2 = (*rs2 as u32) & 0xF;
                let d  = (*rd as u32) & 0xF;
                let f  = (*func as u32) & 0xF;
                // OP[31:28] RS1[27:24] RS2[23:20] RD[19:16] FUNC[15:12] 0[11:0]
                (op << 28) | (r1 << 24) | (r2 << 20) | (d << 16) | (f << 12)
            }
            Instruction::RI { opcode, rs, rd, imm } => {
                let op = (*opcode as u32) & 0xF;
                let r  = (*rs as u32) & 0xF;
                let d  = (*rd as u32) & 0xF;
                let im = (*imm as u32) & 0xFFF; // 12-bit
                // OP[31:28] RS[27:24] IMM[23:12] RD[11:8] 0[7:0]
                (op << 28) | (r << 24) | (im << 12) | (d << 8)
            }
        }
    }

    /// 生成汇编文本
    pub fn to_asm(&self) -> String {
        match self {
            Instruction::RR { opcode, func, rs1, rs2, rd } => {
                let mnemonic = match (opcode, func) {
                    (0x0, 0x0) => "ADD",
                    (0x0, 0x9) => "ROL",
                    (0x0, 0xA) => "ROR",
                    (0x0, 0xB) => "MAX",
                    (0x0, 0xC) => "MIN",
                    (0x0, 0xD) => "ABS",
                    (0x0, 0xE) => "SWAP",
                    (0x0, 0xF) => "SEL",
                    (0x1, _)   => "SUB",
                    (0x2, _)   => "AND",
                    (0x3, _)   => "OR",
                    (0x4, _)   => "XOR",
                    (0x5, _)   => "SHL",
                    (0x6, _)   => "SHR",
                    (0x7, 0x8) => "MUL",
                    (0x7, 0x9) => "MULH",
                    (0x7, 0xC) => "DIV",
                    (0x7, 0xD) => "DIVU",
                    (0x7, 0xE) => "REM",
                    (0x7, 0xF) => "REMU",
                    (0xF, 0x1) => "RET",  // RET: opcode=0xF, func=0x1
                    _ => "???",
                };
                if *opcode == 0xF && *func == 0x1 {
                    // RET 指令特殊格式: RET rs1
                    format!("{:<5} x{}", mnemonic, rs1)
                } else {
                    format!("{:<5} x{}, x{}, x{}", mnemonic, rd, rs1, rs2)
                }
            }
            Instruction::RI { opcode, rs, rd, imm } => {
                let mnemonic = match opcode {
                    0x8 => "ADDI",
                    0x9 => "ANDI",
                    0xA => "ORI",
                    0xB => "LOAD",
                    0xC => "STORE",
                    0xD => "BEQ",
                    0xE => "JUMP",
                    _ => "???",
                };
                match opcode {
                    0xB => format!("{:<5} x{}, {}(x{})", mnemonic, rd, imm, rs),
                    0xC => format!("{:<5} x{}, {}(x{})", mnemonic, rd, imm, rs),
                    _ => format!("{:<5} x{}, x{}, {}", mnemonic, rd, rs, imm),
                }
            }
        }
    }
}

/// 输出汇编文本
pub fn emit_asm(instructions: &[Instruction]) -> String {
    let mut output = String::new();
    for (i, instr) in instructions.iter().enumerate() {
        let code = instr.encode();
        output.push_str(&format!("{:04X}: {:08X}  {}\n", i * 4, code, instr.to_asm()));
    }
    output
}

/// 输出 .hex 文件（Verilog  格式）
pub fn emit_hex(instructions: &[Instruction], path: &str) -> Result<(), String> {
    let mut file = fs::File::create(path)
        .map_err(|e| format!("无法创建文件 '{}': {}", path, e))?;

    // 写入文件头注释
    writeln!(file, "// SNARjs 编译输出 - COUNPRE64 机器码")
        .map_err(|e| e.to_string())?;
    writeln!(file, "// 格式: Verilog  compatible")
        .map_err(|e| e.to_string())?;

    for instr in instructions {
        let code = instr.encode();
        writeln!(file, "{:08X}", code).map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_addi() {
        // ADDI x3, x0, 10  →  OP=0x8, RS=0, IMM=10, RD=3
        let instr = Instruction::RI { opcode: 0x8, rs: 0, rd: 3, imm: 10 };
        let code = instr.encode();
        // 0x8[31:28] 0x0[27:24] 0x00A[23:12] 0x3[11:8] 0x0[7:0]
        assert_eq!(code, 0x8000_A300);
        println!("ADDI x3, x0, 10 = 0x{:08X}", code);
    }

    #[test]
    fn test_encode_add() {
        // ADD x5, x3, x4  →  OP=0x0, RS1=3, RS2=4, RD=5, FUNC=0
        let instr = Instruction::RR { opcode: 0x0, func: 0x0, rs1: 3, rs2: 4, rd: 5 };
        let code = instr.encode();
        // 0x0[31:28] 0x3[27:24] 0x4[23:20] 0x5[19:16] 0x0[15:12] 0[11:0]
        assert_eq!(code, 0x0345_0000);
        println!("ADD x5, x3, x4 = 0x{:08X}", code);
    }

    #[test]
    fn test_encode_ret() {
        // RET x1 (ra)  →  OP=0xF, RS1=1, RS2=0, RD=0, FUNC=1
        let instr = Instruction::RR { opcode: 0xF, func: 0x1, rs1: 1, rs2: 0, rd: 0 };
        let code = instr.encode();
        // 0xF[31:28] 0x1[27:24] 0x0[23:20] 0x0[19:16] 0x1[15:12] 0[11:0]
        assert_eq!(code, 0xF100_1000);
        println!("RET x1 = 0x{:08X}", code);
    }

    #[test]
    fn test_asm_output() {
        let instr = Instruction::RI { opcode: 0x8, rs: 0, rd: 3, imm: 42 };
        let asm = instr.to_asm();
        assert!(asm.contains("ADDI"));
        assert!(asm.contains("x3"));
        assert!(asm.contains("42"));
    }
}
