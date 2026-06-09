//! SNARjs 编译器库
//!
//! JavaScript → COUNPRE64 机器码的完整编译流水线
//! Phase 4: 添加优化器

pub mod lexer;
pub mod parser;
pub mod compiler;
pub mod backend;
pub mod optimizer;

/// 编译 JavaScript 源码为 COUNPRE64 机器码
pub fn compile_source(source: &str) -> Result<Vec<u32>, String> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let instructions = compiler::compile(&ast)?;
    let machine_code: Vec<u32> = instructions.iter().map(|i| i.encode()).collect();
    Ok(machine_code)
}

/// 编译 JavaScript 源码为 COUNPRE64 机器码（带优化）
pub fn compile_source_optimized(source: &str) -> Result<Vec<u32>, String> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let optimized_ast = optimizer::optimize(&ast);
    let instructions = compiler::compile(&optimized_ast)?;
    let machine_code: Vec<u32> = instructions.iter().map(|i| i.encode()).collect();
    Ok(machine_code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_compile() {
        let source = "let a = 42;";
        let result = compile_source(source);
        assert!(result.is_ok());
        let code = result.unwrap();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_optimized_compile() {
        let source = "let a = 3 + 4;";
        let result = compile_source_optimized(source);
        assert!(result.is_ok());
        let code = result.unwrap();
        // 优化后只需 ADDI x9, x0, 7 + halt = 2 条指令
        assert_eq!(code.len(), 2, "常量折叠后应只有 2 条指令");
    }

    #[test]
    fn test_console_log_compile() {
        let source = "console.log(100);";
        let result = compile_source(source);
        assert!(result.is_ok());
    }
}
