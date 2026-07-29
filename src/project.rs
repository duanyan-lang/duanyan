//! 项目模板生成模块
//!
//! Phase B: `hardy new` 命令
//! 快速创建 HardyScript 项目模板

use std::fs;
use std::path::Path;

/// 项目模板类型
#[derive(Debug, Clone)]
pub enum ProjectTemplate {
    /// 空白项目
    Blank,
    /// LED 闪烁
    Blinky,
    /// 串口输出
    Uart,
    /// GPIO 按键控制
    ButtonGpio,
}

impl ProjectTemplate {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "blank" | "empty" => Some(Self::Blank),
            "blinky" | "blink" | "led" => Some(Self::Blinky),
            "uart" | "serial" => Some(Self::Uart),
            "button" | "gpio" => Some(Self::ButtonGpio),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Blank => "blank",
            Self::Blinky => "blinky",
            Self::Uart => "uart",
            Self::ButtonGpio => "button-gpio",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Blank => "空白项目模板",
            Self::Blinky => "LED 闪烁示例",
            Self::Uart => "串口通信示例",
            Self::ButtonGpio => "GPIO 按键控制示例",
        }
    }
}

/// 创建项目
pub fn create_project(name: &str, template: &ProjectTemplate, target_dir: &Path, board: Option<&str>) -> Result<(), String> {
    let project_dir = target_dir.join(name);

    if project_dir.exists() {
        return Err(format!("目录已存在: {}", project_dir.display()));
    }

    fs::create_dir_all(&project_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    // 推断目标平台
    let target = if let Some(b) = board {
        let bl = b.to_lowercase();
        if bl.starts_with("esp32") || bl.starts_with("esp") {
            "esp32"
        } else if bl.starts_with("arduino") || bl == "uno" || bl == "nano" || bl == "mega" || bl == "due" || bl == "zero" {
            "arduino-avr"
        } else if bl.starts_with("stm32") {
            "stm32"
        } else if bl.starts_with("raspberry") || bl == "rpi" {
            "rpi"
        } else {
            "counpre64"
        }
    } else {
        "counpre64"
    };

    // 创建 hardy.json 配置
    let config = format!(
        r#"{{
    "name": "{}",
    "version": "0.1.0",
    "target": "{}",
    "board": "{}",
    "entry": "src/main.js",
    "build": {{
        "optimize": true,
        "output": "dist/{}.hex"
    }}
}}
"#,
        name, target,
        board.unwrap_or("counpre64-fpga"),
        name
    );
    write_file(&project_dir.join("hardy.json"), &config)?;

    // 创建 .gitignore
    write_file(
        &project_dir.join(".gitignore"),
        "dist/\n*.hex\n*.bin\n*.v\n",
    )?;

    // 创建 src 目录
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|e| format!("创建 src 目录失败: {}", e))?;

    // 根据模板生成代码
    let source = match template {
        ProjectTemplate::Blank => generate_blank_template(),
        ProjectTemplate::Blinky => generate_blinky_template(),
        ProjectTemplate::Uart => generate_uart_template(),
        ProjectTemplate::ButtonGpio => generate_button_template(),
    };

    write_file(&src_dir.join("main.js"), &source)?;

    Ok(())
}

fn write_file(path: &Path, content: &str) -> Result<(), String> {
    fs::write(path, content).map_err(|e| format!("写入文件失败 {}: {}", path.display(), e))
}

fn generate_blank_template() -> String {
    r#"// HardyScript 空白项目
// 在此编写你的代码

let x = 42;

console.log(x);
"#
    .to_string()
}

fn generate_blinky_template() -> String {
    r#"// HardyScript LED 闪烁示例
// 使用 hardy:hw 硬件API

import { GPIO, Timer } from "hardy:hw";

let led = GPIO.output(13);

while (true) {
    led.high();
    Timer.delay(500);
    led.low();
    Timer.delay(500);
}
"#
    .to_string()
}

fn generate_uart_template() -> String {
    r#"// HardyScript 串口通信示例
// 使用 hardy:hw 硬件API

import { UART } from "hardy:hw";

UART.init(115200);
UART.write("Hello from HardyScript!\r\n");

let counter = 0;
while (true) {
    UART.write("Counter: ");
    console.log(counter);
    UART.write("\r\n");
    counter = counter + 1;
}
"#
    .to_string()
}

fn generate_button_template() -> String {
    r#"// HardyScript GPIO 按键控制示例
// 使用 hardy:hw 硬件API

import { GPIO } from "hardy:hw";

let led = GPIO.output(13);
let button = GPIO.input(2, "pullup");

while (true) {
    if (button.read() === 0) {
        led.high();
    } else {
        led.low();
    }
}
"#
    .to_string()
}

/// 列出所有可用模板
pub fn list_templates() {
    println!("📋 可用项目模板:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let templates = [
        ProjectTemplate::Blank,
        ProjectTemplate::Blinky,
        ProjectTemplate::Uart,
        ProjectTemplate::ButtonGpio,
    ];
    for t in &templates {
        println!("  {:<12} {}", t.name(), t.description());
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_template_from_str() {
        assert!(matches!(ProjectTemplate::from_str("blank"), Some(ProjectTemplate::Blank)));
        assert!(matches!(ProjectTemplate::from_str("blinky"), Some(ProjectTemplate::Blinky)));
        assert!(matches!(ProjectTemplate::from_str("uart"), Some(ProjectTemplate::Uart)));
        assert!(matches!(ProjectTemplate::from_str("button"), Some(ProjectTemplate::ButtonGpio)));
        assert!(ProjectTemplate::from_str("unknown").is_none());
    }
}
