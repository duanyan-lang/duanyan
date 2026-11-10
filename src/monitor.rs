//! 串口监视器模块
//!
//! Phase B: `duanyan monitor` 命令
//! 串口监视器，显示 UART 输出

use std::io::{self, Read, Write};

/// 监视器配置
#[derive(Debug, Clone)]
pub struct MonitorConfig {
    pub port: String,
    pub baud_rate: u32,
    pub echo: bool,
    pub hex_mode: bool,
    pub timestamp: bool,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        MonitorConfig {
            port: "COM3".to_string(),
            baud_rate: 115200,
            echo: true,
            hex_mode: false,
            timestamp: false,
        }
    }
}

/// 启动监视器（当前为模拟模式）
pub fn start_monitor(config: &MonitorConfig) -> Result<(), String> {
    println!("📺 DUANYAN 串口监视器");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  端口: {}", config.port);
    println!("  波特率: {}", config.baud_rate);
    println!("  回显: {}", if config.echo { "开启" } else { "关闭" });
    println!("  十六进制: {}", if config.hex_mode { "开启" } else { "关闭" });
    println!("  时间戳: {}", if config.timestamp { "开启" } else { "关闭" });
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  ⚠️  当前为模拟模式 (实际串口通信需要 serialport crate)");
    println!("  按 Ctrl+C 退出");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // 模拟模式 - 只是等待用户输入并回显
    println!("\n[模拟模式: 输入任意内容，将回显显示]\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        stdout.flush().ok();
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {
                if config.echo {
                    if config.timestamp {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default();
                        print!("[{:>6}.{:03}] ", now.as_secs() % 100000, now.subsec_millis());
                    }
                    if config.hex_mode {
                        let hex: Vec<String> = line.bytes().map(|b| format!("{:02X}", b)).collect();
                        println!("HEX: {}", hex.join(" "));
                    } else {
                        print!("{}", line);
                    }
                }
            }
            Err(e) => {
                return Err(format!("读取错误: {}", e));
            }
        }
    }

    Ok(())
}
