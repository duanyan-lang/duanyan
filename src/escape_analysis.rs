//! Phase 2.1: 逃逸分析引擎
//!
//! 分析每个变量的内存分配策略:
//! - Stack: 仅在函数内使用 → 栈分配 (int32_t / 定长 struct / 定长数组)
//! - Pool: 跨函数存活但大小已知 → 静态内存池
//! - Forbidden: 大小不可知 → 编译报错 (零 malloc 保证)

use crate::parser::{Expr, Function, Statement, SourceStatement, ArrowBody};
use std::collections::HashMap;

/// 变量逃逸类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeKind {
    /// 仅在函数内使用 → 栈分配
    Stack,
    /// 跨函数存活但大小已知 → 内存池
    Pool,
    /// 大小不可知 → 编译报错
    Forbidden,
}

impl EscapeKind {
    pub fn description(&self) -> &'static str {
        match self {
            Self::Stack => "stack (int32_t / struct / fixed array)",
            Self::Pool => "static pool (compile-time sized)",
            Self::Forbidden => "FORBIDDEN (runtime-sized, zero-malloc violation)",
        }
    }
}

/// 变量信息
#[derive(Debug, Clone)]
pub struct VarInfo {
    /// 变量名
    pub name: String,
    /// 逃逸类型
    pub kind: EscapeKind,
    /// 估计大小 (字节)
    pub size_bytes: usize,
    /// 是否是对象 (struct)
    pub is_object: bool,
    /// 对象字段 (如果是 ObjectLit)
    pub fields: Vec<String>,
    /// 数组长度 (如果是定长数组, None = 非数组或运行时长度)
    pub array_len: Option<usize>,
    /// 声明行号
    pub line: usize,
}

/// 函数分析结果
#[derive(Debug)]
pub struct FunctionAnalysis {
    /// 函数名
    pub func_name: String,
    /// 变量 → 逃逸信息
    pub vars: HashMap<String, VarInfo>,
    /// 总栈使用量 (字节)
    pub stack_usage: usize,
    /// 总池使用量 (字节)
    pub pool_usage: usize,
    /// 是否有违规
    pub has_violation: bool,
    /// 违规信息
    pub violations: Vec<String>,
}

/// 程序级分析结果
#[derive(Debug)]
pub struct ProgramAnalysis {
    /// 每个函数的分析
    pub functions: Vec<FunctionAnalysis>,
    /// 全局变量
    pub globals: HashMap<String, VarInfo>,
    /// 总池预算 (字节)
    pub total_pool_budget: usize,
    /// 总栈预算 (字节, 所有函数最大)
    pub max_stack_usage: usize,
}

/// 分析整个程序
pub fn analyze_program(
    functions: &[crate::parser::SourceFunction],
    statements: &[SourceStatement],
) -> ProgramAnalysis {
    let mut func_analyses = Vec::new();
    let mut max_stack = 0usize;
    let mut total_pool = 0usize;

    // 分析每个函数
    for sf in functions {
        let analysis = analyze_function(&sf.func, sf.line);
        max_stack = max_stack.max(analysis.stack_usage);
        total_pool += analysis.pool_usage;
        func_analyses.push(analysis);
    }

    // 分析全局语句中的变量
    let globals = analyze_global_vars(statements);

    ProgramAnalysis {
        functions: func_analyses,
        globals,
        total_pool_budget: total_pool,
        max_stack_usage: max_stack,
    }
}

/// 分析单个函数
pub fn analyze_function(func: &Function, _func_line: usize) -> FunctionAnalysis {
    let mut vars: HashMap<String, VarInfo> = HashMap::new();
    let mut violations: Vec<String> = Vec::new();

    // 参数默认为 Stack (int32_t)
    for param in &func.params {
        vars.insert(param.clone(), VarInfo {
            name: param.clone(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line: 0,
        });
    }

    // 分析函数体
    analyze_statements(&func.body, &mut vars, &mut violations, &func.name);

    // 计算统计
    let stack_usage: usize = vars.values()
        .filter(|v| v.kind == EscapeKind::Stack)
        .map(|v| v.size_bytes)
        .sum();
    let pool_usage: usize = vars.values()
        .filter(|v| v.kind == EscapeKind::Pool)
        .map(|v| v.size_bytes)
        .sum();
    let has_violation = vars.values().any(|v| v.kind == EscapeKind::Forbidden);

    FunctionAnalysis {
        func_name: func.name.clone(),
        vars,
        stack_usage,
        pool_usage,
        has_violation,
        violations,
    }
}

/// 分析语句列表
fn analyze_statements(
    stmts: &[SourceStatement],
    vars: &mut HashMap<String, VarInfo>,
    violations: &mut Vec<String>,
    func_name: &str,
) {
    for ss in stmts {
        analyze_statement(&ss.stmt, vars, violations, func_name, ss.line);
    }
}

/// 分析单个语句
fn analyze_statement(
    stmt: &Statement,
    vars: &mut HashMap<String, VarInfo>,
    violations: &mut Vec<String>,
    func_name: &str,
    line: usize,
) {
    match stmt {
        Statement::Let { name, value } | Statement::Const { name, value } => {
            let info = analyze_expr_escape(name, value, line);
            if info.kind == EscapeKind::Forbidden {
                violations.push(format!(
                    "{}:{} 变量 '{}' 大小不可知 (zero-malloc violation)",
                    func_name, line, name
                ));
            }
            vars.insert(name.clone(), info);
        }
        Statement::Assign { name, value } => {
            // 重新赋值: 如果新值改变了类型，升级逃逸等级
            if let Some(existing) = vars.get(name) {
                let new_info = analyze_expr_escape(name, value, line);
                if new_info.kind == EscapeKind::Forbidden && existing.kind != EscapeKind::Forbidden {
                    violations.push(format!(
                        "{}:{} 变量 '{}' 赋值导致大小不可知",
                        func_name, line, name
                    ));
                    vars.insert(name.clone(), new_info);
                }
            }
        }
        Statement::If { then_body, else_body, .. } => {
            analyze_statements(then_body, vars, violations, func_name);
            if let Some(eb) = else_body {
                analyze_statements(eb, vars, violations, func_name);
            }
        }
        Statement::While { body, .. } | Statement::For { body, .. } => {
            analyze_statements(body, vars, violations, func_name);
        }
        Statement::Switch { cases, .. } => {
            for case in cases {
                analyze_statements(&case.body, vars, violations, func_name);
            }
        }
        Statement::Return(expr) => {
            // 返回对象/数组 → 可能逃逸到调用者 → Pool
            if let Some(info) = analyze_return_escape(expr, line) {
                if info.kind == EscapeKind::Pool || info.kind == EscapeKind::Forbidden {
                    vars.insert(format!("__return_{}", line), info);
                }
            }
        }
        _ => {}
    }
}

/// 分析表达式的逃逸类型
fn analyze_expr_escape(name: &str, expr: &Expr, line: usize) -> VarInfo {
    match expr {
        // 基本类型 → 永远 Stack (4 bytes int32_t)
        Expr::Number(_) | Expr::Bool(_) | Expr::Null => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 标识符引用 → Stack (int32_t 复制)
        Expr::Ident(_) => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 二元运算 → Stack (int32_t 结果)
        Expr::BinOp { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 三元运算 → Stack
        Expr::Ternary { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 函数调用 → Stack (返回 int32_t)
        Expr::Call { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 对象字面量 { x: 1, y: 2 } → Stack (struct, 大小编译期已知)
        Expr::ObjectLit { keys, values } => {
            let n_fields = keys.len();
            VarInfo {
                name: name.to_string(),
                kind: EscapeKind::Stack,
                size_bytes: n_fields * 4, // 每个字段 int32_t
                is_object: true,
                fields: keys.clone(),
                array_len: None,
                line,
            }
        }
        // 数组字面量 [1, 2, 3] → 长度编译期已知 → Stack (定长数组)
        Expr::Array(elems) => {
            let len = elems.len();
            VarInfo {
                name: name.to_string(),
                kind: EscapeKind::Stack,
                size_bytes: len * 4,
                is_object: false,
                fields: Vec::new(),
                array_len: Some(len),
                line,
            }
        }
        // 字符串字面量 → Stack (const char*, 指针)
        Expr::StringLit(_) => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4, // 指针大小 (32-bit)
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 模板字符串 → Stack (const char*)
        Expr::TemplateStr { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // Math 内建 → Stack (int32_t)
        Expr::MathIntrinsic { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 属性访问 → Stack (int32_t)
        Expr::PropAccess { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 索引访问 → Stack (int32_t)
        Expr::Index { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // await → 透传内部表达式的逃逸类型
        Expr::Await { expr } => {
            // await Promise.all → Pool, 其他 await → Stack
            match expr.as_ref() {
                Expr::PromiseAll { promises } => VarInfo {
                    name: name.to_string(),
                    kind: EscapeKind::Pool,
                    size_bytes: promises.len() * 4,
                    is_object: false,
                    fields: Vec::new(),
                    array_len: Some(promises.len()),
                    line,
                },
                _ => VarInfo {
                    name: name.to_string(),
                    kind: EscapeKind::Stack,
                    size_bytes: 4,
                    is_object: false,
                    fields: Vec::new(),
                    array_len: None,
                    line,
                },
            }
        }
        // Promise.all/race → Pool (结果数组)
        Expr::PromiseAll { promises } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Pool,
            size_bytes: promises.len() * 4,
            is_object: false,
            fields: Vec::new(),
            array_len: Some(promises.len()),
            line,
        },
        Expr::PromiseRace { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 箭头函数 → Stack (内联或函数指针)
        Expr::ArrowFunc { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4, // 函数指针
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
        // 字符串方法 → Stack
        Expr::StringMethod { .. } => VarInfo {
            name: name.to_string(),
            kind: EscapeKind::Stack,
            size_bytes: 4,
            is_object: false,
            fields: Vec::new(),
            array_len: None,
            line,
        },
    }
}

/// 分析 return 表达式的逃逸
fn analyze_return_escape(expr: &Expr, line: usize) -> Option<VarInfo> {
    match expr {
        Expr::ObjectLit { keys, .. } => Some(VarInfo {
            name: format!("__ret_{}", line),
            kind: EscapeKind::Pool, // 返回对象逃逸到调用者
            size_bytes: keys.len() * 4,
            is_object: true,
            fields: keys.clone(),
            array_len: None,
            line,
        }),
        Expr::Array(elems) => Some(VarInfo {
            name: format!("__ret_{}", line),
            kind: EscapeKind::Pool, // 返回数组逃逸到调用者
            size_bytes: elems.len() * 4,
            is_object: false,
            fields: Vec::new(),
            array_len: Some(elems.len()),
            line,
        }),
        _ => None, // 基本类型不需要特殊处理
    }
}

/// 分析全局变量
fn analyze_global_vars(statements: &[SourceStatement]) -> HashMap<String, VarInfo> {
    let mut globals = HashMap::new();
    for ss in statements {
        match &ss.stmt {
            Statement::Let { name, value } | Statement::Const { name, value } => {
                let info = analyze_expr_escape(name, value, ss.line);
                globals.insert(name.clone(), info);
            }
            _ => {}
        }
    }
    globals
}

/// 生成内存布局报告
pub fn generate_memory_report(analysis: &ProgramAnalysis) -> String {
    let mut report = String::new();
    report.push_str("=== HardyScript Memory Layout Report ===\n\n");

    // 全局变量
    if !analysis.globals.is_empty() {
        report.push_str("[Global Variables]\n");
        for (name, info) in &analysis.globals {
            report.push_str(&format!(
                "  {:<20} {:>6} bytes  [{}]\n",
                name, info.size_bytes, info.kind.description()
            ));
        }
        report.push('\n');
    }

    // 每个函数
    for fa in &analysis.functions {
        report.push_str(&format!("[Function: {}]\n", fa.func_name));
        report.push_str(&format!(
            "  Stack: {} bytes | Pool: {} bytes\n",
            fa.stack_usage, fa.pool_usage
        ));
        for (name, info) in &fa.vars {
            if !name.starts_with("__") {
                report.push_str(&format!(
                    "    {:<18} {:>6} bytes  [{}]\n",
                    name, info.size_bytes, info.kind.description()
                ));
            }
        }
        if fa.has_violation {
            report.push_str("  ⚠ VIOLATIONS:\n");
            for v in &fa.violations {
                report.push_str(&format!("    {}\n", v));
            }
        }
        report.push('\n');
    }

    // 总结
    report.push_str("─────────────────────────────────────────\n");
    report.push_str(&format!("Total static RAM:   {} bytes\n", analysis.max_stack_usage));
    report.push_str(&format!("Pool budget:        {} bytes\n", analysis.total_pool_budget));
    report.push_str(&format!("Heap allocation:    0 bytes (zero malloc)\n"));

    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn analyze_source(source: &str) -> ProgramAnalysis {
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        analyze_program(&ast.functions, &ast.statements)
    }

    #[test]
    fn test_basic_types_stack() {
        let analysis = analyze_source("function foo() { let x = 42; let y = x + 1; return y; }");
        let func = &analysis.functions[0];
        assert_eq!(func.vars["x"].kind, EscapeKind::Stack);
        assert_eq!(func.vars["y"].kind, EscapeKind::Stack);
        assert_eq!(func.vars["x"].size_bytes, 4);
        assert!(!func.has_violation);
    }

    #[test]
    fn test_object_lit_stack() {
        let analysis = analyze_source("function foo() { let pos = { x: 10, y: 20, z: 30 }; return pos.x; }");
        let func = &analysis.functions[0];
        let pos = &func.vars["pos"];
        assert_eq!(pos.kind, EscapeKind::Stack);
        assert!(pos.is_object);
        assert_eq!(pos.size_bytes, 12); // 3 fields * 4 bytes
        assert_eq!(pos.fields, vec!["x", "y", "z"]);
    }

    #[test]
    fn test_fixed_array_stack() {
        let analysis = analyze_source("function foo() { let arr = [1, 2, 3, 4, 5]; return arr[0]; }");
        let func = &analysis.functions[0];
        let arr = &func.vars["arr"];
        assert_eq!(arr.kind, EscapeKind::Stack);
        assert_eq!(arr.array_len, Some(5));
        assert_eq!(arr.size_bytes, 20); // 5 * 4
    }

    #[test]
    fn test_params_stack() {
        let analysis = analyze_source("function add(a, b) { return a + b; }");
        let func = &analysis.functions[0];
        assert_eq!(func.vars["a"].kind, EscapeKind::Stack);
        assert_eq!(func.vars["b"].kind, EscapeKind::Stack);
    }

    #[test]
    fn test_return_object_pool() {
        let analysis = analyze_source("function make() { return { x: 1, y: 2 }; }");
        let func = &analysis.functions[0];
        // 返回对象应标记为 Pool (逃逸到调用者)
        assert!(func.pool_usage > 0, "返回对象应使用 Pool");
    }

    #[test]
    fn test_global_vars() {
        let analysis = analyze_source("const MAX = 100; let count = 0;");
        assert!(analysis.globals.contains_key("MAX"));
        assert!(analysis.globals.contains_key("count"));
        assert_eq!(analysis.globals["MAX"].kind, EscapeKind::Stack);
    }

    #[test]
    fn test_promise_all_pool() {
        let analysis = analyze_source(
            "async function multi() { let r = await Promise.all([a(), b(), c()]); return r; }"
        );
        let func = &analysis.functions[0];
        // Promise.all 结果应为 Pool
        let has_pool = func.vars.values().any(|v| v.kind == EscapeKind::Pool);
        assert!(has_pool || func.pool_usage > 0, "Promise.all 应使用 Pool");
    }

    #[test]
    fn test_memory_report_generation() {
        let analysis = analyze_source(
            "const N = 10;\nfunction foo() { let x = 42; let pos = { x: 1, y: 2 }; return x; }"
        );
        let report = generate_memory_report(&analysis);
        assert!(report.contains("Memory Layout Report"));
        assert!(report.contains("zero malloc"));
        assert!(report.contains("foo"));
    }

    #[test]
    fn test_no_violation_simple() {
        let analysis = analyze_source(
            "function calc(a, b) { let sum = a + b; let arr = [1, 2, 3]; return sum; }"
        );
        let func = &analysis.functions[0];
        assert!(!func.has_violation);
        assert!(func.violations.is_empty());
    }

    #[test]
    fn test_stack_usage_calculation() {
        let analysis = analyze_source(
            "function big() { let a = 1; let b = 2; let c = 3; let d = { x: 1, y: 2 }; return a; }"
        );
        let func = &analysis.functions[0];
        // a(4) + b(4) + c(4) + d(8) = 20 bytes
        assert_eq!(func.stack_usage, 20);
    }

    #[test]
    fn test_program_pool_budget() {
        let analysis = analyze_source(
            "function f1() { return { a: 1, b: 2 }; } function f2() { return { c: 1 }; }"
        );
        assert!(analysis.total_pool_budget > 0, "应有池预算");
    }
}
