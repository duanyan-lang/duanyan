//! 代码生成器 (Compiler)
//!
//! 将 AST 编译为 COUNPRE64 指令序列
//! Phase 4: console.log → UART, 数组, 取模, 优化

use crate::parser::{SourceStatement, Program, Statement, Expr, BinOp, MathOp, Function, ArrowBody, TemplateExprPart, SwitchCase};
use crate::backend::Instruction;

/// 将 AST 编译为 COUNPRE64 指令列表
pub fn compile(program: &Program) -> Result<Vec<Instruction>, String> {
    let mut compiler = Compiler::new();
    compiler.compile_program(program)?;
    Ok(compiler.instructions)
}

/// 编译结果（含源码映射）
pub struct CompileResult {
    pub instructions: Vec<Instruction>,
    pub source_map: Vec<usize>,
}

/// 编译并返回源码映射
pub fn compile_with_sourcemap(program: &Program) -> Result<CompileResult, String> {
    let mut compiler = Compiler::new();
    compiler.compile_program(program)?;
    let source_map = compiler.source_map.clone();
    Ok(CompileResult {
        instructions: compiler.instructions,
        source_map,
    })
}

/// 函数信息
struct FunctionInfo {
    name: String,
    param_count: usize,
    addr: usize,  // 函数入口地址（字节）
}

/// 寄存器约定 (COUNPRE64 ABI)
const REG_ZERO: u8 = 0;   // 硬连线零
const REG_RA: u8 = 1;     // 返回地址
const REG_SP: u8 = 2;     // 栈指针
const REG_A0: u8 = 3;     // 参数 a0
const REG_A1: u8 = 4;     // 参数 a1
const REG_A2: u8 = 5;     // 参数 a2
const REG_A3: u8 = 6;     // 参数 a3
const REG_A4: u8 = 7;     // 参数 a4
const REG_RV: u8 = 8;     // 返回值
const SCRATCH_REG_MIN: u8 = 9;  // 临时寄存器起始
const SCRATCH_REG_MAX: u8 = 14; // 临时寄存器结束
const REG_FP: u8 = 15;    // 帧指针

/// UART TX 数据寄存器地址 (内存映射)
const UART_TX_ADDR: i16 = 0x100;  // 对齐 FPGA memory_ctrl 中 UART 映射

/// 堆起始地址 (用于数组分配)
const HEAP_BASE: i16 = 0x200;

/// RI 型指令 12-bit 立即数的可表示范围
const IMM12_MIN: i64 = -2048;
const IMM12_MAX: i64 = 2047;

/// 函数栈帧大小 (字节)：16 字节保存 RA/FP，其余留给局部变量槽
const FUNCTION_FRAME_SIZE: i16 = 144;
/// 顶层 (main) 栈帧大小 (字节)，容量比函数帧大一些
const MAIN_FRAME_SIZE: i16 = 512;

/// 编译器状态
struct Compiler {
    instructions: Vec<Instruction>,
    /// 命名单元（let/const/参数）不再占用物理寄存器，
    /// 而是分配在栈帧上相对 FP 的固定偏移槽位。
    /// 物理寄存器只作为表达式求值期间的短暂 scratch 使用。
    slots: Vec<(String, i16)>,
    next_slot_off: i16,
    frame_size: i16,
    functions: Vec<FunctionInfo>,
    in_function: bool,
    heap_offset: i16,
    source_map: Vec<usize>,
    current_line: usize,
    /// scratch 寄存器空闲池（仅包含 SCRATCH_REG_MIN..=SCRATCH_REG_MAX）
    free_pool: Vec<u8>,
    active_temps: Vec<u8>,
    /// scratch 寄存器耗尽时溢出到栈帧的匿名槽位（从 -frame_size 往上分配）
    scratch_spill_off: i16,
    scratch_spills: Vec<(u8, i16)>, // (被溢出的寄存器, 栈偏移)
    /// Phase 10: 对象属性映射 (var_name -> [prop_names])
    object_props: Vec<(String, Vec<String>)>,
    /// 函数内提前 return 需要修补的 JUMP 指令索引
    return_patches: Vec<usize>,
    /// 循环/switch 嵌套上下文（栈顶为最内层），支撑 break/continue 补丁链
    loop_stack: Vec<LoopContext>,
}

/// break/continue 跳转补丁上下文
#[derive(Default)]
struct LoopContext {
    /// true = switch 上下文（break 对其 NOP 即等价，if-else 链无 fallthrough）
    is_switch: bool,
    break_patches: Vec<usize>,
    continue_patches: Vec<usize>,
}

impl LoopContext {
    fn new_loop() -> Self {
        Self { is_switch: false, break_patches: Vec::new(), continue_patches: Vec::new() }
    }

    fn new_switch() -> Self {
        Self { is_switch: true, break_patches: Vec::new(), continue_patches: Vec::new() }
    }
}

/// 作用域快照（块级作用域进出时保存/恢复）
#[derive(Clone)]
struct ScopeSnapshot {
    slots: Vec<(String, i16)>,
    next_slot_off: i16,
}

/// 条件求值结果
struct CondResult {
    reg: u8,
    zero_means_true: bool,
}

impl Compiler {
    fn new() -> Self {
        Self {
            instructions: Vec::new(),
            slots: Vec::new(),
            next_slot_off: -24, // -8=保存的调用者 FP，-16=保存的 RA，-24 开始才是局部变量槽
            frame_size: FUNCTION_FRAME_SIZE,
            functions: Vec::new(),
            in_function: false,
            heap_offset: HEAP_BASE,
            source_map: Vec::new(),
            current_line: 0,
            free_pool: (SCRATCH_REG_MIN..=SCRATCH_REG_MAX).rev().collect(),
            active_temps: Vec::new(),
            scratch_spill_off: 0,
            scratch_spills: Vec::new(),
            object_props: Vec::new(),
            return_patches: Vec::new(),
            loop_stack: Vec::new(),
        }
    }

    // =========================================================================
    // 变量槽位分配（取代旧版“变量直接占用物理寄存器”的做法）
    // =========================================================================

    /// 查询变量所在栈槽偏移
    fn slot_off(&self, name: &str) -> Option<i16> {
        self.slots.iter().find(|(n, _)| n == name).map(|(_, o)| *o)
    }

    /// 为变量分配一个新的栈槽（局部变量区不能侵入 scratch 溢出区，否则明确报错）
    fn alloc_slot(&mut self, name: &str) -> Result<i16, String> {
        if let Some(off) = self.slot_off(name) {
            return Ok(off);
        }
        let locals_floor = -(self.frame_size / 2 + 8);
        if self.next_slot_off <= locals_floor {
            return Err(format!(
                "栈帧局部变量空间不足：变量 '{}' 无法分配，请拆分为多个函数", name
            ));
        }
        let off = self.next_slot_off;
        self.next_slot_off -= 8;
        self.slots.push((name.to_string(), off));
        Ok(off)
    }

    fn enter_scope(&self) -> ScopeSnapshot {
        ScopeSnapshot { slots: self.slots.clone(), next_slot_off: self.next_slot_off }
    }

    fn leave_scope(&mut self, snap: ScopeSnapshot) {
        self.slots = snap.slots;
        self.next_slot_off = snap.next_slot_off;
    }

    // =========================================================================
    // scratch 寄存器分配（仅用于表达式求值期间的短期临时值）
    // =========================================================================

    fn alloc_temp(&mut self) -> Result<u8, String> {
        let reg = self.get_scratch_reg()?;
        self.active_temps.push(reg);
        Ok(reg)
    }

    /// 同 alloc_temp，但保证不会返回 exclude 中的寄存器。当前实现不做活跃临时之间
    /// 的隐式溢出/重载（那需要真正的活跃性分析），因此池已空时直接明确报错，
    /// 避免顶掉仍在使用中的临时寄存器而静默生成错误代码。
    fn alloc_temp_excluding(&mut self, exclude: &[u8]) -> Result<u8, String> {
        if let Some(pos) = self.free_pool.iter().rposition(|r| !exclude.contains(r)) {
            let reg = self.free_pool.remove(pos);
            self.active_temps.push(reg);
            return Ok(reg);
        }
        Err(format!(
            "表达式过于复杂：可用 scratch 寄存器不足（已占用 {:?}，需要避开 {:?}）",
            self.active_temps, exclude
        ))
    }

    /// 获取一个 scratch 寄存器。scratch 池已空时不做隐式驱逐（会破坏其他
    /// 仍持有该寄存器的调用方），而是直接报错。溢出到栈帧的能力目前
    /// 仅留给显式的 spill/reload 路径（后续完善活跃性分析后再接入）。
    fn get_scratch_reg(&mut self) -> Result<u8, String> {
        if let Some(reg) = self.free_pool.pop() {
            return Ok(reg);
        }
        Err("表达式过于复杂：scratch 寄存器已耗尽，请拆分为多个中间变量".to_string())
    }

    fn spill_scratch(&mut self, reg: u8) -> Result<(), String> {
        // 溢出区从 -(frame_size/2+8) 向下增长，不得越到栈帧底部
        if self.scratch_spill_off < -self.frame_size {
            return Err("栈帧溢出空间不足：表达式过于复杂，请拆分为多个中间变量".to_string());
        }
        let off = self.scratch_spill_off;
        self.scratch_spill_off -= 8;
        self.scratch_spills.push((reg, off));
        self.emit_store(reg, REG_FP, off);
        Ok(())
    }

    fn reload_scratch(&mut self, reg: u8) -> Result<(), String> {
        let off = self.scratch_spills.iter()
            .rev()
            .find(|(r, _)| *r == reg)
            .map(|(_, o)| *o)
            .ok_or_else(|| format!("内部错误：寄存器 {} 未记录溢出位置", reg))?;
        self.emit_load(reg, REG_FP, off);
        Ok(())
    }

    /// 释放临时寄存器
    fn free_temp(&mut self, reg: u8) {
        self.active_temps.retain(|&r| r != reg);
        // 若该寄存器之前被溢出到过栈，先恢复到寄存器本身，再回收进空闲池
        if self.scratch_spills.iter().any(|(r, _)| *r == reg) {
            // 恢复不影响分配逻辑（值将在下一次使用前由 reload_scratch 从栈取回）
            self.scratch_spills.retain(|(r, _)| *r != reg);
        }
        if !self.free_pool.contains(&reg) {
            self.free_pool.push(reg);
        }
    }

    /// 获取变量值到指定寄存器（变量统一从栈槽加载）
    fn get_var_to_reg(&mut self, name: &str, rd: u8) -> Result<(), String> {
        match self.slot_off(name) {
            Some(off) => {
                self.emit_load(rd, REG_FP, off);
                Ok(())
            }
            None => Err(format!("未定义变量 '{}'", name)),
        }
    }

    /// 将寄存器中的值存回变量栈槽
    fn set_var_from_reg(&mut self, name: &str, src_reg: u8) -> Result<(), String> {
        match self.slot_off(name) {
            Some(off) => {
                self.emit_store(src_reg, REG_FP, off);
                Ok(())
            }
            None => Err(format!("未定义变量 '{}'", name)),
        }
    }

    // =========================================================================
    // 大整数常量装载（不再静默截断为 12-bit）
    // =========================================================================

    fn load_const(&mut self, rd: u8, value: i64) -> Result<(), String> {
        if value >= IMM12_MIN && value <= IMM12_MAX {
            self.emit_addi(rd, REG_ZERO, value as i16);
            return Ok(());
        }
        // 取一个能装进 12-bit 有符号范围的余数
        let lo = ((value + 2048) % 4096) - 2048;
        let hi = (value - lo) / 4096;
        self.load_const(rd, hi)?;
        let shift_reg = self.alloc_temp_excluding(&[rd])?;
        self.emit_addi(shift_reg, REG_ZERO, 12);
        self.emit_nop();
        // SHL rd, rd, shift_reg
        self.push_instr(Instruction::RR {
            opcode: 0x5, func: 0x0, rs1: rd, rs2: shift_reg, rd,
        });
        self.free_temp(shift_reg);
        self.emit_nop();
        self.emit_addi(rd, rd, lo as i16);
        Ok(())
    }

    fn get_function(&self, name: &str) -> Option<&FunctionInfo> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// 设置当前源码行号
    fn set_line(&mut self, line: usize) {
        self.current_line = line;
    }

    /// 发射指令并记录源码映射
    fn push_instr(&mut self, instr: Instruction) {
        self.instructions.push(instr);
        self.source_map.push(self.current_line);
    }

    /// 分配堆空间（编译期）
    fn alloc_heap(&mut self, size: i16) -> i16 {
        let addr = self.heap_offset;
        self.heap_offset += size * 8; // 每元素 8 字节 (64-bit)
        addr
    }

    // =========================================================================
    // 指令发射
    // =========================================================================

    fn emit_nop(&mut self) {
        self.push_instr(Instruction::RR {
            opcode: 0x0, func: 0x0, rs1: 0, rs2: 0, rd: 0,
        });
    }

    fn emit_bez(&mut self, rs: u8, byte_offset: i16) {
        self.push_instr(Instruction::RI {
            opcode: 0xD, rs, rd: 0, imm: byte_offset,
        });
    }

    fn emit_jump(&mut self, byte_offset: i16) {
        self.push_instr(Instruction::RI {
            opcode: 0xE, rs: 0, rd: REG_RA, imm: byte_offset,
        });
    }

    fn emit_ret(&mut self) {
        self.push_instr(Instruction::RR {
            opcode: 0xF, func: 0x1, rs1: REG_RA, rs2: 0, rd: 0,
        });
    }

    fn emit_addi(&mut self, rd: u8, rs: u8, imm: i16) {
        self.push_instr(Instruction::RI {
            opcode: 0x8, rs, rd, imm,
        });
    }

    fn emit_store(&mut self, rs: u8, base: u8, offset: i16) {
        self.push_instr(Instruction::RI {
            opcode: 0xC, rs: base, rd: rs, imm: offset,
        });
    }

    fn emit_load(&mut self, rd: u8, base: u8, offset: i16) {
        self.push_instr(Instruction::RI {
            opcode: 0xB, rs: base, rd, imm: offset,
        });
    }

    fn patch_offset(&mut self, instr_idx: usize, byte_offset: i16) {
        match &mut self.instructions[instr_idx] {
            Instruction::RI { imm, .. } => { *imm = byte_offset; }
            _ => panic!("只能修补 RI 型指令的 offset"),
        }
    }

    fn current_addr(&self) -> usize {
        self.instructions.len() * 4
    }

    fn calc_offset(&self, from_idx: usize, to_idx: usize) -> i16 {
        let from_addr = from_idx as i32 * 4;
        let to_addr = to_idx as i32 * 4;
        (to_addr - from_addr) as i16
    }

    // =========================================================================
    // 程序编译
    // =========================================================================

    fn compile_program(&mut self, program: &Program) -> Result<(), String> {
        // 第一遍：扫描函数定义
        for sf in &program.functions {
            self.functions.push(FunctionInfo {
                name: sf.func.name.clone(),
                param_count: sf.func.params.len(),
                addr: 0,
            });
        }

        // 跳转到 main（跳过函数定义）
        let jump_to_main_idx = self.instructions.len();
        if !program.functions.is_empty() {
            self.emit_jump(0); // placeholder
        }

        // 编译函数体
        for sf in &program.functions {
            let func = &sf.func;
            let func_addr = self.current_addr();
            if let Some(info) = self.functions.iter_mut().find(|f| f.name == func.name) {
                info.addr = func_addr;
            }
            self.compile_function(func, sf.line)?;
        }

        // Patch 入口跳转
        if !program.functions.is_empty() {
            let main_addr = self.current_addr();
            let offset = self.calc_offset(jump_to_main_idx, main_addr / 4);
            self.patch_offset(jump_to_main_idx, offset);
        }

        // ===== 顶层 (main) 序言：建立与函数一致的栈帧 =====
        self.reset_frame(MAIN_FRAME_SIZE);

        // 编译主程序
        for ss in &program.statements {
            self.set_line(ss.line);
            self.compile_statement(&ss.stmt)?;
        }

        // ===== 顶层 (main) 结语：先恢复 SP，再恢复 FP =====
        self.emit_addi(REG_SP, REG_FP, 0);
        self.emit_load(REG_FP, REG_FP, -8);

        // 程序结束：halt (JUMP 0)
        let halt_idx = self.instructions.len();
        self.emit_jump(0);
        let halt_offset = self.calc_offset(halt_idx, halt_idx);
        self.patch_offset(halt_idx, halt_offset);

        Ok(())
    }

    /// 为当前帧（函数体或顶层 main）重置变量槽/scratch 寄存器/溢出区状态
    fn reset_frame(&mut self, frame_size: i16) {
        self.frame_size = frame_size;
        self.slots.clear();
        self.next_slot_off = -24; // -8=旧FP, -16=RA, -24 开始为局部变量槽（向下增长）
        self.free_pool = (SCRATCH_REG_MIN..=SCRATCH_REG_MAX).rev().collect();
        self.active_temps.clear();
        // 匿名溢出区与局部变量区各占栈帧一半，中间以 -frame_size/2-8 为界，
        // 不会互相侵入（两边各自越界时都会明确报错）。
        self.scratch_spill_off = -(frame_size / 2 + 8);
        self.scratch_spills.clear();
        self.return_patches.clear();
    }

    fn compile_function(&mut self, func: &Function, func_line: usize) -> Result<(), String> {
        self.set_line(func_line);
        self.in_function = true;
        // 函数体独立编译：break/continue 不得链接到外层循环补丁链
        self.loop_stack.clear();
        self.reset_frame(FUNCTION_FRAME_SIZE);

        // Prologue
        self.emit_addi(REG_SP, REG_SP, -FUNCTION_FRAME_SIZE);
        self.emit_store(REG_FP, REG_SP, FUNCTION_FRAME_SIZE - 8);
        self.emit_store(REG_RA, REG_SP, FUNCTION_FRAME_SIZE - 16);
        self.emit_addi(REG_FP, REG_SP, FUNCTION_FRAME_SIZE);

        // 参数：拷贝到独立栈槽，从此以后 A0..A4 只是易失寄存器，
        // 调用者/被调用者都不再依赖它们的內容长期存活。
        for (i, param) in func.params.iter().enumerate() {
            let reg = REG_A0 + i as u8;
            if reg > REG_A4 {
                return Err(format!("函数 '{}' 参数过多（最多5个）", func.name));
            }
            let off = self.alloc_slot(param)?;
            self.emit_store(reg, REG_FP, off);
        }

        // 函数体
        for ss in &func.body {
            self.set_line(ss.line);
            self.compile_statement(&ss.stmt)?;
        }

        // Epilogue（所有 return 跳到这里）
        let epilogue_idx = self.instructions.len();
        let pending_returns: Vec<usize> = self.return_patches.clone();
        for jump_idx in pending_returns {
            let offset = self.calc_offset(jump_idx, epilogue_idx);
            self.patch_offset(jump_idx, offset);
        }
        self.return_patches.clear();
        self.emit_addi(REG_SP, REG_FP, 0);
        self.emit_load(REG_RA, REG_FP, -16);
        self.emit_load(REG_FP, REG_FP, -8);
        self.emit_ret();

        self.in_function = false;
        Ok(())
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), String> {
        match stmt {
            Statement::Let { name, value } => {
                // Phase 10: track object property layout
                if let Expr::ObjectLit { keys, .. } = value {
                    self.object_props.push((name.clone(), keys.clone()));
                }
                let off = self.alloc_slot(name)?;
                let tmp = self.alloc_temp()?;
                self.compile_expr_into(value, tmp)?;
                self.emit_store(tmp, REG_FP, off);
                self.free_temp(tmp);
                Ok(())
            }
            // Phase A.5: const 与 let 相同（不可变在编译期不强制）
            Statement::Const { name, value } => {
                let off = self.alloc_slot(name)?;
                let tmp = self.alloc_temp()?;
                self.compile_expr_into(value, tmp)?;
                self.emit_store(tmp, REG_FP, off);
                self.free_temp(tmp);
                Ok(())
            }
            Statement::Assign { name, value } => {
                if self.slot_off(name).is_none() {
                    return Err(format!("未定义变量 '{}'", name));
                }
                let tmp = self.alloc_temp()?;
                self.compile_expr_into(value, tmp)?;
                self.set_var_from_reg(name, tmp)?;
                self.free_temp(tmp);
                Ok(())
            }
            Statement::IndexAssign { name, index, value } => {
                self.compile_index_assign(name, index, value)
            }
            Statement::Return(expr) => {
                self.compile_expr_into(expr, REG_RV)?;
                let jump_idx = self.instructions.len();
                self.emit_jump(0); // 占位，后续修补到 epilogue
                self.return_patches.push(jump_idx);
                Ok(())
            }
            Statement::Expr(expr) => {
                let tmp = self.alloc_temp()?;
                self.compile_expr_into(expr, tmp)?;
                self.free_temp(tmp);
                Ok(())
            }
            Statement::For { init, condition, update, body } => {
                self.compile_for(init, condition, update, body)
            }
            Statement::PostInc(name) => {
                // name = name + 1：从栈槽加载，加 1，写回栈槽
                let tmp = self.alloc_temp()?;
                self.get_var_to_reg(name, tmp)?;
                self.emit_addi(tmp, tmp, 1);
                self.set_var_from_reg(name, tmp)?;
                self.free_temp(tmp);
                Ok(())
            }
            Statement::PostDec(name) => {
                // name = name - 1
                let tmp = self.alloc_temp()?;
                self.get_var_to_reg(name, tmp)?;
                self.emit_addi(tmp, tmp, -1);
                self.set_var_from_reg(name, tmp)?;
                self.free_temp(tmp);
                Ok(())
            }
            Statement::PropAssign { object, property, value } => {
                self.compile_prop_assign(object, property, value)
            }
            Statement::If { condition, then_body, else_body } => {
                self.compile_if(condition, then_body, else_body.as_deref())
            }
            Statement::While { condition, body } => {
                // body is Vec<SourceStatement>
                self.compile_while(condition, body)
            }
            Statement::ConsoleLog(args) => {
                self.compile_console_log(args)
            }
            // Phase A.3: switch 编译为 if-else 链
            Statement::Switch { discriminant, cases } => {
                self.compile_switch(discriminant, cases)
            }
            // break：循环内生成跳转补丁；switch 内（if-else 链无 fallthrough）NOP 即等价
            Statement::Break => {
                let in_loop = matches!(self.loop_stack.last(), Some(ctx) if !ctx.is_switch);
                if in_loop {
                    let idx = self.instructions.len();
                    self.emit_jump(0);
                    if let Some(ctx) = self.loop_stack.last_mut() {
                        ctx.break_patches.push(idx);
                    }
                } else {
                    self.emit_nop();
                }
                Ok(())
            }
            // continue：JS 语义下即使位于 switch 内也作用于最内层循环
            Statement::Continue => {
                let has_loop = self.loop_stack.iter().rev().any(|c| !c.is_switch);
                if !has_loop {
                    return Err("continue 只能用在循环内".to_string());
                }
                let idx = self.instructions.len();
                self.emit_jump(0);
                if let Some(ctx) = self.loop_stack.iter_mut().rev().find(|c| !c.is_switch) {
                    ctx.continue_patches.push(idx);
                }
                Ok(())
            }
            // Phase A.4: import 为编译期指令，不产生机器码
            Statement::Import { .. } => {
                Ok(())
            }
            // Phase A.4: export 编译内部语句
            Statement::Export { item } => {
                self.compile_statement(item)
            }
            // Phase A.6: 解构赋值 let { a, b } = obj;
            Statement::DestructLet { names, source } => {
                // 计算源表达式到临时寄存器
                let src_reg = self.alloc_temp()?;
                self.compile_expr_into(source, src_reg)?;
                // 每个 name 对应源对象的一个属性（偏移 = index * 8）
                for (i, name) in names.iter().enumerate() {
                    let offset = (i as i16) * 8;
                    let off = self.alloc_slot(name)?;
                    let tmp = self.alloc_temp_excluding(&[src_reg])?;
                    self.emit_nop();
                    self.emit_load(tmp, src_reg, offset);
                    self.emit_store(tmp, REG_FP, off);
                    self.free_temp(tmp);
                }
                self.free_temp(src_reg);
                Ok(())
            }
        }
    }

    // =========================================================================
    // Phase 4: console.log → UART 输出
    // =========================================================================

    fn compile_console_log(&mut self, args: &[Expr]) -> Result<(), String> {
        for arg in args {
            match arg {
                Expr::StringLit(s) => {
                    // 字符串：逐字符 STORE 到 UART TX
                    let tmp = self.alloc_temp()?;
                    for byte in s.bytes() {
                        self.emit_addi(tmp, REG_ZERO, byte as i16);
                        self.emit_nop();
                        self.emit_store(tmp, REG_ZERO, UART_TX_ADDR);
                    }
                    // 换行符
                    self.emit_addi(tmp, REG_ZERO, 0x0A);
                    self.emit_nop();
                    self.emit_store(tmp, REG_ZERO, UART_TX_ADDR);
                    self.free_temp(tmp);
                }
                Expr::Number(n) => {
                    // 编译期数字：转为字符串后逐字符输出到 UART
                    let s = n.to_string();
                    let tmp = self.alloc_temp()?;
                    for byte in s.bytes() {
                        self.emit_addi(tmp, REG_ZERO, byte as i16);
                        self.emit_nop();
                        self.emit_store(tmp, REG_ZERO, UART_TX_ADDR);
                    }
                    // 换行
                    self.emit_addi(tmp, REG_ZERO, 0x0A);
                    self.emit_nop();
                    self.emit_store(tmp, REG_ZERO, UART_TX_ADDR);
                    self.free_temp(tmp);
                }
                _ => {
                    // 通用表达式：分配专用寄存器避免冲突
                    let expr_reg = self.alloc_temp()?;
                    let marker_reg = self.alloc_temp()?;
                    // 发数字标记 0xFF
                    self.emit_addi(marker_reg, REG_ZERO, -1_i16);
                    self.emit_nop();
                    self.emit_store(marker_reg, REG_ZERO, UART_TX_ADDR);
                    // 计算表达式结果
                    self.compile_expr_into(arg, expr_reg)?;
                    self.emit_nop();
                    self.emit_store(expr_reg, REG_ZERO, UART_TX_ADDR);
                    // 换行
                    self.emit_addi(marker_reg, REG_ZERO, 0x0A);
                    self.emit_nop();
                    self.emit_store(marker_reg, REG_ZERO, UART_TX_ADDR);
                    // 回收临时寄存器
                    self.free_temp(marker_reg);
                    self.free_temp(expr_reg);
                }
            }
        }
        Ok(())
    }

    // =========================================================================
    // Phase 4: 数组支持
    // =========================================================================

    fn compile_index_assign(&mut self, name: &str, index: &Expr, value: &Expr) -> Result<(), String> {
        // 数组变量现在存在栈槽里，先把它加载到一个 scratch 寄存器
        let base_reg = self.alloc_temp()?;
        self.get_var_to_reg(name, base_reg)?;

        // 计算 index
        let idx_reg = self.alloc_temp()?;
        self.compile_expr_into(index, idx_reg)?;

        // 计算值
        let val_reg = self.alloc_temp()?;
        self.compile_expr_into(value, val_reg)?;

        // index * 8 (每元素 8 字节) → 左移 3 位
        let shift_reg = self.alloc_temp_excluding(&[base_reg, idx_reg, val_reg])?;
        self.emit_addi(shift_reg, REG_ZERO, 3);
        self.emit_nop();
        self.push_instr(Instruction::RR {
            opcode: 0x5, func: 0x0, rs1: idx_reg, rs2: shift_reg, rd: idx_reg,
        });

        // ADD base + offset
        self.emit_nop();
        self.push_instr(Instruction::RR {
            opcode: 0x0, func: 0x0, rs1: base_reg, rs2: idx_reg, rd: idx_reg,
        });

        // STORE value, 0(addr)
        self.emit_nop();
        self.emit_store(val_reg, idx_reg, 0);

        self.free_temp(shift_reg);
        self.free_temp(val_reg);
        self.free_temp(idx_reg);
        self.free_temp(base_reg);
        Ok(())
    }

    fn compile_array_literal(&mut self, elements: &[Expr], rd: u8) -> Result<(), String> {
        // 静态分配堆空间
        let base_addr = self.alloc_heap(elements.len() as i16);

        // 将基址加载到 rd
        self.emit_addi(rd, REG_ZERO, base_addr);

        // 初始化各元素：使用专门的 scratch，不再复用固定寄存器（避免与 rd 冲突）
        for (i, elem) in elements.iter().enumerate() {
            let tmp = self.alloc_temp_excluding(&[rd])?;
            self.compile_expr_into(elem, tmp)?;
            let offset = (i as i16) * 8;
            self.emit_nop();
            self.emit_store(tmp, rd, offset);
            self.free_temp(tmp);
        }

        Ok(())
    }

    fn compile_array_index(&mut self, array: &Expr, index: &Expr, rd: u8) -> Result<(), String> {
        // 计算数组基址
        let base_reg = self.alloc_temp()?;
        self.compile_expr_into(array, base_reg)?;

        // 计算 index
        let idx_reg = self.alloc_temp()?;
        self.compile_expr_into(index, idx_reg)?;

        // index * 8
        let shift_reg = self.alloc_temp()?;
        self.emit_addi(shift_reg, REG_ZERO, 3);
        self.emit_nop();
        self.push_instr(Instruction::RR {
            opcode: 0x5, func: 0x0, rs1: idx_reg, rs2: shift_reg, rd: idx_reg,
        });

        // base + offset
        self.emit_nop();
        self.push_instr(Instruction::RR {
            opcode: 0x0, func: 0x0, rs1: base_reg, rs2: idx_reg, rd: idx_reg,
        });

        // LOAD rd, 0(addr)
        self.emit_nop();
        self.emit_load(rd, idx_reg, 0);

        self.free_temp(shift_reg);
        self.free_temp(idx_reg);
        self.free_temp(base_reg);
        Ok(())
    }

    // =========================================================================
    // 函数调用
    // =========================================================================

    fn compile_call(&mut self, name: &str, args: &[Expr], rd: u8) -> Result<(), String> {
        // Phase C.3: 硬件 API 静态链接 — 拦截 hardy:hw 调用
        if let Some(hw_call) = self.try_compile_hw_call(name, args, rd)? {
            return Ok(());
        }

        let func_info = self.get_function(name)
            .ok_or_else(|| format!("未定义函数 '{}'", name))?;
        let func_addr = func_info.addr;
        let param_count = func_info.param_count;

        if args.len() != param_count {
            return Err(format!("函数 '{}' 期望 {} 个参数，得到 {} 个", name, param_count, args.len()));
        }

        for (i, arg) in args.iter().enumerate() {
            let arg_reg = REG_A0 + i as u8;
            self.compile_expr_into(arg, arg_reg)?;
        }

        if !args.is_empty() {
            self.emit_nop();
        }

        let current_pc = self.current_addr();
        let offset = (func_addr as i32 - current_pc as i32) as i16;
        self.emit_jump(offset);

        if rd != REG_RV {
            self.emit_addi(rd, REG_RV, 0);
        }

        Ok(())
    }

    // =========================================================================
    // Phase C.3: 硬件 API 静态链接
    // 拦截 hardy:hw 调用，生成内存映射 I/O 指令
    // =========================================================================

    fn try_compile_hw_call(&mut self, name: &str, args: &[Expr], rd: u8) -> Result<Option<()>, String> {
        use crate::hardware_api::counpre64_map as hw;

        match name {
            // GPIO.output(pin) → 设置方向寄存器 (写入pin号)
            "GPIO.output" if args.len() == 1 => {
                let pin_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], pin_reg)?;
                // 写入 pin 号到 GPIO_DIR → 该 pin 设为 output
                self.emit_store(pin_reg, 0, hw::GPIO_DIR);
                self.emit_addi(rd, 0, 1); // 返回 1
                self.free_temp(pin_reg);
                Ok(Some(()))
            }
            // GPIO.high(pin) → 输出高 (写入pin号)
            "GPIO.high" if args.len() == 1 => {
                let pin_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], pin_reg)?;
                // 写入 pin 号到 GPIO_OUT → 该 pin 设高
                self.emit_store(pin_reg, 0, hw::GPIO_OUT);
                self.emit_addi(rd, 0, 1);
                self.free_temp(pin_reg);
                Ok(Some(()))
            }
            // GPIO.low(pin) → 输出低 (写入pin号)
            "GPIO.low" if args.len() == 1 => {
                let pin_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], pin_reg)?;
                // 写入 pin 号到 GPIO_LOW → 该 pin 设低
                self.emit_store(pin_reg, 0, hw::GPIO_LOW);
                self.emit_addi(rd, 0, 0);
                self.free_temp(pin_reg);
                Ok(Some(()))
            }
            // GPIO.read(pin) → 读取输入寄存器
            "GPIO.read" if args.len() == 1 => {
                self.emit_load(rd, 0, hw::GPIO_IN);
                Ok(Some(()))
            }
            // UART.init(baud)
            "UART.init" if args.len() == 1 => {
                let baud_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], baud_reg)?;
                self.emit_store(baud_reg, 0, hw::UART_BAUD);
                self.emit_addi(rd, 0, 1); // 返回 1 表示成功
                self.free_temp(baud_reg);
                Ok(Some(()))
            }
            // Timer.delay(ms) → 简化为 NOP 循环 (实际硬件用忙等待)
            "Timer.delay" if args.len() == 1 => {
                let ms_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], ms_reg)?;
                // 简化: 写入 TIMER_PERIOD 并启动
                self.emit_store(ms_reg, 0, hw::TIMER_PERIOD);
                let ctrl_reg = self.alloc_temp()?;
                self.emit_addi(ctrl_reg, 0, 1); // enable
                self.emit_store(ctrl_reg, 0, hw::TIMER_CTRL);
                self.emit_addi(rd, 0, 0);
                self.free_temp(ms_reg);
                self.free_temp(ctrl_reg);
                Ok(Some(()))
            }
            // PWM.duty(pin, value)
            "PWM.duty" if args.len() == 2 => {
                let pin_reg = self.alloc_temp()?;
                let duty_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], pin_reg)?;
                self.compile_expr_into(&args[1], duty_reg)?;
                self.emit_store(duty_reg, 0, hw::PWM_DUTY);
                self.emit_addi(rd, 0, 1);
                self.free_temp(pin_reg);
                self.free_temp(duty_reg);
                Ok(Some(()))
            }
            // ADC.read(channel)
            "ADC.read" if args.len() == 1 => {
                let ch_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], ch_reg)?;
                self.emit_store(ch_reg, 0, hw::ADC_CHANNEL);
                self.emit_load(rd, 0, hw::ADC_VALUE);
                self.free_temp(ch_reg);
                Ok(Some(()))
            }
            // ADC.channel(ch) → 设置 ADC 通道
            "ADC.channel" if args.len() == 1 => {
                let ch_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], ch_reg)?;
                self.emit_store(ch_reg, 0, hw::ADC_CHANNEL);
                self.emit_addi(rd, 0, 1);
                self.free_temp(ch_reg);
                Ok(Some(()))
            }
            // UART.write(byte) → 发送一个字节
            "UART.write" if args.len() == 1 => {
                let data_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], data_reg)?;
                self.emit_store(data_reg, 0, hw::UART_TX_DATA);
                self.emit_addi(rd, 0, 1);
                self.free_temp(data_reg);
                Ok(Some(()))
            }
            // GPIO.input(pin) → 设为输入
            "GPIO.input" if args.len() == 1 => {
                let pin_reg = self.alloc_temp()?;
                self.compile_expr_into(&args[0], pin_reg)?;
                self.emit_store(pin_reg, 0, hw::GPIO_DIR_IN);
                self.emit_addi(rd, 0, 1);
                self.free_temp(pin_reg);
                Ok(Some(()))
            }
            // ADC.read() → 读取当前通道
            "ADC.read" if args.is_empty() => {
                self.emit_load(rd, 0, hw::ADC_VALUE);
                Ok(Some(()))
            }
            // 非硬件 API 调用
            _ => Ok(None),
        }
    }

    // =========================================================================
    // 条件分支
    // =========================================================================

    fn compile_if(&mut self, condition: &Expr, then_body: &[SourceStatement], else_body: Option<&[SourceStatement]>) -> Result<(), String> {
        let cond = self.compile_condition(condition)?;
        self.emit_nop();
        self.free_condition_temps(cond.reg);

        if cond.zero_means_true {
            self.emit_bez(cond.reg, 8);
            self.free_temp(cond.reg);
            let jump_to_else_idx = self.instructions.len();
            self.emit_jump(0);

            let snap = self.enter_scope();
            for ss in then_body {
                self.set_line(ss.line);
                self.compile_statement(&ss.stmt)?;
            }
            self.leave_scope(snap);

            if let Some(else_stmts) = else_body {
                let jump_to_end_idx = self.instructions.len();
                self.emit_jump(0);

                let else_start_idx = self.instructions.len();
                let offset = self.calc_offset(jump_to_else_idx, else_start_idx);
                self.patch_offset(jump_to_else_idx, offset);

                let snap2 = self.enter_scope();
                for ss in else_stmts {
                    self.set_line(ss.line);
                    self.compile_statement(&ss.stmt)?;
                }
                self.leave_scope(snap2);

                let end_idx = self.instructions.len();
                let offset = self.calc_offset(jump_to_end_idx, end_idx);
                self.patch_offset(jump_to_end_idx, offset);
            } else {
                let end_idx = self.instructions.len();
                let offset = self.calc_offset(jump_to_else_idx, end_idx);
                self.patch_offset(jump_to_else_idx, offset);
            }
        } else {
            let bez_idx = self.instructions.len();
            self.emit_bez(cond.reg, 0);
            self.free_temp(cond.reg);

            let snap = self.enter_scope();
            for ss in then_body {
                self.set_line(ss.line);
                self.compile_statement(&ss.stmt)?;
            }
            self.leave_scope(snap);

            if let Some(else_stmts) = else_body {
                let jump_to_end_idx = self.instructions.len();
                self.emit_jump(0);

                let else_start_idx = self.instructions.len();
                let offset = self.calc_offset(bez_idx, else_start_idx);
                self.patch_offset(bez_idx, offset);

                let snap2 = self.enter_scope();
                for ss in else_stmts {
                    self.set_line(ss.line);
                    self.compile_statement(&ss.stmt)?;
                }
                self.leave_scope(snap2);

                let end_idx = self.instructions.len();
                let offset = self.calc_offset(jump_to_end_idx, end_idx);
                self.patch_offset(jump_to_end_idx, offset);
            } else {
                let end_idx = self.instructions.len();
                let offset = self.calc_offset(bez_idx, end_idx);
                self.patch_offset(bez_idx, offset);
            }
        }

        Ok(())
    }


    fn compile_for(
        &mut self,
        init: &Statement,
        condition: &Expr,
        update: &Statement,
        body: &[SourceStatement],
    ) -> Result<(), String> {
        // 0. 循环自身是一个作用域：初始化变量只在循环内可见
        let outer_snap = self.enter_scope();

        // 1. 执行初始化
        self.compile_statement(init)?;

        // 2. 循环开始: 条件检查
        let loop_start_idx = self.instructions.len();
        let cond = self.compile_condition(condition)?;
        self.emit_nop();
        // Free condition scratch registers (keep only cond.reg)
        self.free_condition_temps(cond.reg);

        // 3. 条件分支 (与 while 相同逻辑)
        self.loop_stack.push(LoopContext::new_loop());
        if cond.zero_means_true {
            self.emit_bez(cond.reg, 8);
            self.free_temp(cond.reg);
            let jump_exit_idx = self.instructions.len();
            self.emit_jump(0);

            // 4. 循环体
            let body_snap = self.enter_scope();
            for ss in body {
                self.set_line(ss.line);
                self.compile_statement(&ss.stmt)?;
            }
            self.leave_scope(body_snap);
            let ctx = self.loop_stack.pop().unwrap_or_default();

            // 5. 更新表达式（continue 跳转到此处）
            let update_start_idx = self.instructions.len();
            self.patch_jumps(&ctx.continue_patches, update_start_idx);
            self.compile_statement(update)?;

            // 6. 跳回循环开始
            let jump_back_idx = self.instructions.len();
            let back_offset = self.calc_offset(jump_back_idx, loop_start_idx);
            self.emit_jump(back_offset);

            // 7. 修补退出偏移（break 跳转到此处）
            let end_idx = self.instructions.len();
            let exit_offset = self.calc_offset(jump_exit_idx, end_idx);
            self.patch_offset(jump_exit_idx, exit_offset);
            self.patch_jumps(&ctx.break_patches, end_idx);
        } else {
            let bez_exit_idx = self.instructions.len();
            self.emit_bez(cond.reg, 0);
            self.free_temp(cond.reg);

            // 4. 循环体
            let body_snap = self.enter_scope();
            for ss in body {
                self.set_line(ss.line);
                self.compile_statement(&ss.stmt)?;
            }
            self.leave_scope(body_snap);
            let ctx = self.loop_stack.pop().unwrap_or_default();

            // 5. 更新表达式（continue 跳转到此处）
            let update_start_idx = self.instructions.len();
            self.patch_jumps(&ctx.continue_patches, update_start_idx);
            self.compile_statement(update)?;

            // 6. 跳回循环开始
            let jump_back_idx = self.instructions.len();
            let back_offset = self.calc_offset(jump_back_idx, loop_start_idx);
            self.emit_jump(back_offset);

            // 7. 修补退出偏移（break 跳转到此处）
            let end_idx = self.instructions.len();
            let exit_offset = self.calc_offset(bez_exit_idx, end_idx);
            self.patch_offset(bez_exit_idx, exit_offset);
            self.patch_jumps(&ctx.break_patches, end_idx);
        }

        self.leave_scope(outer_snap);
        Ok(())
    }

    fn compile_while(&mut self, condition: &Expr, body: &[SourceStatement]) -> Result<(), String> {
        let loop_start_idx = self.instructions.len();

        let cond = self.compile_condition(condition)?;
        self.emit_nop();
        self.free_condition_temps(cond.reg);

        self.loop_stack.push(LoopContext::new_loop());
        if cond.zero_means_true {
            self.emit_bez(cond.reg, 8);
            self.free_temp(cond.reg);
            let jump_exit_idx = self.instructions.len();
            self.emit_jump(0);

            let body_snap = self.enter_scope();
            for ss in body {
                self.set_line(ss.line);
                self.compile_statement(&ss.stmt)?;
            }
            self.leave_scope(body_snap);
            let ctx = self.loop_stack.pop().unwrap_or_default();

            // continue → 重新求值条件
            self.patch_jumps(&ctx.continue_patches, loop_start_idx);

            let jump_back_idx = self.instructions.len();
            let back_offset = self.calc_offset(jump_back_idx, loop_start_idx);
            self.emit_jump(back_offset);

            let end_idx = self.instructions.len();
            let exit_offset = self.calc_offset(jump_exit_idx, end_idx);
            self.patch_offset(jump_exit_idx, exit_offset);
            self.patch_jumps(&ctx.break_patches, end_idx);
        } else {
            let bez_exit_idx = self.instructions.len();
            self.emit_bez(cond.reg, 0);
            self.free_temp(cond.reg);

            let body_snap = self.enter_scope();
            for ss in body {
                self.set_line(ss.line);
                self.compile_statement(&ss.stmt)?;
            }
            self.leave_scope(body_snap);
            let ctx = self.loop_stack.pop().unwrap_or_default();

            // continue → 重新求值条件
            self.patch_jumps(&ctx.continue_patches, loop_start_idx);

            let jump_back_idx = self.instructions.len();
            let back_offset = self.calc_offset(jump_back_idx, loop_start_idx);
            self.emit_jump(back_offset);

            let end_idx = self.instructions.len();
            let exit_offset = self.calc_offset(bez_exit_idx, end_idx);
            self.patch_offset(bez_exit_idx, exit_offset);
            self.patch_jumps(&ctx.break_patches, end_idx);
        }

        Ok(())
    }

    /// 批量回填跳转补丁（break/continue 链）
    fn patch_jumps(&mut self, idxs: &[usize], target: usize) {
        for &idx in idxs {
            let offset = self.calc_offset(idx, target);
            self.patch_offset(idx, offset);
        }
    }

    fn compile_condition(&mut self, expr: &Expr) -> Result<CondResult, String> {
        match expr {
            Expr::BinOp { op: op @ (BinOp::Eq | BinOp::NotEq | BinOp::StrictEq | BinOp::StrictNotEq | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge), left, right } => {
                let (actual_left, actual_right, actual_op) = match op {
                    BinOp::Gt => (right.as_ref(), left.as_ref(), BinOp::Lt),
                    BinOp::Ge => (right.as_ref(), left.as_ref(), BinOp::Le),
                    // StrictEq/StrictNotEq → 整数类型与Eq/NotEq相同
                    BinOp::StrictEq => (left.as_ref(), right.as_ref(), BinOp::Eq),
                    BinOp::StrictNotEq => (left.as_ref(), right.as_ref(), BinOp::NotEq),
                    _ => (left.as_ref(), right.as_ref(), *op),
                };

                let left_reg = self.alloc_temp()?;
                self.compile_expr_into(actual_left, left_reg)?;
                let right_reg = self.alloc_temp()?;
                self.compile_expr_into(actual_right, right_reg)?;

                self.emit_nop();

                self.push_instr(Instruction::RR {
                    opcode: 0x1, func: 0x0,
                    rs1: left_reg, rs2: right_reg, rd: left_reg,
                });

                match actual_op {
                    BinOp::Eq => Ok(CondResult { reg: left_reg, zero_means_true: true }),
                    BinOp::NotEq => Ok(CondResult { reg: left_reg, zero_means_true: false }),
                    BinOp::Lt => {
                        let shift_reg = self.alloc_temp()?;
                        self.emit_addi(shift_reg, REG_ZERO, 63);
                        self.emit_nop();
                        self.push_instr(Instruction::RR {
                            opcode: 0x6, func: 0x0,
                            rs1: left_reg, rs2: shift_reg, rd: left_reg,
                        });
                        Ok(CondResult { reg: left_reg, zero_means_true: false })
                    }
                    BinOp::Le => {
                        // a <= b  ⟺  (a - b - 1) >> 63 == 0
                        // left_reg 已有 a - b (外层 SUB)
                        // ADDI -1: left_reg = (a - b) - 1
                        self.emit_addi(left_reg, left_reg, -1_i16);
                        // NOP: 等待 ADDI 结果写回
                        self.emit_nop();
                        let shift_reg = self.alloc_temp()?;
                        self.emit_addi(shift_reg, REG_ZERO, 63);
                        self.emit_nop();
                        self.push_instr(Instruction::RR {
                            opcode: 0x6, func: 0x0,  // SHR
                            rs1: left_reg, rs2: shift_reg, rd: left_reg,
                        });
                        // SHR 结果: 0 表示 a<=b (true), 1 表示 a>b (false)
                        Ok(CondResult { reg: left_reg, zero_means_true: false })
                    }
                    _ => unreachable!(),
                }
            }
            _ => {
                let reg = self.alloc_temp()?;
                self.compile_expr_into(expr, reg)?;
                Ok(CondResult { reg, zero_means_true: false })
            }
        }
    }

    // =========================================================================
    // 表达式编译
    // =========================================================================

    /// 编译 Math.max/min/abs → COUNPRE64 创新指令 (单周期!)
    fn compile_math_intrinsic(&mut self, op: &MathOp, args: &[Expr], rd: u8) -> Result<(), String> {
        match op {
            MathOp::Abs => {
                // ABS 只需一个参数: ABS rs1 → rd (rs2 ignored)
                if args.is_empty() {
                    return Err("Math.abs 需要 1 个参数".to_string());
                }
                self.compile_expr_into(&args[0], rd)?;
                self.emit_nop();
                self.push_instr(Instruction::RR {
                    opcode: 0x0, func: 0xD,
                    rs1: rd, rs2: 0, rd,  // ABS rd → rd
                });
            }
            MathOp::Max | MathOp::Min => {
                // MAX/MIN 需要两个参数
                if args.len() < 2 {
                    return Err(format!("Math.{:?} 需要 2 个参数", op));
                }
                let func = match op {
                    MathOp::Max => 0xB,
                    MathOp::Min => 0xC,
                    _ => unreachable!(),
                };
                // 计算第一个参数到 rd
                self.compile_expr_into(&args[0], rd)?;
                // 计算第二个参数到临时寄存器
                let tmp = self.alloc_temp()?;
                self.compile_expr_into(&args[1], tmp)?;
                self.emit_nop();
                // MAX/MIN rd, tmp → rd
                self.push_instr(Instruction::RR {
                    opcode: 0x0, func,
                    rs1: rd, rs2: tmp, rd,
                });
                self.free_temp(tmp);
            }
            // Sqrt/Pow 需要循环，COUNPRE64 本机引擎不提供（已封存线）；
            // 产品路径请使用 --target esp32 等 C 目标（生成 hardy_isqrt/hardy_ipow helper）
            MathOp::Sqrt | MathOp::Pow => {
                return Err(format!(
                    "Math.{:?} 需要循环实现，本机 COUNPRE64 引擎不支持，请使用 --target esp32 等 C 目标", op));
            }
        }
        Ok(())
    }

    /// 将任意表达式求值为规范的 0/1 布尔值，写入 rd。
    /// 比较运算符使用 compile_condition 已经正确的 SUB/移位逻辑，
    /// && / || 使用真正的短路跳转（不依赖按位与/或，也不会因两个
    /// 操作数都映射到同一 opcode 而静默丢失 ||）。
    fn compile_expr_to_bool(&mut self, expr: &Expr, rd: u8) -> Result<(), String> {
        match expr {
            Expr::BinOp { op: BinOp::And, left, right } => {
                // if (!a) { rd=0 } else { rd = bool(b) }
                self.compile_expr_to_bool(left, rd)?;
                let bez_idx = self.instructions.len();
                self.emit_bez(rd, 0); // rd==0：短路，跳到 end（rd 已经就是 0）
                self.compile_expr_to_bool(right, rd)?;
                let end_idx = self.instructions.len();
                let off = self.calc_offset(bez_idx, end_idx);
                self.patch_offset(bez_idx, off);
                Ok(())
            }
            Expr::BinOp { op: BinOp::Or, left, right } => {
                // if (a) { rd=1 } else { rd = bool(b) }
                self.compile_expr_to_bool(left, rd)?;
                let bez_idx = self.instructions.len();
                self.emit_bez(rd, 0); // rd==0：需要求值右边，跳到 eval_right
                let jump_end_idx = self.instructions.len();
                self.emit_jump(0); // rd!=0：短路，保持 rd=1，跳到 end
                let eval_right_idx = self.instructions.len();
                let off = self.calc_offset(bez_idx, eval_right_idx);
                self.patch_offset(bez_idx, off);
                self.compile_expr_to_bool(right, rd)?;
                let end_idx = self.instructions.len();
                let off = self.calc_offset(jump_end_idx, end_idx);
                self.patch_offset(jump_end_idx, off);
                Ok(())
            }
            _ => {
                // 非逻辑连接词：统一走 compile_condition（处理比较）或普通求值，
                // 然后将结果规范化为 0/1。
                let cond = self.compile_condition(expr)?;
                self.normalize_cond_to_bool(cond, rd)
            }
        }
    }

    /// 将 compile_condition 返回的 (reg, zero_means_true) 规范化为 rd 里的 0/1。
    fn normalize_cond_to_bool(&mut self, cond: CondResult, rd: u8) -> Result<(), String> {
        self.emit_nop();
        // 注意：不能释放 rd —— 在嵌套表达式（如 (a>0)-(b<0)）中，rd 由外层
        // BinOp 臂持有并已装入左侧结果；若在此释放，alloc_temp 会把 rd 重新
        // 分配给右侧的 scratch 寄存器，踩掉左侧活值（表现为 62/63 类错误值）。
        let to_free: Vec<u8> = self.active_temps.iter()
            .filter(|&&r| r != cond.reg && r != rd)
            .copied()
            .collect();
        for r in to_free {
            self.free_temp(r);
        }

        if cond.zero_means_true {
            let bez_idx = self.instructions.len();
            self.emit_bez(cond.reg, 0); // reg==0 → true 分支
            self.addi_imm(rd, 0); // false: rd=0
            let jump_end_idx = self.instructions.len();
            self.emit_jump(0);
            let true_start = self.instructions.len();
            let off = self.calc_offset(bez_idx, true_start);
            self.patch_offset(bez_idx, off);
            self.addi_imm(rd, 1); // true: rd=1
            let end_idx = self.instructions.len();
            let off = self.calc_offset(jump_end_idx, end_idx);
            self.patch_offset(jump_end_idx, off);
        } else {
            let bez_idx = self.instructions.len();
            self.emit_bez(cond.reg, 0); // reg==0 → false 分支
            self.addi_imm(rd, 1); // true: rd=1
            let jump_end_idx = self.instructions.len();
            self.emit_jump(0);
            let false_start = self.instructions.len();
            let off = self.calc_offset(bez_idx, false_start);
            self.patch_offset(bez_idx, off);
            self.addi_imm(rd, 0); // false: rd=0
            let end_idx = self.instructions.len();
            let off = self.calc_offset(jump_end_idx, end_idx);
            self.patch_offset(jump_end_idx, off);
        }

        self.free_temp(cond.reg);
        Ok(())
    }

    fn addi_imm(&mut self, rd: u8, imm: i16) {
        self.emit_addi(rd, REG_ZERO, imm);
    }

    fn compile_expr_into(&mut self, expr: &Expr, rd: u8) -> Result<(), String> {
        match expr {
            Expr::Number(n) => {
                self.load_const(rd, *n)
            }
            Expr::StringLit(_s) => {
                // 字符串作为表达式 → 返回首字符 ASCII 值
                // (完整字符串处理需要堆，这里简化)
                let first_byte = _s.bytes().next().unwrap_or(0);
                self.emit_addi(rd, REG_ZERO, first_byte as i16);
                Ok(())
            }
            Expr::Ident(name) => {
                self.get_var_to_reg(name, rd)?;
                Ok(())
            }
            Expr::Call { name, args } => {
                self.compile_call(name, args, rd)
            }
            Expr::Array(elements) => {
                self.compile_array_literal(elements, rd)
            }
            Expr::Index { array, index } => {
                self.compile_array_index(array, index, rd)
            }
            Expr::MathIntrinsic { op, args } => {
                self.compile_math_intrinsic(op, args, rd)?;
                Ok(())
            }
            Expr::ObjectLit { keys, values } => {
                self.compile_object_literal(keys, values, rd)?;
                Ok(())
            }
            Expr::PropAccess { object, property } => {
                self.compile_prop_access(object, property, rd)?;
                Ok(())
            }
            Expr::BinOp { op, left, right } => {
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod => {
                        self.compile_expr_into(left, rd)?;
                        let tmp = self.alloc_temp()?;
                        self.compile_expr_into(right, tmp)?;
                        self.emit_nop();
                        let (opcode, func) = match op {
                            BinOp::Add => (0x0, 0x0),
                            BinOp::Sub => (0x1, 0x0),
                            BinOp::Mul => (0x7, 0x8),
                            BinOp::Div => (0x7, 0xC),
                            BinOp::Mod => (0x7, 0xE),
                            _ => unreachable!(),
                        };
                        self.push_instr(Instruction::RR {
                            opcode, func,
                            rs1: rd, rs2: tmp, rd,
                        });
                        self.free_temp(tmp);
                        Ok(())
                    }
                    // 比较/逻辑运算符在“值上下文”中必须产生规范的 0/1 布尔结果，
                    // 不能像旧版那样静默退化为原始 SUB/AND 数值。
                    _ => self.compile_expr_to_bool(expr, rd),
                }
            }
            // Phase A: 布尔值 → 0/1
            Expr::Bool(b) => {
                self.emit_addi(rd, REG_ZERO, if *b { 1 } else { 0 });
                Ok(())
            }
            // Phase A: null → 0
            Expr::Null => {
                self.emit_addi(rd, REG_ZERO, 0);
                Ok(())
            }
            // Phase A.7: 三元运算符 cond ? then : else
            Expr::Ternary { condition, then_expr, else_expr } => {
                let cond = self.compile_condition(condition)?;
                self.emit_nop();
                self.free_condition_temps(cond.reg);

                if cond.zero_means_true {
                    // BEZ → then (zero means true)
                    self.emit_bez(cond.reg, 8);
                    let jump_else_idx = self.instructions.len();
                    self.emit_jump(0); // → else
                    // then branch
                    self.compile_expr_into(then_expr, rd)?;
                    let jump_end_idx = self.instructions.len();
                    self.emit_jump(0); // → end
                    // else branch
                    let else_start = self.instructions.len();
                    let offset = self.calc_offset(jump_else_idx, else_start);
                    self.patch_offset(jump_else_idx, offset);
                    self.compile_expr_into(else_expr, rd)?;
                    // end
                    let end_idx = self.instructions.len();
                    let offset = self.calc_offset(jump_end_idx, end_idx);
                    self.patch_offset(jump_end_idx, offset);
                } else {
                    let bez_else_idx = self.instructions.len();
                    self.emit_bez(cond.reg, 0); // skip then if zero (false)
                    // then branch
                    self.compile_expr_into(then_expr, rd)?;
                    let jump_end_idx = self.instructions.len();
                    self.emit_jump(0);
                    // else branch
                    let else_start = self.instructions.len();
                    let offset = self.calc_offset(bez_else_idx, else_start);
                    self.patch_offset(bez_else_idx, offset);
                    self.compile_expr_into(else_expr, rd)?;
                    // end
                    let end_idx = self.instructions.len();
                    let offset = self.calc_offset(jump_end_idx, end_idx);
                    self.patch_offset(jump_end_idx, offset);
                }
                Ok(())
            }
            // Phase A.1: 箭头函数 → 编译为内联函数定义
            Expr::ArrowFunc { params, body, .. } => {
                // 生成唯一函数名
                let func_name = format!("__arrow_{}", self.functions.len());
                let func_body = match body {
                    ArrowBody::Block(stmts) => stmts.clone(),
                    ArrowBody::Expr(expr) => vec![SourceStatement {
                        stmt: Statement::Return(*expr.clone()),
                        line: 0,
                    }],
                };
                let func = Function {
                    name: func_name.clone(),
                    params: params.clone(),
                    body: func_body,
                    is_async: false,
                    is_realtime: false,
                    wcet_limit_us: None,
                    interrupt_timer: None,
                    interrupt_period_us: None,
                    priority: None,
                };
                // 记录函数信息
                let addr = self.current_addr();
                self.functions.push(FunctionInfo {
                    name: func_name.clone(),
                    param_count: params.len(),
                    addr,
                });
                // 编译函数体（需要快照/恢复当前帧的全部分配状态，因为
                // compile_function 会调用 reset_frame 重置 slots/free_pool 等）
                let saved_in_function = self.in_function;
                let saved_slots = self.slots.clone();
                let saved_next_slot_off = self.next_slot_off;
                let saved_frame_size = self.frame_size;
                let saved_free_pool = self.free_pool.clone();
                let saved_active_temps = self.active_temps.clone();
                let saved_scratch_spill_off = self.scratch_spill_off;
                let saved_scratch_spills = self.scratch_spills.clone();
                let saved_return_patches = self.return_patches.clone();
                self.compile_function(&func, 0)?;
                self.in_function = saved_in_function;
                self.slots = saved_slots;
                self.next_slot_off = saved_next_slot_off;
                self.frame_size = saved_frame_size;
                self.free_pool = saved_free_pool;
                self.active_temps = saved_active_temps;
                self.scratch_spill_off = saved_scratch_spill_off;
                self.scratch_spills = saved_scratch_spills;
                self.return_patches = saved_return_patches;
                // 将函数地址存入变量
                self.emit_addi(rd, REG_ZERO, addr as i16);
                Ok(())
            }
            // Phase A.2: 模板字符串 → 输出第一个文本部分的ASCII值（简化）
            Expr::TemplateStr { parts } => {
                // 简化：取第一个文本部分的第一个字符
                let first_byte = parts.iter().find_map(|p| match p {
                    TemplateExprPart::Text(t) => t.bytes().next(),
                    _ => None,
                }).unwrap_or(0);
                self.emit_addi(rd, REG_ZERO, first_byte as i16);
                Ok(())
            }
            // Phase A.8: 字符串方法
            Expr::StringMethod { object, method, args: _ } => {
                match method.as_str() {
                    "length" => {
                        // 编译期无法知道字符串长度，返回 0
                        self.compile_expr_into(object, rd)?;
                        // NOP: 占位
                        self.emit_nop();
                        Ok(())
                    }
                    "charAt" => {
                        // str.charAt(i) → 简化为返回字符串首字符
                        self.compile_expr_into(object, rd)?;
                        Ok(())
                    }
                    "slice" | "substring" | "indexOf" => {
                        // 简化：返回对象值
                        self.compile_expr_into(object, rd)?;
                        Ok(())
                    }
                    _ => {
                        self.compile_expr_into(object, rd)?;
                        Ok(())
                    }
                }
            }
            // Phase v1.5: await expr → 状态机变换（当前简化为编译内部表达式）
            Expr::Await { expr } => {
                // TODO: 完整状态机变换在 Task 3 实现
                // 当前简化：直接编译内部表达式
                self.compile_expr_into(expr, rd)
            }
            // Phase v1.5: Promise.all → 并行等待（当前简化为编译第一个 promise）
            Expr::PromiseAll { promises } => {
                // TODO: 完整并行等待在 Task 3 实现
                if let Some(first) = promises.first() {
                    self.compile_expr_into(first, rd)
                } else {
                    self.emit_addi(rd, REG_ZERO, 0);
                    Ok(())
                }
            }
            // Phase v1.5: Promise.race → 竞争等待（当前简化为编译第一个 promise）
            Expr::PromiseRace { promises } => {
                // TODO: 完整竞争等待在 Task 3 实现
                if let Some(first) = promises.first() {
                    self.compile_expr_into(first, rd)
                } else {
                    self.emit_addi(rd, REG_ZERO, 0);
                    Ok(())
                }
            }
        }
    }

    /// Free all active temps except the one holding condition result
    fn free_condition_temps(&mut self, keep: u8) {
        let to_free: Vec<u8> = self.active_temps.iter()
            .filter(|&&r| r != keep)
            .copied()
            .collect();
        for r in to_free {
            self.free_temp(r);
        }
    }

    // =========================================================================
    // Phase A.3: switch 编译
    // =========================================================================

    fn compile_switch(&mut self, discriminant: &Expr, cases: &[SwitchCase]) -> Result<(), String> {
        // switch 上下文：break 在内层时 NOP 即等价（if-else 链无 fallthrough），
        // 但需压栈以免循环内的 break/continue 误链接到外层循环
        self.loop_stack.push(LoopContext::new_switch());
        // 计算判别表达式的值到临时寄存器
        let disc_reg = self.alloc_temp()?;
        self.compile_expr_into(discriminant, disc_reg)?;

        let mut end_jumps: Vec<usize> = Vec::new();

        for case in cases {
            if let Some(test) = &case.test {
                // case val: 比较 disc_reg == val
                let val_reg = self.alloc_temp()?;
                self.compile_expr_into(test, val_reg)?;
                self.emit_nop();
                // SUB disc_reg - val_reg → scratch
                let cmp_reg = self.alloc_temp()?;
                self.push_instr(Instruction::RR {
                    opcode: 0x1, func: 0x0,
                    rs1: disc_reg, rs2: val_reg, rd: cmp_reg,
                });
                // BEZ: if cmp_reg == 0, fall through to body
                self.emit_nop();
                self.emit_bez(cmp_reg, 8);
                let skip_body_idx = self.instructions.len();
                self.emit_jump(0); // skip body if not equal

                self.free_temp(cmp_reg);
                self.free_temp(val_reg);

                // case body
                for ss in &case.body {
                    self.set_line(ss.line);
                    self.compile_statement(&ss.stmt)?;
                }

                // jump to end of switch
                let jump_end_idx = self.instructions.len();
                self.emit_jump(0);
                end_jumps.push(jump_end_idx);

                // patch skip_body
                let body_start = self.instructions.len();
                let offset = self.calc_offset(skip_body_idx, body_start);
                self.patch_offset(skip_body_idx, offset);
            } else {
                // default: 直接执行
                for ss in &case.body {
                    self.set_line(ss.line);
                    self.compile_statement(&ss.stmt)?;
                }
            }
        }

        // patch all end jumps
        let end_idx = self.instructions.len();
        for j in end_jumps {
            let offset = self.calc_offset(j, end_idx);
            self.patch_offset(j, offset);
        }

        self.loop_stack.pop();
        self.free_temp(disc_reg);
        Ok(())
    }

    // =========================================================================
    // Phase 10: Object literal + property access
    // =========================================================================

    fn compile_object_literal(&mut self, _keys: &[String], values: &[Expr], rd: u8) -> Result<(), String> {
        let base_addr = self.alloc_heap(values.len() as i16);
        self.emit_addi(rd, REG_ZERO, base_addr);
        for (i, val) in values.iter().enumerate() {
            let tmp = self.alloc_temp()?;
            self.compile_expr_into(val, tmp)?;
            let offset = (i as i16) * 8;
            self.emit_nop();
            self.emit_store(tmp, rd, offset);
            self.free_temp(tmp);
        }
        Ok(())
    }

    fn compile_prop_access(&mut self, object: &Expr, property: &str, rd: u8) -> Result<(), String> {
        self.compile_expr_into(object, rd)?;
        let offset = self.get_prop_offset(object, property)?;
        self.emit_nop();
        self.emit_load(rd, rd, offset);
        Ok(())
    }

    fn compile_prop_assign(&mut self, object: &str, property: &str, value: &Expr) -> Result<(), String> {
        if self.slot_off(object).is_none() {
            return Err(format!("undefined object '{}'", object));
        }
        let obj_expr = Expr::Ident(object.to_string());
        let offset = self.get_prop_offset(&obj_expr, property)?;
        // 基址从栈槽加载到专门的 scratch（不再需要临时存到 SP-8 躲避覆盖）
        let base_reg = self.alloc_temp()?;
        self.get_var_to_reg(object, base_reg)?;
        let val_reg = self.alloc_temp()?;
        self.compile_expr_into(value, val_reg)?;
        self.emit_nop();
        self.emit_store(val_reg, base_reg, offset);
        self.free_temp(val_reg);
        self.free_temp(base_reg);
        Ok(())
    }

    fn get_prop_offset(&self, object: &Expr, property: &str) -> Result<i16, String> {
        if let Expr::Ident(name) = object {
            if let Some((_, props)) = self.object_props.iter().find(|(n, _)| n == name) {
                if let Some(idx) = props.iter().position(|p| p == property) {
                    return Ok((idx as i16) * 8);
                }
                return Err(format!("object '{}' has no property '{}'", name, property));
            }
        }
        Err(format!("cannot resolve property offset for '{}'", property))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    #[test]
    fn test_compile_let() {
        let tokens = tokenize("let a = 10;").unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        assert!(!instrs.is_empty());
    }

    #[test]
    fn test_compile_add() {
        let tokens = tokenize("let a = 10; let b = 20; let c = a + b;").unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        assert!(instrs.len() >= 3);
    }

    #[test]
    fn test_compile_function() {
        let source = "function add(a, b) { return a + b; }";
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        let has_ret = instrs.iter().any(|i| matches!(i, Instruction::RR { opcode: 0xF, func: 0x1, .. }));
        assert!(has_ret, "应包含 RET 指令");
    }

    #[test]
    fn test_compile_call() {
        let source = "function add(a, b) { return a + b; } let x = add(3, 4);";
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        let has_jump = instrs.iter().any(|i| matches!(i, Instruction::RI { opcode: 0xE, .. }));
        assert!(has_jump, "应包含 JUMP 指令");
    }

    #[test]
    fn test_compile_console_log_number() {
        let source = "console.log(42);";
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        // 应包含 STORE 到 UART 地址
        let has_store = instrs.iter().any(|i| matches!(i, Instruction::RI { opcode: 0xC, .. }));
        assert!(has_store, "console.log 应生成 STORE 指令");
    }

    #[test]
    fn test_compile_console_log_string() {
        let source = r#"console.log("Hi");"#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        // "Hi\n" = 3 个字符, 每个字符 ADDI+NOP+STORE = 3 条
        // 总共 3*3 + halt = 10 条
        let store_count = instrs.iter().filter(|i| matches!(i, Instruction::RI { opcode: 0xC, .. })).count();
        assert_eq!(store_count, 3, "Hi + \\n = 3 个 STORE");
    }

    #[test]
    fn test_compile_array() {
        let source = "let arr = [10, 20, 30];";
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        // 应包含多个 STORE（初始化数组元素）
        let store_count = instrs.iter().filter(|i| matches!(i, Instruction::RI { opcode: 0xC, .. })).count();
        assert!(store_count >= 3, "数组初始化应有 3 个 STORE");
    }

    #[test]
    fn test_compile_if_eq() {
        let source = "let a = 10; let b = 10; let c = 0; if (a == b) { c = 1; }";
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        let has_branch = instrs.iter().any(|i| matches!(i, Instruction::RI { opcode: 0xD, .. }));
        assert!(has_branch, "应包含 BEZ 指令");
    }

    #[test]
    fn test_compile_while() {
        let source = "let i = 0; while (i != 10) { i = i + 1; }";
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        let has_backward_jump = instrs.iter().any(|i| {
            matches!(i, Instruction::RI { opcode: 0xE, imm, .. } if *imm < 0)
        });
        assert!(has_backward_jump, "while 应包含向回跳转");
    }

    #[test]
    fn test_compile_modulo() {
        let source = "let a = 10; let b = 3; let c = a % b;";
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        // 应包含 REM 指令 (opcode=7, func=E)
        let has_rem = instrs.iter().any(|i| matches!(i, Instruction::RR { opcode: 0x7, func: 0xE, .. }));
        assert!(has_rem, "% 应生成 REM 指令");
    }

    // Phase C.3: 硬件 API 静态链接测试
    #[test]
    fn test_hw_gpio_high() {
        // GPIO.high(13) 应生成 STORE 指令到 GPIO_OUT 地址 (0x201)
        let source = r#"let x = GPIO.high(13);"#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        // 应包含 STORE 指令 (opcode=0xC)
        let has_store = instrs.iter().any(|i| matches!(i, Instruction::RI { opcode: 0xC, .. }));
        assert!(has_store, "GPIO.high 应生成 STORE 指令");
    }

    #[test]
    fn test_hw_uart_init() {
        // UART.init(9600) 应生成 STORE 指令到 UART_BAUD 地址 (0x103)
        let source = r#"let x = UART.init(9600);"#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        let has_store = instrs.iter().any(|i| matches!(i, Instruction::RI { opcode: 0xC, .. }));
        assert!(has_store, "UART.init 应生成 STORE 指令");
    }

    #[test]
    fn test_hw_adc_read() {
        // ADC.read(0) 应生成 LOAD 指令从 ADC_VALUE 地址 (0x502)
        let source = r#"let x = ADC.read(0);"#;
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let instrs = compile(&ast).unwrap();
        let has_load = instrs.iter().any(|i| matches!(i, Instruction::RI { opcode: 0xB, .. }));
        assert!(has_load, "ADC.read 应生成 LOAD 指令");
    }
}
