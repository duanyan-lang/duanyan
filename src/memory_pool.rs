//! Phase 2.2: 静态内存池生成器
//!
//! 对逃逸分析中 Pool 类型的变量，在生成的 C 代码中创建静态内存池。
//! 池大小由编译器根据逃逸分析结果自动计算，零 malloc。

use crate::escape_analysis::{ProgramAnalysis, EscapeKind};

/// 内存池配置
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// 池总大小 (字节)
    pub pool_size: usize,
    /// 对齐要求 (字节, 默认 4)
    pub alignment: usize,
    /// 是否在溢出时 panic (true) 或 静默失败 (false)
    pub panic_on_overflow: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            pool_size: 4096,
            alignment: 4,
            panic_on_overflow: true,
        }
    }
}

/// 根据逃逸分析结果计算最优池大小
pub fn calculate_pool_size(analysis: &ProgramAnalysis) -> usize {
    let mut total = 0usize;

    // 全局 Pool 变量
    for info in analysis.globals.values() {
        if info.kind == EscapeKind::Pool {
            total += align_up(info.size_bytes, 4);
        }
    }

    // 每个函数的 Pool 变量
    for func in &analysis.functions {
        for info in func.vars.values() {
            if info.kind == EscapeKind::Pool {
                total += align_up(info.size_bytes, 4);
            }
        }
    }

    // 加上 25% 余量
    let with_margin = total + total / 4;
    // 最小 256 字节, 对齐到 16
    let final_size = if with_margin < 256 { 256 } else { align_up(with_margin, 16) };
    final_size
}

/// 生成内存池 C 代码
pub fn generate_pool_c_code(config: &PoolConfig) -> String {
    let mut code = String::new();

    code.push_str("/* HardyScript Memory Pool — compile-time generated, zero heap alloc */\n");
    code.push_str(&format!("static uint8_t _hardy_pool[{}];\n", config.pool_size));
    code.push_str("static uint32_t _hardy_pool_offset = 0;\n\n");

    // 分配函数
    code.push_str("static void* _hardy_pool_alloc(uint32_t size) {\n");
    code.push_str(&format!(
        "    uint32_t aligned = (size + {}) & ~{};\n",
        config.alignment - 1,
        config.alignment - 1
    ));
    code.push_str("    if (_hardy_pool_offset + aligned > sizeof(_hardy_pool)) {\n");
    if config.panic_on_overflow {
        code.push_str("        /* 硬错误：池溢出，编译期预算不足 */\n");
        code.push_str("        while(1); /* halt — POOL_OVERFLOW */\n");
    } else {
        code.push_str("        return (void*)0; /* null on overflow */\n");
    }
    code.push_str("    }\n");
    code.push_str("    void* ptr = &_hardy_pool[_hardy_pool_offset];\n");
    code.push_str("    _hardy_pool_offset += aligned;\n");
    code.push_str("    return ptr;\n");
    code.push_str("}\n\n");

    // 重置函数 (用于测试或循环使用)
    code.push_str("static inline void _hardy_pool_reset(void) {\n");
    code.push_str("    _hardy_pool_offset = 0;\n");
    code.push_str("}\n\n");

    // 使用量查询
    code.push_str("static inline uint32_t _hardy_pool_used(void) {\n");
    code.push_str("    return _hardy_pool_offset;\n");
    code.push_str("}\n\n");

    // 剩余量查询
    code.push_str("static inline uint32_t _hardy_pool_free(void) {\n");
    code.push_str(&format!(
        "    return {} - _hardy_pool_offset;\n",
        config.pool_size
    ));
    code.push_str("}\n\n");

    code
}

/// 生成内存布局报告 (用于 --check-memory 命令)
pub fn generate_memory_layout_report(analysis: &ProgramAnalysis, config: &PoolConfig) -> String {
    let mut report = String::new();
    report.push_str("=== HardyScript Memory Pool Report ===\n\n");
    report.push_str(&format!("Pool size:       {} bytes\n", config.pool_size));
    report.push_str(&format!("Alignment:       {} bytes\n", config.alignment));
    report.push_str(&format!("Panic on overflow: {}\n\n", config.panic_on_overflow));

    // 池分配明细
    let mut offset = 0usize;
    report.push_str("[Pool Allocations]\n");

    // 全局
    for (name, info) in &analysis.globals {
        if info.kind == EscapeKind::Pool {
            let aligned = align_up(info.size_bytes, config.alignment);
            report.push_str(&format!(
                "  0x{:04X}  {:<20} {} bytes\n",
                offset, name, info.size_bytes
            ));
            offset += aligned;
        }
    }

    // 函数
    for func in &analysis.functions {
        for (name, info) in &func.vars {
            if info.kind == EscapeKind::Pool && !name.starts_with("__") {
                let aligned = align_up(info.size_bytes, config.alignment);
                report.push_str(&format!(
                    "  0x{:04X}  {:<20} {} bytes  ({})\n",
                    offset, name, info.size_bytes, func.func_name
                ));
                offset += aligned;
            }
        }
    }

    report.push_str(&format!("\nTotal pool used:  {} / {} bytes ({:.0}%)\n",
        offset, config.pool_size,
        if config.pool_size > 0 { offset as f64 / config.pool_size as f64 * 100.0 } else { 0.0 }
    ));
    report.push_str(&format!("Heap allocation:  0 bytes (zero malloc guaranteed)\n"));

    report
}

/// 对齐到指定边界
fn align_up(size: usize, alignment: usize) -> usize {
    (size + alignment - 1) & !(alignment - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escape_analysis::analyze_program;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn analyze(source: &str) -> ProgramAnalysis {
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        analyze_program(&ast.functions, &ast.statements)
    }

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.pool_size, 4096);
        assert_eq!(config.alignment, 4);
        assert!(config.panic_on_overflow);
    }

    #[test]
    fn test_calculate_pool_size_basic() {
        let analysis = analyze("function f() { return { x: 1, y: 2 }; }");
        let size = calculate_pool_size(&analysis);
        // 返回对象 {x, y} = 8 bytes + 25% margin = 10 → aligned to 16 = 16
        // 但最小 256
        assert!(size >= 256, "池大小至少 256 字节");
    }

    #[test]
    fn test_calculate_pool_size_no_pool() {
        let analysis = analyze("function f() { let x = 42; return x; }");
        let size = calculate_pool_size(&analysis);
        // 没有 Pool 变量 → 最小 256
        assert_eq!(size, 256);
    }

    #[test]
    fn test_generate_pool_c_code() {
        let config = PoolConfig { pool_size: 1024, ..Default::default() };
        let code = generate_pool_c_code(&config);
        assert!(code.contains("static uint8_t _hardy_pool[1024]"), "应生成池数组");
        assert!(code.contains("_hardy_pool_alloc"), "应生成分配函数");
        assert!(code.contains("_hardy_pool_reset"), "应生成重置函数");
        assert!(code.contains("_hardy_pool_used"), "应生成使用量查询");
        assert!(code.contains("_hardy_pool_free"), "应生成剩余量查询");
        assert!(code.contains("while(1)"), "溢出应 halt");
        assert!(!code.contains("malloc"), "零 malloc");
    }

    #[test]
    fn test_generate_pool_no_panic() {
        let config = PoolConfig { panic_on_overflow: false, ..Default::default() };
        let code = generate_pool_c_code(&config);
        assert!(code.contains("return (void*)0"), "非 panic 模式应返回 null");
        assert!(!code.contains("while(1)"), "不应 halt");
    }

    #[test]
    fn test_memory_layout_report() {
        let analysis = analyze("function f() { return { a: 1, b: 2, c: 3 }; }");
        let config = PoolConfig::default();
        let report = generate_memory_layout_report(&analysis, &config);
        assert!(report.contains("Memory Pool Report"));
        assert!(report.contains("zero malloc"));
        assert!(report.contains("Pool size:"));
    }

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 4), 0);
        assert_eq!(align_up(1, 4), 4);
        assert_eq!(align_up(4, 4), 4);
        assert_eq!(align_up(5, 4), 8);
        assert_eq!(align_up(8, 4), 8);
        assert_eq!(align_up(7, 8), 8);
    }

    #[test]
    fn test_zero_malloc_guarantee() {
        let config = PoolConfig::default();
        let code = generate_pool_c_code(&config);
        assert!(!code.contains("malloc"), "生成的代码不应包含 malloc");
        assert!(!code.contains("calloc"), "生成的代码不应包含 calloc");
        assert!(!code.contains("realloc"), "生成的代码不应包含 realloc");
        // 检查独立的 free() 调用 (排除 _hardy_pool_free)
        let without_pool_free = code.replace("_hardy_pool_free", "");
        assert!(!without_pool_free.contains("free("), "生成的代码不应包含 free");
    }
}
