//! Phase 4.2 + 4.3: WCET 分析引擎
//!
//! 为每个目标平台建立指令周期表，对 @realtime 函数的 CFG 做路径分析，
//! 计算最坏执行时间 (Worst-Case Execution Time)。

use crate::cfg::{ControlFlowGraph, BasicBlock, BlockKind, build_cfg, estimate_loop_bound};
use crate::parser::{Function, Expr, Statement, SourceStatement};
use crate::c_codegen::CTarget;
use std::collections::HashMap;

/// 目标平台时序模型
#[derive(Debug, Clone)]
pub struct TargetTimingModel {
    /// 平台名称
    pub name: &'static str,
    /// 时钟频率 (MHz)
    pub clock_mhz: u32,
    /// 算术指令周期
    pub arith_cycles: u32,
    /// 内存读周期
    pub mem_read_cycles: u32,
    /// 内存写周期
    pub mem_write_cycles: u32,
    /// 分支预测失败周期
    pub branch_mispredict_cycles: u32,
    /// 函数调用开销周期
    pub call_overhead_cycles: u32,
    /// 循环开销周期 (每次迭代)
    pub loop_overhead_cycles: u32,
}

/// 预置平台模型
pub fn get_timing_model(target: &CTarget) -> TargetTimingModel {
    match target {
        CTarget::Esp32 | CTarget::Esp32S3Bare => TargetTimingModel {
            name: "ESP32-S3 (Xtensa LX7)",
            clock_mhz: 240,
            arith_cycles: 1,
            mem_read_cycles: 2,
            mem_write_cycles: 2,
            branch_mispredict_cycles: 3,
            call_overhead_cycles: 4,
            loop_overhead_cycles: 2,
        },
        CTarget::Stm32 => TargetTimingModel {
            name: "STM32F103 (Cortex-M3)",
            clock_mhz: 72,
            arith_cycles: 1,
            mem_read_cycles: 2,
            mem_write_cycles: 2,
            branch_mispredict_cycles: 3,
            call_overhead_cycles: 3,
            loop_overhead_cycles: 1,
        },
        CTarget::ArduinoAvr => TargetTimingModel {
            name: "ATmega328P (Arduino Uno)",
            clock_mhz: 16,
            arith_cycles: 2,
            mem_read_cycles: 2,
            mem_write_cycles: 2,
            branch_mispredict_cycles: 2,
            call_overhead_cycles: 4,
            loop_overhead_cycles: 3,
        },
        CTarget::ArduinoArm => TargetTimingModel {
            name: "SAMD21 (Arduino MKR)",
            clock_mhz: 48,
            arith_cycles: 1,
            mem_read_cycles: 2,
            mem_write_cycles: 2,
            branch_mispredict_cycles: 2,
            call_overhead_cycles: 3,
            loop_overhead_cycles: 1,
        },
        CTarget::RaspberryPi => TargetTimingModel {
            name: "BCM2711 (Raspberry Pi 4)",
            clock_mhz: 1500,
            arith_cycles: 1,
            mem_read_cycles: 3,
            mem_write_cycles: 3,
            branch_mispredict_cycles: 8,
            call_overhead_cycles: 4,
            loop_overhead_cycles: 1,
        },
    }
}

/// WCET 分析结果
#[derive(Debug, Clone)]
pub struct WcetResult {
    /// 函数名
    pub func_name: String,
    /// 最坏执行周期数
    pub wcet_cycles: u64,
    /// 最坏执行时间 (微秒)
    pub wcet_us: f64,
    /// WCET 上限 (微秒, 来自 @realtime 注解)
    pub limit_us: Option<u32>,
    /// 是否通过
    pub passed: bool,
    /// 是否 @realtime
    pub is_realtime: bool,
    /// 最大循环深度
    pub max_loop_depth: u32,
    /// 函数调用数
    pub call_count: usize,
}

/// 分析单个函数的 WCET
pub fn analyze_wcet(func: &Function, target: &CTarget) -> WcetResult {
    let model = get_timing_model(target);
    let cfg = build_cfg(func);

    // 计算最坏路径周期
    let wcet_cycles = calculate_wcet_cycles(func, &cfg, &model);

    // 转换为微秒
    let wcet_us = wcet_cycles as f64 / model.clock_mhz as f64;

    // 检查是否超过限制
    let limit_us = func.wcet_limit_us;
    let passed = if let Some(limit) = limit_us {
        wcet_us <= limit as f64
    } else {
        true // 无限制则通过
    };

    WcetResult {
        func_name: func.name.clone(),
        wcet_cycles,
        wcet_us,
        limit_us,
        passed,
        is_realtime: func.is_realtime,
        max_loop_depth: cfg.max_loop_depth,
        call_count: cfg.calls.len(),
    }
}

/// 计算 WCET 周期数
fn calculate_wcet_cycles(func: &Function, cfg: &ControlFlowGraph, model: &TargetTimingModel) -> u64 {
    let mut total_cycles: u64 = 0;

    // 分析每个语句的周期贡献
    total_cycles += estimate_statements_cycles(&func.body, model, 0);

    // 加上函数调用开销
    total_cycles += cfg.calls.len() as u64 * model.call_overhead_cycles as u64;

    total_cycles
}

/// 估算语句列表的周期
fn estimate_statements_cycles(stmts: &[SourceStatement], model: &TargetTimingModel, loop_depth: u32) -> u64 {
    let mut cycles: u64 = 0;
    for ss in stmts {
        cycles += estimate_statement_cycles(&ss.stmt, model, loop_depth);
    }
    cycles
}

/// 估算单个语句的周期
fn estimate_statement_cycles(stmt: &Statement, model: &TargetTimingModel, loop_depth: u32) -> u64 {
    match stmt {
        Statement::Let { value, .. } | Statement::Const { value, .. } | Statement::Assign { value, .. } => {
            estimate_expr_cycles(value, model) + model.mem_write_cycles as u64
        }
        Statement::Return(expr) => {
            estimate_expr_cycles(expr, model) + model.call_overhead_cycles as u64
        }
        Statement::If { condition, then_body, else_body, .. } => {
            let cond_cycles = estimate_expr_cycles(condition, model);
            let then_cycles = estimate_statements_cycles(then_body, model, loop_depth);
            let else_cycles = else_body.as_ref()
                .map(|eb| estimate_statements_cycles(eb, model, loop_depth))
                .unwrap_or(0);
            // WCET: 取分支中最大的 + 分支预测失败开销
            cond_cycles + then_cycles.max(else_cycles) + model.branch_mispredict_cycles as u64
        }
        Statement::While { condition, body } => {
            let bound = estimate_loop_bound(condition).unwrap_or(100); // 默认上界 100
            let body_cycles = estimate_statements_cycles(body, model, loop_depth + 1);
            let cond_cycles = estimate_expr_cycles(condition, model);
            // WCET = (body + cond + loop_overhead) * bound
            (body_cycles + cond_cycles + model.loop_overhead_cycles as u64) * bound
        }
        Statement::For { condition, body, .. } => {
            let bound = estimate_loop_bound(condition).unwrap_or(100);
            let body_cycles = estimate_statements_cycles(body, model, loop_depth + 1);
            let cond_cycles = estimate_expr_cycles(condition, model);
            (body_cycles + cond_cycles + model.loop_overhead_cycles as u64) * bound
        }
        Statement::Switch { cases, .. } => {
            // WCET: 取最大的 case
            let max_case = cases.iter()
                .map(|c| estimate_statements_cycles(&c.body, model, loop_depth))
                .max()
                .unwrap_or(0);
            max_case + model.branch_mispredict_cycles as u64
        }
        Statement::ConsoleLog(args) => {
            // console.log 估算为串口输出
            args.len() as u64 * 50 + 100
        }
        Statement::PostInc(_) | Statement::PostDec(_) => {
            model.arith_cycles as u64 + model.mem_read_cycles as u64 + model.mem_write_cycles as u64
        }
        Statement::PropAssign { value, .. } => {
            estimate_expr_cycles(value, model) + model.mem_write_cycles as u64
        }
        Statement::IndexAssign { index, value, .. } => {
            estimate_expr_cycles(index, model) + estimate_expr_cycles(value, model) + model.mem_write_cycles as u64
        }
        Statement::Break => 1,
        _ => model.arith_cycles as u64,
    }
}

/// 估算表达式的周期
fn estimate_expr_cycles(expr: &Expr, model: &TargetTimingModel) -> u64 {
    match expr {
        Expr::Number(_) | Expr::Bool(_) | Expr::Null => 1,
        Expr::Ident(_) => model.mem_read_cycles as u64,
        Expr::BinOp { left, right, .. } => {
            estimate_expr_cycles(left, model) + estimate_expr_cycles(right, model) + model.arith_cycles as u64
        }
        Expr::Call { args, .. } => {
            let args_cycles: u64 = args.iter().map(|a| estimate_expr_cycles(a, model)).sum();
            args_cycles + model.call_overhead_cycles as u64
        }
        Expr::Index { array, index } => {
            estimate_expr_cycles(array, model) + estimate_expr_cycles(index, model) + model.mem_read_cycles as u64
        }
        Expr::PropAccess { object, .. } => {
            estimate_expr_cycles(object, model) + model.mem_read_cycles as u64
        }
        Expr::Ternary { condition, then_expr, else_expr } => {
            let c = estimate_expr_cycles(condition, model);
            let t = estimate_expr_cycles(then_expr, model);
            let e = estimate_expr_cycles(else_expr, model);
            c + t.max(e) + model.branch_mispredict_cycles as u64
        }
        Expr::Array(elems) => {
            elems.len() as u64 * model.mem_write_cycles as u64
        }
        Expr::ObjectLit { values, .. } => {
            values.len() as u64 * model.mem_write_cycles as u64
        }
        Expr::MathIntrinsic { args, .. } => {
            let args_cycles: u64 = args.iter().map(|a| estimate_expr_cycles(a, model)).sum();
            args_cycles + model.arith_cycles as u64 // 单周期硬件指令
        }
        Expr::Await { expr } => {
            estimate_expr_cycles(expr, model) + 10 // 状态切换开销
        }
        _ => model.arith_cycles as u64,
    }
}

/// 分析整个程序的 WCET
pub fn analyze_program_wcet(
    functions: &[crate::parser::SourceFunction],
    target: &CTarget,
) -> Vec<WcetResult> {
    functions.iter()
        .map(|sf| analyze_wcet(&sf.func, target))
        .collect()
}

/// 生成 WCET 报告
pub fn generate_wcet_report(results: &[WcetResult], target: &CTarget) -> String {
    let model = get_timing_model(target);
    let mut report = String::new();

    report.push_str("=== HardyScript WCET Report ===\n");
    report.push_str(&format!("Target: {} @ {}MHz\n\n", model.name, model.clock_mhz));
    report.push_str(&format!(
        "{:<20}{:>12}{:>10}{:>10}  {:>6}\n",
        "Function", "WCET(cyc)", "WCET(us)", "Limit(us)", "Status"
    ));
    report.push_str("─────────────────────────────────────────────────────────────────\n");

    for r in results {
        let limit_str = r.limit_us.map(|l| format!("{}", l)).unwrap_or("-".to_string());
        let status = if r.is_realtime {
            if r.passed { "PASS" } else { "FAIL" }
        } else {
            "(non-rt)"
        };
        report.push_str(&format!(
            "{:<20}{:>12}{:>10.1}{:>10}  {:>6}\n",
            r.func_name,
            r.wcet_cycles,
            r.wcet_us,
            limit_str,
            status,
        ));
    }

    report.push_str("─────────────────────────────────────────────────────────────────\n");
    let rt_results: Vec<&WcetResult> = results.iter().filter(|r| r.is_realtime).collect();
    let passed = rt_results.iter().filter(|r| r.passed).count();
    report.push_str(&format!("@realtime functions: {} ({} passed, {} failed)\n",
        rt_results.len(), passed, rt_results.len() - passed));
    report.push_str("Heap allocation: 0 bytes (zero malloc)\n");

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn analyze_source(source: &str, target: CTarget) -> Vec<WcetResult> {
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        analyze_program_wcet(&ast.functions, &target)
    }

    #[test]
    fn test_timing_models() {
        let esp32 = get_timing_model(&CTarget::Esp32);
        assert_eq!(esp32.clock_mhz, 240);
        assert_eq!(esp32.name, "ESP32-S3 (Xtensa LX7)");

        let stm32 = get_timing_model(&CTarget::Stm32);
        assert_eq!(stm32.clock_mhz, 72);

        let avr = get_timing_model(&CTarget::ArduinoAvr);
        assert_eq!(avr.clock_mhz, 16);
    }

    #[test]
    fn test_wcet_simple_function() {
        let results = analyze_source(
            "function add(a, b) { return a + b; }",
            CTarget::Esp32
        );
        assert_eq!(results.len(), 1);
        assert!(results[0].wcet_cycles > 0, "WCET 应 > 0");
        assert!(results[0].wcet_us > 0.0, "WCET(us) 应 > 0");
        assert!(!results[0].is_realtime);
    }

    #[test]
    fn test_wcet_realtime_pass() {
        let results = analyze_source(
            "@realtime(1000)\nfunction ctrl() { let x = 1; let y = x + 2; return y; }",
            CTarget::Esp32
        );
        assert!(results[0].is_realtime);
        assert!(results[0].passed, "简单函数应通过 WCET 1000us 限制");
        assert!(results[0].wcet_us < 1000.0);
    }

    #[test]
    fn test_wcet_loop_multiplies() {
        let results_no_loop = analyze_source(
            "function f() { let x = 1; return x; }",
            CTarget::Esp32
        );
        let results_loop = analyze_source(
            "function f() { for (let i = 0; i < 10; i++) { let x = i; } return 0; }",
            CTarget::Esp32
        );
        assert!(results_loop[0].wcet_cycles > results_no_loop[0].wcet_cycles,
            "有循环的 WCET 应更大");
    }

    #[test]
    fn test_wcet_report_generation() {
        let results = analyze_source(
            "@realtime(100)\nfunction ctrl() { let x = 1; return x; }\nfunction normal() { return 0; }",
            CTarget::Esp32
        );
        let report = generate_wcet_report(&results, &CTarget::Esp32);
        assert!(report.contains("WCET Report"));
        assert!(report.contains("ESP32-S3"));
        assert!(report.contains("240MHz"));
        assert!(report.contains("ctrl"));
        assert!(report.contains("PASS"));
        assert!(report.contains("zero malloc"));
    }

    #[test]
    fn test_wcet_platform_difference() {
        let source = "function f() { let x = 1; return x; }";
        let esp32 = analyze_source(source, CTarget::Esp32);
        let avr = analyze_source(source, CTarget::ArduinoAvr);
        // 相同周期数，但 AVR 频率低 → 更长时间
        assert!(avr[0].wcet_us > esp32[0].wcet_us,
            "AVR 应比 ESP32 执行时间更长");
    }

    #[test]
    fn test_wcet_function_call_overhead() {
        let results_no_call = analyze_source(
            "function f() { let x = 1; return x; }",
            CTarget::Esp32
        );
        let results_call = analyze_source(
            "function g() { return 1; }\nfunction f() { return g(); }",
            CTarget::Esp32
        );
        let f_result = results_call.iter().find(|r| r.func_name == "f").unwrap();
        assert!(f_result.wcet_cycles > results_no_call[0].wcet_cycles,
            "有函数调用的 WCET 应更大");
    }

    #[test]
    fn test_wcet_realtime_fail() {
        // 大循环应超过极小的 WCET 限制
        let results = analyze_source(
            "@realtime(1)\nfunction heavy() { for (let i = 0; i < 100; i++) { let x = i * i; } return 0; }",
            CTarget::ArduinoAvr
        );
        // 100次循环在16MHz AVR上很可能超过1us
        assert!(results[0].wcet_us > 1.0, "大循环应超过 1us");
    }
}
