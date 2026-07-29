//! HardyScript 编译器库
//!
//! JavaScript → COUNPRE64 机器码 / ESP32 C 代码 / Arduino sketch / STM32 HAL / RPi 的完整编译流水线
//! Phase E: STM32 + 树莓派后端 (AST → C → HAL/sysfs 项目)

pub mod lexer;
pub mod parser;
pub mod compiler;
pub mod backend;
pub mod optimizer;
pub mod engine;
pub mod flasher;
pub mod firmware;
pub mod config;
pub mod hardware_api;
pub mod hardware_sim;
pub mod board;
pub mod compat;
pub mod stdlib;
pub mod c_codegen;
pub mod esp32_project;
pub mod arduino_project;
pub mod stm32_project;
pub mod rpi_project;
pub mod model;
pub mod workspace;
pub mod git_ops;
pub mod lint;
pub mod ci;
pub mod async_transform;  // Phase v1.5: async/await 状态机变换

use std::path::Path;

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

/// Compile source into the internal instruction list.
pub fn compile_to_instructions(source: &str) -> Result<Vec<backend::Instruction>, String> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let instructions = compiler::compile(&ast)?;
    Ok(instructions)
}

/// Build firmware bundle (hex + bin) from source and write it to `base_dir/{stem}.*`.
pub fn build_firmware(
    source: &str,
    base_dir: &Path,
    stem: &str,
) -> Result<firmware::FirmwareBundle, String> {
    let instructions = compile_to_instructions(source)?;
    firmware::emit_bundle(&instructions, base_dir, stem)
}

/// Compile source, package firmware, and flash to the specified target.
/// Returns the flasher's output message.
pub fn flash_source(
    source: &str,
    base_dir: &Path,
    stem: &str,
    target: &str,
) -> Result<String, String> {
    let bundle = build_firmware(source, base_dir, stem)?;
    let artifact = flasher::FirmwareArtifact {
        hex_path: bundle.hex_path,
        bin_path: Some(bundle.bin_path),
        verilog_path: None,
        entry_address: bundle.entry_address,
    };
    let target_kind = flasher::TargetKind::from_str(target)
        .ok_or_else(|| format!("Unknown target: {}", target))?;
    let target = flasher::Target { kind: target_kind, port: None };
    let flasher = flasher::create_flasher(&target);
    let result = flasher.flash(&artifact)?;
    if result.success {
        Ok(result.stdout)
    } else {
        Err(format!("{}", result.stderr))
    }
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
