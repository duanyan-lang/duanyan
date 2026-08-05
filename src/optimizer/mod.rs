//! 优化器 (Optimizer)
//!
//! Phase 4: 编译期优化 pass
//! - 常量折叠 (Constant Folding)
//! - 死代码消除 (Dead Code Elimination)

use crate::parser::{SourceStatement, SourceFunction, Program, Statement, Expr, BinOp, Function, SwitchCase};

/// 对 AST 执行优化 pass
pub fn optimize(program: &Program) -> Program {
    let mut opt = Optimizer::new();
    opt.optimize_program(program)
}

struct Optimizer;

impl Optimizer {
    fn new() -> Self { Self }

    fn optimize_program(&mut self, program: &Program) -> Program {
        Program {
            statements: {
                let optimized: Vec<SourceStatement> = program.statements.iter()
                    .map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line })
                    .collect();
                optimized.into_iter().filter(|ss| !self.is_dead_code(&ss.stmt)).collect()
            },
            functions: {
                let opt_fns: Vec<SourceFunction> = program.functions.iter()
                    .map(|sf| SourceFunction { func: self.optimize_function(&sf.func), line: sf.line })
                    .collect();
                opt_fns
            },
        }
    }

    fn optimize_function(&mut self, func: &Function) -> Function {
        Function {
            name: func.name.clone(),
            params: func.params.clone(),
            is_async: func.is_async,
            is_realtime: func.is_realtime,
            wcet_limit_us: func.wcet_limit_us,
            interrupt_timer: func.interrupt_timer.clone(),
            interrupt_period_us: func.interrupt_period_us,
            priority: func.priority,
            body: {
                let optimized: Vec<SourceStatement> = func.body.iter()
                    .map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line })
                    .collect();
                optimized.into_iter().filter(|ss| !self.is_dead_code(&ss.stmt)).collect()
            },
        }
    }

    fn optimize_statement(&mut self, stmt: &Statement) -> Statement {
        match stmt {
            Statement::Let { name, value } => Statement::Let {
                name: name.clone(),
                value: self.fold_expr(value),
            },
            Statement::Assign { name, value } => Statement::Assign {
                name: name.clone(),
                value: self.fold_expr(value),
            },
            Statement::IndexAssign { name, index, value } => Statement::IndexAssign {
                name: name.clone(),
                index: self.fold_expr(index),
                value: self.fold_expr(value),
            },
            Statement::Return(expr) => Statement::Return(self.fold_expr(expr)),
            Statement::If { condition, then_body, else_body } => {
                let cond = self.fold_expr(condition);
                // 如果条件是常量，消除无用分支
                if let Expr::Number(n) = &cond {
                    if *n != 0 {
                        // 条件恒真，直接展开 then
                        let optimized_body: Vec<SourceStatement> = then_body.iter()
                            .map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line })
                            .collect();
                        // 返回第一条语句（简化处理）
                        if optimized_body.len() == 1 {
                            return optimized_body.into_iter().next().unwrap().stmt;
                        }
                    } else {
                        // 条件恒假，展开 else
                        if let Some(else_stmts) = else_body {
                            let optimized_body: Vec<SourceStatement> = else_stmts.iter()
                                .map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line })
                                .collect();
                            if optimized_body.len() == 1 {
                                return optimized_body.into_iter().next().unwrap().stmt;
                            }
                        }
                    }
                }
                Statement::If {
                    condition: cond,
                    then_body: then_body.iter().map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line }).collect(),
                    else_body: else_body.as_ref().map(|stmts| {
                        stmts.iter().map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line }).collect()
                    }),
                }
            }
            Statement::While { condition, body } => {
                let cond = self.fold_expr(condition);
                // while(0) → 死代码
                if let Expr::Number(0) = &cond {
                    return Statement::Expr(Expr::Number(0)); // 标记为死代码
                }
                Statement::While {
                    condition: cond,
                    body: body.iter().map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line }).collect(),
                }
            }
            Statement::For { init, condition, update, body } => {
                let cond = self.fold_expr(condition);
                Statement::For {
                    init: Box::new(self.optimize_statement(init)),
                    condition: cond,
                    update: Box::new(self.optimize_statement(update)),
                    body: body.iter().map(|ss| SourceStatement { stmt: self.optimize_statement(&ss.stmt), line: ss.line }).collect(),
                }
            }
            Statement::PostInc(name) => Statement::PostInc(name.clone()),
            Statement::PostDec(name) => Statement::PostDec(name.clone()),
            Statement::PropAssign { object, property, value } => Statement::PropAssign {
                object: object.clone(),
                property: property.clone(),
                value: self.fold_expr(value),
            },
            Statement::ConsoleLog(args) => Statement::ConsoleLog(
                args.iter().map(|e| self.fold_expr(e)).collect()
            ),
            Statement::Expr(expr) => Statement::Expr(self.fold_expr(expr)),
            // Phase A: const 与 let 相同
            Statement::Const { name, value } => Statement::Const {
                name: name.clone(),
                value: self.fold_expr(value),
            },
            // Phase A.3: switch
            Statement::Switch { discriminant, cases } => {
                let disc = self.fold_expr(discriminant);
                let cases: Vec<SwitchCase> = cases.iter().map(|c| {
                    SwitchCase {
                        test: c.test.as_ref().map(|e| self.fold_expr(e)),
                        body: c.body.iter().map(|ss| SourceStatement {
                            stmt: self.optimize_statement(&ss.stmt),
                            line: ss.line,
                        }).collect(),
                    }
                }).collect();
                Statement::Switch { discriminant: disc, cases }
            }
            // Phase A.3: break
            Statement::Break => Statement::Break,
            // Phase A.4: import
            Statement::Import { items, source } => Statement::Import {
                items: items.clone(),
                source: source.clone(),
            },
            // Phase A.4: export
            Statement::Export { item } => Statement::Export {
                item: Box::new(self.optimize_statement(item)),
            },
            // Phase A.6: 解构赋值
            Statement::DestructLet { names, source } => Statement::DestructLet {
                names: names.clone(),
                source: self.fold_expr(source),
            },
        }
    }

    /// 常量折叠：编译期计算常量表达式
    fn fold_expr(&mut self, expr: &Expr) -> Expr {
        match expr {
            Expr::BinOp { op, left, right } => {
                let l = self.fold_expr(left);
                let r = self.fold_expr(right);

                // 如果两边都是常量，直接计算
                if let (Expr::Number(lv), Expr::Number(rv)) = (&l, &r) {
                    if let Some(result) = self.eval_const(*op, *lv, *rv) {
                        return Expr::Number(result);
                    }
                }

                // 代数简化
                match op {
                    BinOp::Add => {
                        if l == Expr::Number(0) { return r; }
                        if r == Expr::Number(0) { return l; }
                    }
                    BinOp::Sub => {
                        if r == Expr::Number(0) { return l; }
                    }
                    BinOp::Mul => {
                        if l == Expr::Number(0) || r == Expr::Number(0) {
                            return Expr::Number(0);
                        }
                        if l == Expr::Number(1) { return r; }
                        if r == Expr::Number(1) { return l; }
                    }
                    BinOp::Div => {
                        if l == Expr::Number(0) { return Expr::Number(0); }
                        if r == Expr::Number(1) { return l; }
                    }
                    _ => {}
                }

                Expr::BinOp {
                    op: *op,
                    left: Box::new(l),
                    right: Box::new(r),
                }
            }
            Expr::Call { name, args } => Expr::Call {
                name: name.clone(),
                args: args.iter().map(|e| self.fold_expr(e)).collect(),
            },
            Expr::Array(elements) => Expr::Array(
                elements.iter().map(|e| self.fold_expr(e)).collect()
            ),
            Expr::Index { array, index } => Expr::Index {
                array: Box::new(self.fold_expr(array)),
                index: Box::new(self.fold_expr(index)),
            },
            Expr::ObjectLit { keys, values } => Expr::ObjectLit {
                keys: keys.clone(),
                values: values.iter().map(|e| self.fold_expr(e)).collect(),
            },
            Expr::PropAccess { object, property } => Expr::PropAccess {
                object: Box::new(self.fold_expr(object)),
                property: property.clone(),
            },
            other => other.clone(),
        }
    }

    /// 编译期计算
    fn eval_const(&self, op: BinOp, lv: i64, rv: i64) -> Option<i64> {
        match op {
            BinOp::Add => Some(lv.wrapping_add(rv)),
            BinOp::Sub => Some(lv.wrapping_sub(rv)),
            BinOp::Mul => Some(lv.wrapping_mul(rv)),
            BinOp::Div => {
                if rv == 0 { None } else { Some(lv / rv) }
            }
            BinOp::Mod => {
                if rv == 0 { None } else { Some(lv % rv) }
            }
            BinOp::Eq => Some(if lv == rv { 1 } else { 0 }),
            BinOp::NotEq => Some(if lv != rv { 1 } else { 0 }),
            BinOp::Lt => Some(if lv < rv { 1 } else { 0 }),
            BinOp::Gt => Some(if lv > rv { 1 } else { 0 }),
            BinOp::Le => Some(if lv <= rv { 1 } else { 0 }),
            BinOp::Ge => Some(if lv >= rv { 1 } else { 0 }),
            BinOp::And => Some(if lv != 0 && rv != 0 { 1 } else { 0 }),
            BinOp::Or => Some(if lv != 0 || rv != 0 { 1 } else { 0 }),
            // Phase A: === 和 !== 对整数与 == / != 等价
            BinOp::StrictEq => Some(if lv == rv { 1 } else { 0 }),
            BinOp::StrictNotEq => Some(if lv != rv { 1 } else { 0 }),
        }
    }

    /// 判断是否为死代码（无效的表达式语句）
    fn is_dead_code(&self, stmt: &Statement) -> bool {
        matches!(stmt, Statement::Expr(Expr::Number(_)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    #[test]
    fn test_constant_folding() {
        let tokens = tokenize("let x = 3 + 4 * 2;").unwrap();
        let ast = parse(&tokens).unwrap();
        let optimized = optimize(&ast);
        // 3 + 4*2 = 3 + 8 = 11
        match &optimized.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert_eq!(*value, Expr::Number(11), "常量折叠: 3+4*2=11");
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_zero_multiply() {
        let tokens = tokenize("let x = a * 0;").unwrap();
        let ast = parse(&tokens).unwrap();
        let optimized = optimize(&ast);
        match &optimized.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert_eq!(*value, Expr::Number(0), "x*0=0");
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_identity_add() {
        let tokens = tokenize("let x = a + 0;").unwrap();
        let ast = parse(&tokens).unwrap();
        let optimized = optimize(&ast);
        match &optimized.statements[0].stmt {
            Statement::Let { value, .. } => {
                assert_eq!(*value, Expr::Ident("a".to_string()), "a+0=a");
            }
            _ => panic!("期望 Let"),
        }
    }

    #[test]
    fn test_dead_while_elimination() {
        let tokens = tokenize("let a = 1; while (0) { a = 2; }").unwrap();
        let ast = parse(&tokens).unwrap();
        let optimized = optimize(&ast);
        // while(0) 应被消除
        assert_eq!(optimized.statements.len(), 1, "while(0) 应被死代码消除");
    }
}
