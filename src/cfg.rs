//! Phase 4.1: 控制流图构建
//!
//! 从 AST 构建控制流图 (CFG)，识别循环、分支、函数调用。
//! 用于 WCET 分析的基础数据结构。

use crate::parser::{Expr, Function, Statement, SourceStatement, BinOp};

/// 基本块
#[derive(Debug, Clone)]
pub struct BasicBlock {
    /// 块 ID
    pub id: usize,
    /// 块中的语句数
    pub stmt_count: usize,
    /// 循环深度
    pub loop_depth: u32,
    /// 估算周期数
    pub estimated_cycles: u64,
    /// 块类型
    pub kind: BlockKind,
}

/// 基本块类型
#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    /// 普通顺序执行
    Normal,
    /// 循环头
    LoopHeader,
    /// 循环体
    LoopBody,
    /// 分支 (if/switch)
    Branch,
    /// 函数调用
    Call,
    /// 返回
    Return,
    /// 入口
    Entry,
    /// 出口
    Exit,
}

/// 控制流图
#[derive(Debug, Clone)]
pub struct ControlFlowGraph {
    /// 所有基本块
    pub blocks: Vec<BasicBlock>,
    /// 边 (from, to)
    pub edges: Vec<(usize, usize)>,
    /// 入口块 ID
    pub entry: usize,
    /// 出口块 ID
    pub exit: usize,
    /// 最大循环深度
    pub max_loop_depth: u32,
    /// 函数调用列表
    pub calls: Vec<String>,
}

impl ControlFlowGraph {
    /// 获取块的后继
    pub fn successors(&self, block_id: usize) -> Vec<usize> {
        self.edges.iter()
            .filter(|(from, _)| *from == block_id)
            .map(|(_, to)| *to)
            .collect()
    }

    /// 获取块的前驱
    pub fn predecessors(&self, block_id: usize) -> Vec<usize> {
        self.edges.iter()
            .filter(|(_, to)| *to == block_id)
            .map(|(from, _)| *from)
            .collect()
    }

    /// 总块数
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// 总边数
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// 从函数 AST 构建 CFG
pub fn build_cfg(func: &Function) -> ControlFlowGraph {
    let mut builder = CfgBuilder::new();
    builder.build_from_function(func);
    builder.finish()
}

/// CFG 构建器
struct CfgBuilder {
    blocks: Vec<BasicBlock>,
    edges: Vec<(usize, usize)>,
    next_id: usize,
    current_block: usize,
    loop_depth: u32,
    max_loop_depth: u32,
    calls: Vec<String>,
}

impl CfgBuilder {
    fn new() -> Self {
        let mut builder = Self {
            blocks: Vec::new(),
            edges: Vec::new(),
            next_id: 0,
            current_block: 0,
            loop_depth: 0,
            max_loop_depth: 0,
            calls: Vec::new(),
        };
        // 创建入口块
        builder.new_block(BlockKind::Entry);
        builder
    }

    fn new_block(&mut self, kind: BlockKind) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.blocks.push(BasicBlock {
            id,
            stmt_count: 0,
            loop_depth: self.loop_depth,
            estimated_cycles: 0,
            kind,
        });
        id
    }

    fn add_edge(&mut self, from: usize, to: usize) {
        if !self.edges.contains(&(from, to)) {
            self.edges.push((from, to));
        }
    }

    fn build_from_function(&mut self, func: &Function) {
        let entry = self.current_block;
        self.build_statements(&func.body);
        // 创建出口块
        let exit = self.new_block(BlockKind::Exit);
        self.add_edge(self.current_block, exit);
        // 确保入口连接
        if entry == 0 && self.blocks.len() > 1 {
            // 已有入口
        }
    }

    fn build_statements(&mut self, stmts: &[SourceStatement]) {
        for ss in stmts {
            self.build_statement(&ss.stmt);
        }
    }

    fn build_statement(&mut self, stmt: &Statement) {
        // 增加当前块的语句计数
        if let Some(block) = self.blocks.last_mut() {
            block.stmt_count += 1;
        }

        match stmt {
            Statement::If { condition, then_body, else_body, .. } => {
                self.scan_expr_for_calls(condition);
                // 分支块
                let branch_block = self.current_block;
                if let Some(block) = self.blocks.get_mut(branch_block) {
                    block.kind = BlockKind::Branch;
                }

                // then 分支
                let then_block = self.new_block(BlockKind::Normal);
                self.add_edge(branch_block, then_block);
                self.current_block = then_block;
                self.build_statements(then_body);
                let then_end = self.current_block;

                // else 分支
                let else_end;
                if let Some(eb) = else_body {
                    let else_block = self.new_block(BlockKind::Normal);
                    self.add_edge(branch_block, else_block);
                    self.current_block = else_block;
                    self.build_statements(eb);
                    else_end = self.current_block;
                } else {
                    else_end = branch_block;
                }

                // 合并点
                let merge = self.new_block(BlockKind::Normal);
                self.add_edge(then_end, merge);
                self.add_edge(else_end, merge);
                self.current_block = merge;
            }
            Statement::While { condition, body } => {
                self.scan_expr_for_calls(condition);
                // 循环头
                let header = self.new_block(BlockKind::LoopHeader);
                self.add_edge(self.current_block, header);
                self.loop_depth += 1;
                self.max_loop_depth = self.max_loop_depth.max(self.loop_depth);

                // 循环体
                let body_block = self.new_block(BlockKind::LoopBody);
                self.add_edge(header, body_block);
                self.current_block = body_block;
                self.build_statements(body);
                // 回边
                self.add_edge(self.current_block, header);

                self.loop_depth -= 1;
                // 循环出口
                let exit_block = self.new_block(BlockKind::Normal);
                self.add_edge(header, exit_block);
                self.current_block = exit_block;
            }
            Statement::For { init, condition, update, body } => {
                self.scan_expr_for_calls(condition);
                // init 在当前块
                self.build_statement(init);

                // 循环头 (条件检查)
                let header = self.new_block(BlockKind::LoopHeader);
                self.add_edge(self.current_block, header);
                self.loop_depth += 1;
                self.max_loop_depth = self.max_loop_depth.max(self.loop_depth);

                // 循环体
                let body_block = self.new_block(BlockKind::LoopBody);
                self.add_edge(header, body_block);
                self.current_block = body_block;
                self.build_statements(body);
                // update
                self.build_statement(update);
                // 回边
                self.add_edge(self.current_block, header);

                self.loop_depth -= 1;
                // 循环出口
                let exit_block = self.new_block(BlockKind::Normal);
                self.add_edge(header, exit_block);
                self.current_block = exit_block;
            }
            Statement::Switch { discriminant, cases } => {
                self.scan_expr_for_calls(discriminant);
                let branch_block = self.current_block;
                if let Some(block) = self.blocks.get_mut(branch_block) {
                    block.kind = BlockKind::Branch;
                }

                let merge = self.new_block(BlockKind::Normal);
                for case in cases {
                    let case_block = self.new_block(BlockKind::Normal);
                    self.add_edge(branch_block, case_block);
                    self.current_block = case_block;
                    self.build_statements(&case.body);
                    self.add_edge(self.current_block, merge);
                }
                self.current_block = merge;
            }
            Statement::Return(expr) => {
                self.scan_expr_for_calls(expr);
                if let Some(block) = self.blocks.last_mut() {
                    block.kind = BlockKind::Return;
                }
            }
            Statement::Let { value, .. } | Statement::Const { value, .. } | Statement::Assign { value, .. } => {
                self.scan_expr_for_calls(value);
            }
            Statement::Expr(expr) => {
                self.scan_expr_for_calls(expr);
            }
            Statement::ConsoleLog(args) => {
                for arg in args {
                    self.scan_expr_for_calls(arg);
                }
            }
            _ => {}
        }
    }

    fn scan_expr_for_calls(&mut self, expr: &Expr) {
        match expr {
            Expr::Call { name, args } => {
                if !self.calls.contains(name) {
                    self.calls.push(name.clone());
                }
                if let Some(block) = self.blocks.last_mut() {
                    if block.kind == BlockKind::Normal {
                        block.kind = BlockKind::Call;
                    }
                }
                for arg in args {
                    self.scan_expr_for_calls(arg);
                }
            }
            Expr::BinOp { left, right, .. } => {
                self.scan_expr_for_calls(left);
                self.scan_expr_for_calls(right);
            }
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.scan_expr_for_calls(condition);
                self.scan_expr_for_calls(then_expr);
                self.scan_expr_for_calls(else_expr);
            }
            Expr::Index { array, index } => {
                self.scan_expr_for_calls(array);
                self.scan_expr_for_calls(index);
            }
            Expr::PropAccess { object, .. } => {
                self.scan_expr_for_calls(object);
            }
            Expr::Await { expr } => {
                self.scan_expr_for_calls(expr);
            }
            Expr::Array(elems) => {
                for elem in elems {
                    self.scan_expr_for_calls(elem);
                }
            }
            Expr::ObjectLit { values, .. } => {
                for val in values {
                    self.scan_expr_for_calls(val);
                }
            }
            _ => {}
        }
    }

    fn finish(self) -> ControlFlowGraph {
        let exit = self.blocks.iter()
            .find(|b| b.kind == BlockKind::Exit)
            .map(|b| b.id)
            .unwrap_or(self.blocks.len().saturating_sub(1));

        ControlFlowGraph {
            blocks: self.blocks,
            edges: self.edges,
            entry: 0,
            exit,
            max_loop_depth: self.max_loop_depth,
            calls: self.calls,
        }
    }
}

/// 估算循环上界 (从条件表达式)
pub fn estimate_loop_bound(condition: &Expr) -> Option<u64> {
    match condition {
        Expr::Bool(true) => None, // 无限循环 (需要内部 break)
        Expr::Number(n) if *n != 0 => None,
        Expr::BinOp { op, right, .. } => {
            match op {
                BinOp::Lt | BinOp::Le => {
                    if let Expr::Number(n) = right.as_ref() {
                        Some(*n as u64)
                    } else {
                        None
                    }
                }
                BinOp::Gt | BinOp::Ge => {
                    // x > N 形式，假设从某个值递减到 N
                    None
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn build_cfg_from_source(source: &str) -> ControlFlowGraph {
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        build_cfg(&ast.functions[0].func)
    }

    #[test]
    fn test_cfg_simple_function() {
        let cfg = build_cfg_from_source("function foo() { let x = 1; return x; }");
        assert!(cfg.block_count() >= 2, "至少应有入口和出口");
        assert_eq!(cfg.entry, 0);
        assert!(cfg.max_loop_depth == 0, "无循环");
    }

    #[test]
    fn test_cfg_if_else() {
        let cfg = build_cfg_from_source(
            "function foo(x) { if (x > 0) { return 1; } else { return 0; } }"
        );
        assert!(cfg.block_count() >= 4, "if/else 应产生多个块");
        // 应有分支块
        assert!(cfg.blocks.iter().any(|b| b.kind == BlockKind::Branch));
    }

    #[test]
    fn test_cfg_while_loop() {
        let cfg = build_cfg_from_source(
            "function foo() { let i = 0; while (i < 10) { i = i + 1; } return i; }"
        );
        assert!(cfg.max_loop_depth >= 1, "应检测到循环");
        assert!(cfg.blocks.iter().any(|b| b.kind == BlockKind::LoopHeader));
        assert!(cfg.blocks.iter().any(|b| b.kind == BlockKind::LoopBody));
        // 应有回边
        let headers: Vec<usize> = cfg.blocks.iter()
            .filter(|b| b.kind == BlockKind::LoopHeader)
            .map(|b| b.id)
            .collect();
        for h in &headers {
            let preds = cfg.predecessors(*h);
            assert!(preds.len() >= 2, "循环头应有多个前驱 (入口+回边)");
        }
    }

    #[test]
    fn test_cfg_for_loop() {
        let cfg = build_cfg_from_source(
            "function foo() { for (let i = 0; i < 5; i++) { let x = i; } return 0; }"
        );
        assert!(cfg.max_loop_depth >= 1, "for 循环应检测到");
        assert!(cfg.blocks.iter().any(|b| b.kind == BlockKind::LoopHeader));
    }

    #[test]
    fn test_cfg_function_calls() {
        let cfg = build_cfg_from_source(
            "function foo() { let a = bar(); let b = baz(a); return b; }"
        );
        assert!(cfg.calls.contains(&"bar".to_string()), "应记录 bar 调用");
        assert!(cfg.calls.contains(&"baz".to_string()), "应记录 baz 调用");
    }

    #[test]
    fn test_cfg_nested_loops() {
        let cfg = build_cfg_from_source(
            "function foo() { for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { let x = i; } } return 0; }"
        );
        assert!(cfg.max_loop_depth >= 2, "嵌套循环深度应 >= 2");
    }

    #[test]
    fn test_loop_bound_estimation() {
        let tokens = tokenize("i < 10").unwrap();
        // 手动构建表达式
        let expr = Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(Expr::Ident("i".to_string())),
            right: Box::new(Expr::Number(10)),
        };
        assert_eq!(estimate_loop_bound(&expr), Some(10));
    }

    #[test]
    fn test_loop_bound_unknown() {
        let expr = Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(Expr::Ident("i".to_string())),
            right: Box::new(Expr::Ident("n".to_string())),
        };
        assert_eq!(estimate_loop_bound(&expr), None, "变量上界应返回 None");
    }

    #[test]
    fn test_cfg_switch() {
        let cfg = build_cfg_from_source(
            "function foo(x) { switch (x) { case 1: return 10; case 2: return 20; default: return 0; } }"
        );
        assert!(cfg.blocks.iter().any(|b| b.kind == BlockKind::Branch));
        assert!(cfg.block_count() >= 4, "switch 应产生多个块");
    }
}
