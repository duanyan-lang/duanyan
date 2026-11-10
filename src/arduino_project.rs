//! Arduino 项目生成器 + 资源限制检查器
//!
//! Phase D: Arduino 后端
//! 生成标准 Arduino sketch 项目结构 (.ino)，供 arduino-cli 编译
//! 同时提供编译期资源估算，在 AVR 等资源紧张平台上提前拦截

use std::fs;
use std::path::{Path, PathBuf};

use crate::c_codegen::CGenerateResult;

/// Arduino 项目路径
pub struct ArduinoProjectPaths {
    pub project_dir: PathBuf,
    pub ino_file: PathBuf,
}

/// 生成 Arduino sketch 项目
pub fn generate_arduino_project(
    c_result: &CGenerateResult,
    base_dir: &Path,
    stem: &str,
) -> Result<ArduinoProjectPaths, String> {
    let sketch_name = sanitize_sketch_name(stem);
    let project_dir = base_dir.join(format!("{}_arduino", sketch_name));
    fs::create_dir_all(&project_dir)
        .map_err(|e| format!("创建 Arduino 项目目录失败: {}", e))?;

    let ino_file = project_dir.join(format!("{}.ino", sketch_name));
    fs::write(&ino_file, &c_result.source)
        .map_err(|e| format!("写入 .ino 文件失败: {}", e))?;

    Ok(ArduinoProjectPaths {
        project_dir,
        ino_file,
    })
}

/// Arduino 板子型号（arduino-cli FQBN 映射）
#[derive(Debug, Clone)]
pub struct ArduinoBoard {
    pub name: &'static str,
    pub fqbn: &'static str,         // Fully Qualified Board Name
    pub max_flash: usize,           // 字节
    pub max_ram: usize,             // 字节
    pub max_gpio: usize,            // 数字引脚数
    pub max_pwm: usize,             // PWM 引脚数
    pub max_adc: usize,             // ADC 通道数
    pub is_arm: bool,
}

/// 已知 Arduino 板子列表
pub fn get_arduino_boards() -> Vec<ArduinoBoard> {
    vec![
        ArduinoBoard {
            name: "arduino-uno",
            fqbn: "arduino:avr:uno",
            max_flash: 32 * 1024,
            max_ram: 2 * 1024,
            max_gpio: 14,
            max_pwm: 6,
            max_adc: 6,
            is_arm: false,
        },
        ArduinoBoard {
            name: "arduino-nano",
            fqbn: "arduino:avr:nano:cpu=atmega328",
            max_flash: 32 * 1024,
            max_ram: 2 * 1024,
            max_gpio: 14,
            max_pwm: 6,
            max_adc: 8,
            is_arm: false,
        },
        ArduinoBoard {
            name: "arduino-mega",
            fqbn: "arduino:avr:mega:cpu=atmega2560",
            max_flash: 256 * 1024,
            max_ram: 8 * 1024,
            max_gpio: 54,
            max_pwm: 15,
            max_adc: 16,
            is_arm: false,
        },
        ArduinoBoard {
            name: "arduino-due",
            fqbn: "arduino:sam:arduino_due_x",
            max_flash: 512 * 1024,
            max_ram: 96 * 1024,
            max_gpio: 54,
            max_pwm: 12,
            max_adc: 12,
            is_arm: true,
        },
        ArduinoBoard {
            name: "arduino-zero",
            fqbn: "arduino:samd:arduino_zero_edbg",
            max_flash: 256 * 1024,
            max_ram: 32 * 1024,
            max_gpio: 20,
            max_pwm: 10,
            max_adc: 6,
            is_arm: true,
        },
    ]
}

/// 根据名称查找 Arduino 板子
pub fn find_arduino_board(name: &str) -> Option<ArduinoBoard> {
    let lower = name.to_lowercase();
    get_arduino_boards().into_iter().find(|b| b.name == lower)
}

/// 根据 CTarget 推断默认板子
pub fn default_board_for_target(is_arm: bool) -> ArduinoBoard {
    let boards = get_arduino_boards();
    if is_arm {
        boards.into_iter().find(|b| b.name == "arduino-due").unwrap()
    } else {
        boards.into_iter().find(|b| b.name == "arduino-uno").unwrap()
    }
}

/// 资源估算结果
#[derive(Debug)]
pub struct ResourceEstimate {
    pub ram_bytes: usize,
    pub flash_bytes: usize,
    pub ram_percent: f64,
    pub flash_percent: f64,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

/// 根据生成的 C 源码估算资源使用量
pub fn estimate_resources(source: &str, board: &ArduinoBoard) -> ResourceEstimate {
    let mut estimate = ResourceEstimate {
        ram_bytes: 0,
        flash_bytes: 0,
        ram_percent: 0.0,
        flash_percent: 0.0,
        warnings: Vec::new(),
        errors: Vec::new(),
    };

    // Flash 估算: C 源码字节数 × 3 (粗略估计，含编译膨胀)
    // 实际 Arduino 编译后 .text + .data 段大小
    let source_bytes = source.len();
    // helpers 函数约 200 字节, 加上源码每行约 5-15 字节机器码
    let code_lines = source.lines().count();
    estimate.flash_bytes = (code_lines * 12).max(source_bytes / 2) + 200;

    // RAM 估算: 全局变量 + 栈
    let mut global_vars = 0usize;
    let mut string_bytes = 0usize;
    for line in source.lines() {
        let trimmed = line.trim();
        // 计算全局变量 (int32_t x = ...)
        if trimmed.starts_with("int32_t ") || trimmed.starts_with("const int32_t ") {
            global_vars += 4;
        }
        // 字符串字面量
        if let Some(start) = trimmed.find('"') {
            if let Some(end) = trimmed.rfind('"') {
                if end > start {
                    string_bytes += end - start - 1;
                }
            }
        }
    }
    // 栈空间: 每层函数约 32 字节，Arduino 默认栈 256-512 字节
    let stack_estimate = 256usize;
    estimate.ram_bytes = global_vars + string_bytes + stack_estimate;

    // 百分比
    estimate.ram_percent = (estimate.ram_bytes as f64 / board.max_ram as f64) * 100.0;
    estimate.flash_percent = (estimate.flash_bytes as f64 / board.max_flash as f64) * 100.0;

    // 警告
    if estimate.ram_percent > 75.0 {
        estimate.warnings.push(format!(
            "⚠️  RAM 使用预估 {:.0}% ({}/{}) — 接近上限，建议升级板子",
            estimate.ram_percent, estimate.ram_bytes, board.max_ram
        ));
    }
    if estimate.flash_percent > 75.0 {
        estimate.warnings.push(format!(
            "⚠️  Flash 使用预估 {:.0}% ({}/{}) — 接近上限",
            estimate.flash_percent, estimate.flash_bytes, board.max_flash
        ));
    }
    if !board.is_arm && string_bytes > 256 {
        estimate.warnings.push(
            "⚠️  AVR 板子字符串字面量较多，考虑使用 PROGMEM 宏优化".to_string()
        );
    }

    // 错误
    if estimate.ram_bytes > board.max_ram {
        estimate.errors.push(format!(
            "❌ RAM 预估超限: {} > {} bytes ({}). 建议升级到更大的板子 (如 Arduino Mega/Due)",
            estimate.ram_bytes, board.max_ram, board.name
        ));
    }
    if estimate.flash_bytes > board.max_flash {
        estimate.errors.push(format!(
            "❌ Flash 预估超限: {} > {} bytes ({}). 建议升级到更大的板子",
            estimate.flash_bytes, board.max_flash, board.name
        ));
    }

    estimate
}

/// 清理 sketch 名称，确保 Arduino CLI 兼容
fn sanitize_sketch_name(name: &str) -> String {
    let sanitized: String = name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if sanitized.is_empty() { "duanyan_sketch".to_string() } else { sanitized }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_codegen::{generate_c_source, CTarget};
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn make_c_result(source: &str) -> CGenerateResult {
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        generate_c_source(&ast, &CTarget::ArduinoAvr).unwrap()
    }

    #[test]
    fn test_sanitize_sketch_name() {
        assert_eq!(sanitize_sketch_name("my-sketch"), "my_sketch");
        assert_eq!(sanitize_sketch_name("hello.js"), "hello_js");
        assert_eq!(sanitize_sketch_name(""), "duanyan_sketch");
        assert_eq!(sanitize_sketch_name("blink"), "blink");
    }

    #[test]
    fn test_arduino_board_lookup() {
        let uno = find_arduino_board("arduino-uno").unwrap();
        assert_eq!(uno.fqbn, "arduino:avr:uno");
        assert_eq!(uno.max_flash, 32 * 1024);
        assert!(!uno.is_arm);

        let due = find_arduino_board("arduino-due").unwrap();
        assert!(due.is_arm);
        assert_eq!(due.max_ram, 96 * 1024);
    }

    #[test]
    fn test_resource_estimate_small_program() {
        let board = find_arduino_board("arduino-uno").unwrap();
        let source = "int32_t x = 42;";
        let est = estimate_resources(source, &board);
        assert!(est.flash_bytes < board.max_flash);
        assert!(est.ram_bytes < board.max_ram);
        assert!(est.errors.is_empty());
    }

    #[test]
    fn test_default_board_avr() {
        let board = default_board_for_target(false);
        assert_eq!(board.name, "arduino-uno");
    }

    #[test]
    fn test_default_board_arm() {
        let board = default_board_for_target(true);
        assert_eq!(board.name, "arduino-due");
    }
}
