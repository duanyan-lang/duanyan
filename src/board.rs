//! 开发板检测与管理模块
//!
//! Phase B: `hardy board list/info` 命令
//! USB串口自动检测板子类型

use std::fmt;

/// 支持的硬件目标
#[derive(Debug, Clone)]
pub struct BoardInfo {
    pub name: String,
    pub description: String,
    pub mcu: String,
    pub frequency_mhz: u32,
    pub ram_kb: u32,
    pub flash_kb: u32,
    pub gpio_count: u32,
    pub uart_count: u32,
    pub adc_channels: u32,
    pub pwm_channels: u32,
    pub spi_count: u32,
    pub i2c_count: u32,
    pub usb_pid: Option<u16>,
    pub usb_vid: Option<u16>,
}

impl fmt::Display for BoardInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  📋 {}", self.name)?;
        writeln!(f, "     {}", self.description)?;
        writeln!(f, "     MCU: {} @ {}MHz", self.mcu, self.frequency_mhz)?;
        writeln!(f, "     RAM: {}KB | Flash: {}KB", self.ram_kb, self.flash_kb)?;
        writeln!(f, "     GPIO: {} | UART: {} | ADC: {} | PWM: {}",
            self.gpio_count, self.uart_count, self.adc_channels, self.pwm_channels)?;
        writeln!(f, "     SPI: {} | I2C: {}", self.spi_count, self.i2c_count)
    }
}

/// 获取所有已知开发板
pub fn get_known_boards() -> Vec<BoardInfo> {
    vec![
        BoardInfo {
            name: "COUNPRE64-FPGA".to_string(),
            description: "Gowin GW1NSR-4C FPGA 开发板 (HardyScript 原生目标)".to_string(),
            mcu: "COUNPRE64 (Gowin GW1NSR-4C)".to_string(),
            frequency_mhz: 27,
            ram_kb: 32,
            flash_kb: 64,
            gpio_count: 32,
            uart_count: 1,
            adc_channels: 0,
            pwm_channels: 8,
            spi_count: 1,
            i2c_count: 1,
            usb_pid: None,
            usb_vid: None,
        },
        BoardInfo {
            name: "ESP32-DevKit".to_string(),
            description: "ESP32 通用开发板 (v1.0 最高优先级)".to_string(),
            mcu: "ESP32-WROOM-32".to_string(),
            frequency_mhz: 240,
            ram_kb: 520,
            flash_kb: 4096,
            gpio_count: 34,
            uart_count: 3,
            adc_channels: 18,
            pwm_channels: 16,
            spi_count: 4,
            i2c_count: 2,
            usb_pid: Some(0xEA60),
            usb_vid: Some(0x10C4),
        },
        BoardInfo {
            name: "ESP32-S3-DevKit".to_string(),
            description: "ESP32-S3 开发板 (带PSRAM, AI能力, v1.0 最高优先级)".to_string(),
            mcu: "ESP32-S3-WROOM-1".to_string(),
            frequency_mhz: 240,
            ram_kb: 512,
            flash_kb: 8192,
            gpio_count: 45,
            uart_count: 3,
            adc_channels: 20,
            pwm_channels: 16,
            spi_count: 4,
            i2c_count: 2,
            usb_pid: Some(0x1001),
            usb_vid: Some(0x303A),
        },
        BoardInfo {
            name: "STM32-BluePill".to_string(),
            description: "STM32F103C8T6 最小系统 (Phase E)".to_string(),
            mcu: "STM32F103C8T6".to_string(),
            frequency_mhz: 72,
            ram_kb: 20,
            flash_kb: 64,
            gpio_count: 37,
            uart_count: 3,
            adc_channels: 10,
            pwm_channels: 15,
            spi_count: 2,
            i2c_count: 2,
            usb_pid: Some(0x5740),
            usb_vid: Some(0x0483),
        },
        BoardInfo {
            name: "Raspberry Pi Pico".to_string(),
            description: "RP2040 开发板 (Phase E)".to_string(),
            mcu: "RP2040".to_string(),
            frequency_mhz: 133,
            ram_kb: 264,
            flash_kb: 2048,
            gpio_count: 26,
            uart_count: 2,
            adc_channels: 4,
            pwm_channels: 16,
            spi_count: 2,
            i2c_count: 2,
            usb_pid: Some(0x000A),
            usb_vid: Some(0x2E8A),
        },
        BoardInfo {
            name: "Arduino Uno".to_string(),
            description: "ATmega328P 开发板 (v1.0 最高优先级, 全球最普及)".to_string(),
            mcu: "ATmega328P".to_string(),
            frequency_mhz: 16,
            ram_kb: 2,
            flash_kb: 32,
            gpio_count: 14,
            uart_count: 1,
            adc_channels: 6,
            pwm_channels: 6,
            spi_count: 1,
            i2c_count: 1,
            usb_pid: Some(0x0043),
            usb_vid: Some(0x2341),
        },
        BoardInfo {
            name: "Arduino Nano".to_string(),
            description: "ATmega328P 最小系统板 (v1.0)".to_string(),
            mcu: "ATmega328P".to_string(),
            frequency_mhz: 16,
            ram_kb: 2,
            flash_kb: 32,
            gpio_count: 14,
            uart_count: 1,
            adc_channels: 8,
            pwm_channels: 6,
            spi_count: 1,
            i2c_count: 1,
            usb_pid: Some(0x6001),
            usb_vid: Some(0x0403),
        },
        BoardInfo {
            name: "Arduino Mega".to_string(),
            description: "ATmega2560 开发板 (v1.0, basic+ tier)".to_string(),
            mcu: "ATmega2560".to_string(),
            frequency_mhz: 16,
            ram_kb: 8,
            flash_kb: 256,
            gpio_count: 54,
            uart_count: 4,
            adc_channels: 16,
            pwm_channels: 15,
            spi_count: 1,
            i2c_count: 1,
            usb_pid: Some(0x0042),
            usb_vid: Some(0x2341),
        },
        BoardInfo {
            name: "Arduino Due".to_string(),
            description: "SAM3X8E ARM Cortex-M3 (v1.0, standard tier)".to_string(),
            mcu: "SAM3X8E".to_string(),
            frequency_mhz: 84,
            ram_kb: 96,
            flash_kb: 512,
            gpio_count: 54,
            uart_count: 4,
            adc_channels: 12,
            pwm_channels: 12,
            spi_count: 1,
            i2c_count: 2,
            usb_pid: Some(0x003D),
            usb_vid: Some(0x2341),
        },
        BoardInfo {
            name: "Arduino Zero".to_string(),
            description: "SAMD21 ARM Cortex-M0+ (v1.0, standard tier)".to_string(),
            mcu: "SAMD21G18A".to_string(),
            frequency_mhz: 48,
            ram_kb: 32,
            flash_kb: 256,
            gpio_count: 20,
            uart_count: 1,
            adc_channels: 6,
            pwm_channels: 10,
            spi_count: 1,
            i2c_count: 1,
            usb_pid: Some(0x804D),
            usb_vid: Some(0x2341),
        },
        BoardInfo {
            name: "Arduino Nano 33 BLE".to_string(),
            description: "nRF52840 ARM Cortex-M4F (ai tier, 可跑锻言Mini)".to_string(),
            mcu: "nRF52840".to_string(),
            frequency_mhz: 64,
            ram_kb: 256,
            flash_kb: 1024,
            gpio_count: 14,
            uart_count: 1,
            adc_channels: 8,
            pwm_channels: 6,
            spi_count: 1,
            i2c_count: 1,
            usb_pid: Some(0x005A),
            usb_vid: Some(0x2341),
        },
        BoardInfo {
            name: "Arduino Portenta H7".to_string(),
            description: "STM32H747XI 双核 ARM Cortex-M7/M4 (ai tier, 旗舰全功能)".to_string(),
            mcu: "STM32H747XI".to_string(),
            frequency_mhz: 480,
            ram_kb: 1024,
            flash_kb: 2048,
            gpio_count: 36,
            uart_count: 4,
            adc_channels: 16,
            pwm_channels: 12,
            spi_count: 3,
            i2c_count: 3,
            usb_pid: Some(0x025B),
            usb_vid: Some(0x2341),
        },
        BoardInfo {
            name: "Raspberry Pi 4B".to_string(),
            description: "BCM2711 ARM Cortex-A72 (Phase E)".to_string(),
            mcu: "BCM2711".to_string(),
            frequency_mhz: 1500,
            ram_kb: 2 * 1024 * 1024,
            flash_kb: 0, // SD card
            gpio_count: 26,
            uart_count: 6,
            adc_channels: 0,
            pwm_channels: 2,
            spi_count: 2,
            i2c_count: 2,
            usb_pid: None,
            usb_vid: None,
        },
    ]
}

/// 扫描系统上连接的串口设备
pub fn scan_serial_ports() -> Vec<SerialPortInfo> {
    let mut ports = Vec::new();

    // Windows: 检查 COM1-COM20
    #[cfg(target_os = "windows")]
    {
        for i in 1..=20 {
            let port_name = format!("COM{}", i);
            // 简单检查端口是否存在（实际应使用 serialport crate）
            let path = format!("\\\\.\\{}", port_name);
            if std::path::Path::new(&path).exists() {
                ports.push(SerialPortInfo {
                    name: port_name,
                    description: "串口设备".to_string(),
                    board_type: None,
                });
            }
        }
    }

    // Linux: 检查 /dev/ttyUSB* /dev/ttyACM*
    #[cfg(target_os = "linux")]
    {
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("ttyUSB") || name.starts_with("ttyACM") {
                    ports.push(SerialPortInfo {
                        name: format!("/dev/{}", name),
                        description: "串口设备".to_string(),
                        board_type: None,
                    });
                }
            }
        }
    }

    ports
}

#[derive(Debug, Clone)]
pub struct SerialPortInfo {
    pub name: String,
    pub description: String,
    pub board_type: Option<String>,
}

/// 列出开发板命令
pub fn cmd_board_list() {
    println!("🔌 已知开发板:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    for board in get_known_boards() {
        print!("{}", board);
        println!();
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    println!("\n🔍 已检测串口:");
    let ports = scan_serial_ports();
    if ports.is_empty() {
        println!("  (未检测到串口设备)");
    } else {
        for port in &ports {
            let board = port.board_type.as_deref().unwrap_or("未知设备");
            println!("  {} - {} [{}]", port.name, port.description, board);
        }
    }
}

/// 设置默认开发板
pub fn cmd_board_set(board_name: &str) {
    let boards = get_known_boards();
    let name_lower = board_name.to_lowercase();

    for board in &boards {
        if board.name.to_lowercase().contains(&name_lower) || name_lower.contains(&board.name.to_lowercase()) {
            println!("✅ 默认开发板已设置: {}", board.name);
            println!("   {}", board.description);
            println!();
            println!("   💡 下次 'hardy build' 将自动使用此板子作为目标");
            println!("   💡 配置文件: .hardy/board (当前目录)");
            // 写入 .hardy/board 文件
            let hardy_dir = std::path::Path::new(".hardy");
            if !hardy_dir.exists() {
                let _ = std::fs::create_dir_all(hardy_dir);
            }
            let _ = std::fs::write(hardy_dir.join("board"), &board.name);
            return;
        }
    }

    eprintln!("❌ 未找到开发板: {}", board_name);
    eprintln!("已知开发板:");
    for board in &boards {
        eprintln!("  - {}", board.name);
    }
}

/// 查看开发板详情
pub fn cmd_board_info(board_name: &str) {
    let boards = get_known_boards();
    let name_lower = board_name.to_lowercase();

    for board in &boards {
        if board.name.to_lowercase().contains(&name_lower) || name_lower.contains(&board.name.to_lowercase()) {
            println!("📋 开发板详情: {}", board.name);
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            print!("{}", board);
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            return;
        }
    }

    eprintln!("❌ 未找到开发板: {}", board_name);
    eprintln!("已知开发板:");
    for board in &boards {
        eprintln!("  - {}", board.name);
    }
}
