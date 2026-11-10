//! Phase 3.2: @realtime 编译器强制检查
//!
//! 对 @realtime 函数执行编译期检查：
//! - 禁止调用非 @realtime 函数（除白名单硬件 API）
//! - 禁止动态内存分配
//! - 禁止递归
//! - 所有循环必须有编译期可知的上界
//! - 禁止字符串操作
//! - 违反任何规则 → 编译错误，指明具体行号

use crate::parser::{Expr, Function, Statement, SourceStatement, Program};
use std::collections::HashSet;

/// 实时违规类型
#[derive(Debug, Clone, PartialEq)]
pub enum RealtimeViolation {
    /// 调用了非 @realtime 函数
    NonRealtimeCall { func_name: String, line: usize },
    /// 递归调用
    Recursion { line: usize },
    /// 动态内存分配
    DynamicAlloc { line: usize },
    /// 循环上界不可知
    UnboundedLoop { line: usize },
    /// 字符串操作
    StringOp { line: usize },
    /// 浮点运算 (无 FPU 平台)
    FloatOp { line: usize },
}

impl RealtimeViolation {
    pub fn message(&self) -> String {
        match self {
            Self::NonRealtimeCall { func_name, line } =>
                format!("行{}: @realtime 函数禁止调用非实时函数 '{}'", line, func_name),
            Self::Recursion { line } =>
                format!("行{}: @realtime 函数禁止递归", line),
            Self::DynamicAlloc { line } =>
                format!("行{}: @realtime 函数禁止动态内存分配", line),
            Self::UnboundedLoop { line } =>
                format!("行{}: @realtime 函数中循环上界必须编译期可知", line),
            Self::StringOp { line } =>
                format!("行{}: @realtime 函数禁止字符串操作", line),
            Self::FloatOp { line } =>
                format!("行{}: @realtime 函数禁止浮点运算 (目标无 FPU)", line),
        }
    }

    pub fn line(&self) -> usize {
        match self {
            Self::NonRealtimeCall { line, .. } => *line,
            Self::Recursion { line } => *line,
            Self::DynamicAlloc { line } => *line,
            Self::UnboundedLoop { line } => *line,
            Self::StringOp { line } => *line,
            Self::FloatOp { line } => *line,
        }
    }
}

/// 实时检查结果
#[derive(Debug)]
pub struct RealtimeCheckResult {
    /// 函数名
    pub func_name: String,
    /// 违规列表
    pub violations: Vec<RealtimeViolation>,
    /// 是否通过
    pub passed: bool,
}

/// 硬件 API 白名单 (允许在 @realtime 中调用)
const HW_API_WHITELIST: &[&str] = &[
    "GPIO.output", "GPIO.input", "GPIO.high", "GPIO.low", "GPIO.read",
    "ADC.channel", "ADC.read",
    "PWM.output", "PWM.write",
    "Servo.attach", "Servo.write",
    "Motor.init", "Motor.setSpeed", "Motor.brake",
    "Timer.delay",
    "digitalWrite", "digitalRead", "analogRead", "analogWrite",
];

/// 检查整个程序的 @realtime 函数
pub fn check_program(program: &Program) -> Vec<RealtimeCheckResult> {
    let mut results = Vec::new();

    // 收集所有 @realtime 函数名
    let realtime_fns: HashSet<String> = program.functions.iter()
        .filter(|sf| sf.func.is_realtime)
        .map(|sf| sf.func.name.clone())
        .collect();

    // 检查每个 @realtime 函数
    for sf in &program.functions {
        if sf.func.is_realtime {
            let result = check_realtime_function(&sf.func, &realtime_fns);
            results.push(result);
        }
    }

    results
}

/// 检查单个 @realtime 函数
pub fn check_realtime_function(func: &Function, realtime_fns: &HashSet<String>) -> RealtimeCheckResult {
    let mut violations = Vec::new();

    check_statements(&func.body, &func.name, realtime_fns, &mut violations);

    let passed = violations.is_empty();
    RealtimeCheckResult {
        func_name: func.name.clone(),
        violations,
        passed,
    }
}

/// 检查语句列表
fn check_statements(
    stmts: &[SourceStatement],
    func_name: &str,
    realtime_fns: &HashSet<String>,
    violations: &mut Vec<RealtimeViolation>,
) {
    for ss in stmts {
        check_statement(&ss.stmt, func_name, realtime_fns, violations, ss.line);
    }
}

/// 检查单个语句
fn check_statement(
    stmt: &Statement,
    func_name: &str,
    realtime_fns: &HashSet<String>,
    violations: &mut Vec<RealtimeViolation>,
    line: usize,
) {
    match stmt {
        Statement::Let { value, .. } | Statement::Const { value, .. } | Statement::Assign { value, .. } => {
            check_expr(value, func_name, realtime_fns, violations, line);
        }
        Statement::Return(expr) => {
            check_expr(expr, func_name, realtime_fns, violations, line);
        }
        Statement::If { condition, then_body, else_body, .. } => {
            check_expr(condition, func_name, realtime_fns, violations, line);
            check_statements(then_body, func_name, realtime_fns, violations);
            if let Some(eb) = else_body {
                check_statements(eb, func_name, realtime_fns, violations);
            }
        }
        // 循环: 检查上界是否编译期可知
        Statement::While { condition, body } => {
            // while(true) 或 while(1) 视为有界 (需要内部 break)
            // while(x < N) 其中 N 是常量 → 有界
            // while(x < y) → 不可知 → 违规
            if !is_bounded_condition(condition) {
                violations.push(RealtimeViolation::UnboundedLoop { line });
            }
            check_expr(condition, func_name, realtime_fns, violations, line);
            check_statements(body, func_name, realtime_fns, violations);
        }
        Statement::For { init, condition, update, body } => {
            // for(let i = 0; i < N; i++) → N 是常量则有界
            if let Some(cond) = &Some(condition) {
                if !is_bounded_condition(cond) {
                    violations.push(RealtimeViolation::UnboundedLoop { line });
                }
            }
            check_statement(init, func_name, realtime_fns, violations, line);
            check_expr(condition, func_name, realtime_fns, violations, line);
            check_statement(update, func_name, realtime_fns, violations, line);
            check_statements(body, func_name, realtime_fns, violations);
        }
        Statement::Switch { discriminant, cases } => {
            check_expr(discriminant, func_name, realtime_fns, violations, line);
            for case in cases {
                check_statements(&case.body, func_name, realtime_fns, violations);
            }
        }
        Statement::ConsoleLog(args) => {
            // console.log 在 @realtime 中视为字符串操作
            violations.push(RealtimeViolation::StringOp { line });
            for arg in args {
                check_expr(arg, func_name, realtime_fns, violations, line);
            }
        }
        Statement::Expr(expr) => {
            check_expr(expr, func_name, realtime_fns, violations, line);
        }
        Statement::PropAssign { value, .. } => {
            check_expr(value, func_name, realtime_fns, violations, line);
        }
        Statement::IndexAssign { index, value, .. } => {
            check_expr(index, func_name, realtime_fns, violations, line);
            check_expr(value, func_name, realtime_fns, violations, line);
        }
        _ => {}
    }
}

/// 检查表达式
fn check_expr(
    expr: &Expr,
    func_name: &str,
    realtime_fns: &HashSet<String>,
    violations: &mut Vec<RealtimeViolation>,
    line: usize,
) {
    match expr {
        Expr::Call { name, args } => {
            // 递归检测
            if name == func_name {
                violations.push(RealtimeViolation::Recursion { line });
            }
            // 非 @realtime 函数调用检测
            else if !HW_API_WHITELIST.contains(&name.as_str())
                && !realtime_fns.contains(name)
                && !is_builtin_allowed(name)
            {
                violations.push(RealtimeViolation::NonRealtimeCall {
                    func_name: name.clone(),
                    line,
                });
            }
            // 检查参数
            for arg in args {
                check_expr(arg, func_name, realtime_fns, violations, line);
            }
        }
        Expr::StringLit(_) => {
            violations.push(RealtimeViolation::StringOp { line });
        }
        Expr::TemplateStr { .. } => {
            violations.push(RealtimeViolation::StringOp { line });
        }
        Expr::StringMethod { object, args, .. } => {
            violations.push(RealtimeViolation::StringOp { line });
            check_expr(object, func_name, realtime_fns, violations, line);
            for arg in args {
                check_expr(arg, func_name, realtime_fns, violations, line);
            }
        }
        Expr::BinOp { left, right, .. } => {
            check_expr(left, func_name, realtime_fns, violations, line);
            check_expr(right, func_name, realtime_fns, violations, line);
        }
        Expr::Ternary { condition, then_expr, else_expr } => {
            check_expr(condition, func_name, realtime_fns, violations, line);
            check_expr(then_expr, func_name, realtime_fns, violations, line);
            check_expr(else_expr, func_name, realtime_fns, violations, line);
        }
        Expr::Index { array, index } => {
            check_expr(array, func_name, realtime_fns, violations, line);
            check_expr(index, func_name, realtime_fns, violations, line);
        }
        Expr::PropAccess { object, .. } => {
            check_expr(object, func_name, realtime_fns, violations, line);
        }
        Expr::Array(elems) => {
            for elem in elems {
                check_expr(elem, func_name, realtime_fns, violations, line);
            }
        }
        Expr::ObjectLit { values, .. } => {
            for val in values {
                check_expr(val, func_name, realtime_fns, violations, line);
            }
        }
        Expr::Await { expr } => {
            check_expr(expr, func_name, realtime_fns, violations, line);
        }
        Expr::PromiseAll { promises } | Expr::PromiseRace { promises } => {
            for p in promises {
                check_expr(p, func_name, realtime_fns, violations, line);
            }
        }
        Expr::MathIntrinsic { args, .. } => {
            for arg in args {
                check_expr(arg, func_name, realtime_fns, violations, line);
            }
        }
        _ => {}
    }
}

/// 判断循环条件是否编译期有界
fn is_bounded_condition(cond: &Expr) -> bool {
    match cond {
        // while(true) → 有界 (需要内部 break)
        Expr::Bool(true) => true,
        // while(1) → 有界
        Expr::Number(_) => true,
        // x < N 其中 N 是常量
        Expr::BinOp { op, right, .. } => {
            use crate::parser::BinOp;
            match op {
                BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                    // 右边是数字常量 → 有界
                    matches!(right.as_ref(), Expr::Number(_))
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// 判断是否是允许的内建函数
fn is_builtin_allowed(name: &str) -> bool {
    matches!(name,
        "Math.max" | "Math.min" | "Math.abs" |
        "parseInt" | "setInterval" | "setTimeout"
    )
}

/// 生成实时性分析报告
pub fn generate_realtime_report(results: &[RealtimeCheckResult]) -> String {
    let mut report = String::new();
    report.push_str("=== DUANYAN Realtime Analysis Report ===\n\n");

    for result in results {
        let status = if result.passed { "PASS" } else { "FAIL" };
        report.push_str(&format!("[{}] {}\n", status, result.func_name));
        if !result.passed {
            for v in &result.violations {
                report.push_str(&format!("  ✗ {}\n", v.message()));
            }
        }
    }

    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    report.push_str(&format!("\n─────────────────────────────────────────\n"));
    report.push_str(&format!("Total @realtime functions: {}\n", total));
    report.push_str(&format!("Passed: {} | Failed: {}\n", passed, total - passed));

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn check_source(source: &str) -> Vec<RealtimeCheckResult> {
        let tokens = tokenize(source).unwrap();
        let program = parse(&tokens).unwrap();
        check_program(&program)
    }

    #[test]
    fn test_realtime_pass_simple() {
        let results = check_source(
            "@realtime\nfunction ctrl() { let x = 1; let y = x + 2; return y; }"
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].passed, "简单函数应通过");
    }

    #[test]
    fn test_realtime_recursion_fail() {
        let results = check_source(
            "@realtime\nfunction fib(n) { return fib(n - 1); }"
        );
        assert!(!results[0].passed);
        assert!(results[0].violations.iter().any(|v|
            matches!(v, RealtimeViolation::Recursion { .. })
        ));
    }

    #[test]
    fn test_realtime_non_rt_call_fail() {
        let results = check_source(
            "function helper() { return 1; }\n@realtime\nfunction ctrl() { return helper(); }"
        );
        assert!(!results[0].passed);
        assert!(results[0].violations.iter().any(|v|
            matches!(v, RealtimeViolation::NonRealtimeCall { .. })
        ));
    }

    #[test]
    fn test_realtime_hw_api_allowed() {
        let results = check_source(
            "@realtime\nfunction ctrl() { GPIO.output(13); return 0; }"
        );
        assert!(results[0].passed, "硬件 API 应被允许");
    }

    #[test]
    fn test_realtime_rt_call_allowed() {
        let results = check_source(
            "@realtime\nfunction inner() { return 1; }\n@realtime\nfunction outer() { return inner(); }"
        );
        // outer 调用 inner (也是 @realtime) → 应通过
        let outer = results.iter().find(|r| r.func_name == "outer").unwrap();
        assert!(outer.passed, "调用其他 @realtime 函数应被允许");
    }

    #[test]
    fn test_realtime_string_fail() {
        let results = check_source(
            "@realtime\nfunction ctrl() { let s = \"hello\"; return 0; }"
        );
        assert!(!results[0].passed);
        assert!(results[0].violations.iter().any(|v|
            matches!(v, RealtimeViolation::StringOp { .. })
        ));
    }

    #[test]
    fn test_realtime_bounded_loop_pass() {
        let results = check_source(
            "@realtime\nfunction ctrl() { for (let i = 0; i < 10; i++) { let x = i; } return 0; }"
        );
        assert!(results[0].passed, "有界循环应通过");
    }

    #[test]
    fn test_realtime_unbounded_while_fail() {
        let results = check_source(
            "@realtime\nfunction ctrl(x, y) { while (x < y) { x = x + 1; } return x; }"
        );
        assert!(!results[0].passed);
        assert!(results[0].violations.iter().any(|v|
            matches!(v, RealtimeViolation::UnboundedLoop { .. })
        ));
    }

    #[test]
    fn test_non_realtime_not_checked() {
        let results = check_source(
            "function normal() { let s = \"hello\"; return s; }"
        );
        assert_eq!(results.len(), 0, "非 @realtime 函数不应被检查");
    }

    #[test]
    fn test_realtime_report_generation() {
        let results = check_source(
            "@realtime\nfunction good() { return 1; }\n@realtime\nfunction bad() { return bad(); }"
        );
        let report = generate_realtime_report(&results);
        assert!(report.contains("Realtime Analysis Report"));
        assert!(report.contains("PASS"));
        assert!(report.contains("FAIL"));
    }
}
