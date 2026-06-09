//! COUNPRE64 指令集架构定义
//! 
//! COUNPRE64 是自主设计的 64 位 CPU 指令集
//! 完全独立于 RISC-V/ARM/MIPS
//!
//! 特性:
//! - 16 个通用寄存器 (x0-x15)
//! - 4-bit FUNC 操作码
//! - 32 位定长指令
//! - 64 位数据处理

use alloc::string::String;

/// COUNPRE64 操作码 (4-bit)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Opcode {
    // 基础运算
    ADD  = 0x0,  // 加法
    SUB  = 0x1,  // 减法
    AND  = 0x2,  // 按位与
    OR   = 0x3,  // 按位或
    XOR  = 0x4,  // 按位异或
    SLL  = 0x5,  // 逻辑左移
    SRL  = 0x6,  // 逻辑右移
    SRA  = 0x7,  // 算术右移
    
    // 立即数运算
    ADDI = 0x8,  // 加立即数
    ANDI = 0x9,  // 与立即数
    ORI  = 0xA,  // 或立即数
    XORI = 0xB,  // 异或立即数
    
    // 乘法扩展 (M Extension)
    MUL  = 0x2,  // 乘法 (FUNC4 区分)
    DIV  = 0x2,  // 除法 (FUNC4 区分)
    
    // 内存访问
    LOAD  = 0xC,  // 加载
    STORE = 0xD,  // 存储
    
    // 分支跳转
    BEQ  = 0xE,  // 相等跳转
    BNE  = 0xE,  // 不等跳转 (FUNC4 区分)
    JAL  = 0xF,  // 跳转并链接
    JALR = 0xF,  // 寄存器间接跳转
    
    // 系统调用
    ECALL = 0x3,  // 环境调用
}

/// FUNC4 子操作码 (4-bit)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Func4 {
    ADD  = 0x0,
    SUB  = 0x1,
    MUL  = 0x0,  // MUL: Opcode=0x2, Func4=0x0
    DIV  = 0x1,  // DIV: Opcode=0x2, Func4=0x1
    BEQ  = 0x0,
    BNE  = 0x1,
}

/// 寄存器 (4-bit, 16个寄存器)
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Register {
    X0 = 0,   // 硬连线为零
    X1 = 1,   // 临时寄存器
    X2 = 2,
    X3 = 3,
    X4 = 4,
    X5 = 5,
    X6 = 6,
    X7 = 7,
    X8 = 8,
    X9 = 9,
    X10 = 10, // 函数参数/返回值
    X11 = 11,
    X12 = 12,
    X13 = 13,
    X14 = 14,
    X15 = 15,
}

/// COUNPRE64 指令格式
#[derive(Debug, Clone)]
pub enum Instruction {
    /// R-Type: 寄存器-寄存器运算
    RType {
        opcode: Opcode,
        func4: Func4,
        rs1: Register,
        rs2: Register,
        rd: Register,
    },
    
    /// I-Type: 立即数运算
    IType {
        opcode: Opcode,
        rs1: Register,
        rd: Register,
        imm: i32,
    },
    
    /// Load/Store: 内存访问
    MemType {
        opcode: Opcode,
        base: Register,
        rd: Register,
        offset: i32,
    },
    
    /// Branch: 条件分支
    BranchType {
        opcode: Opcode,
        func4: Func4,
        rs1: Register,
        rs2: Register,
        imm: i32,
    },
    
    /// Jump: 跳转
    JumpType {
        opcode: Opcode,
        func4: Func4,
        rd: Register,
        imm: i32,
    },
}

impl Instruction {
    /// 编码为 32 位机器码
    pub fn encode(&self) -> u32 {
        match self {
            Instruction::RType { opcode, func4, rs1, rs2, rd } => {
                ((*opcode as u32) << 28)
                    | ((*func4 as u32) << 24)
                    | ((*rs1 as u32) << 20)
                    | ((*rs2 as u32) << 16)
                    | ((*rd as u32) << 12)
            }
            
            Instruction::IType { opcode, rs1, rd, imm } => {
                ((*opcode as u32) << 28)
                    | ((*rs1 as u32) << 24)
                    | ((*rd as u32) << 20)
                    | ((*imm as u32) & 0xFFFFF)
            }
            
            Instruction::MemType { opcode, base, rd, offset } => {
                ((*opcode as u32) << 28)
                    | ((*base as u32) << 24)
                    | ((*rd as u32) << 20)
                    | ((*offset as u32) & 0xFFFFF)
            }
            
            Instruction::BranchType { opcode, func4, rs1, rs2, imm } => {
                ((*opcode as u32) << 28)
                    | ((*func4 as u32) << 24)
                    | ((*rs1 as u32) << 20)
                    | ((*rs2 as u32) << 16)
                    | ((*imm as u32) & 0xFFFF)
            }
            
            Instruction::JumpType { opcode, func4, rd, imm } => {
                ((*opcode as u32) << 28)
                    | ((*func4 as u32) << 24)
                    | ((*rd as u32) << 20)
                    | ((*imm as u32) & 0xFFFFF)
            }
        }
    }
    
    /// 生成 COUNPRE64 汇编代码
    pub fn to_asm(&self) -> String {
        match self {
            Instruction::RType { opcode, rs1, rs2, rd, .. } => {
                format!("{:?} x{:?}, x{:?}, x{:?}", opcode, rd, rs1, rs2)
            }
            Instruction::IType { opcode, rs1, rd, imm } => {
                format!("{:?} x{:?}, x{:?}, {}", opcode, rd, rs1, imm)
            }
            Instruction::MemType { opcode, base, rd, offset } => {
                if *opcode == Opcode::LOAD {
                    format!("LOAD x{:?}, {}(x{:?})", rd, offset, base)
                } else {
                    format!("STORE x{:?}, {}(x{:?})", rd, offset, base)
                }
            }
            Instruction::BranchType { opcode, rs1, rs2, imm, .. } => {
                format!("{:?} x{:?}, x{:?}, {}", opcode, rs1, rs2, imm)
            }
            Instruction::JumpType { opcode, rd, imm, .. } => {
                format!("{:?} x{:?}, {}", opcode, rd, imm)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_encode_add() {
        let instr = Instruction::RType {
            opcode: Opcode::ADD,
            func4: Func4::ADD,
            rs1: Register::X1,
            rs2: Register::X2,
            rd: Register::X3,
        };
        
        let encoded = instr.encode();
        assert_ne!(encoded, 0);
        
        let asm = instr.to_asm();
        assert!(asm.contains("ADD"));
    }
}
