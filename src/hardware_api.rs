//! 硬件API模块 (hardy:hw)
//!
//! 定义统一的硬件API接口规范和各平台的内存映射
//! Phase B: 硬件API层

/// COUNPRE64 硬件内存映射地址
pub mod counpre64_map {
    // UART (串口)
    pub const UART_TX_DATA: i16 = 0x100;     // 发送数据寄存器
    pub const UART_RX_DATA: i16 = 0x101;     // 接收数据寄存器
    pub const UART_STATUS: i16 = 0x102;      // 状态寄存器 (bit0=TX ready, bit1=RX available)
    pub const UART_BAUD: i16 = 0x103;        // 波特率配置

    // GPIO (通用输入输出)
    pub const GPIO_BASE: i16 = 0x200;        // GPIO 基地址
    pub const GPIO_PIN_COUNT: usize = 32;    // GPIO 引脚数量
    pub const GPIO_DIR: i16 = 0x200;         // 方向寄存器 (写入pin号=设为output)
    pub const GPIO_OUT: i16 = 0x201;         // 输出高 (写入pin号=设高)
    pub const GPIO_IN: i16 = 0x202;          // 输入寄存器 (读取位掩码)
    pub const GPIO_PULLUP: i16 = 0x203;      // 上拉使能
    pub const GPIO_INT_EN: i16 = 0x204;      // 中断使能
    pub const GPIO_INT_EDGE: i16 = 0x205;    // 中断边沿 (0=level, 1=edge)
    pub const GPIO_LOW: i16 = 0x206;         // 输出低 (写入pin号=设低)
    pub const GPIO_DIR_IN: i16 = 0x207;      // 设为输入 (写入pin号)

    // Timer (定时器)
    pub const TIMER_BASE: i16 = 0x300;
    pub const TIMER_CTRL: i16 = 0x300;       // 控制寄存器 (bit0=enable, bit1=oneshot)
    pub const TIMER_COUNT: i16 = 0x301;      // 计数值
    pub const TIMER_PERIOD: i16 = 0x302;     // 周期值
    pub const TIMER_INT: i16 = 0x303;        // 中断标志

    // PWM (脉宽调制)
    pub const PWM_BASE: i16 = 0x400;
    pub const PWM_CHANNEL_COUNT: usize = 8;
    pub const PWM_CTRL: i16 = 0x400;         // PWM 控制
    pub const PWM_FREQ: i16 = 0x401;         // 频率
    pub const PWM_DUTY: i16 = 0x402;         // 占空比 (0-255)

    // ADC (模数转换)
    pub const ADC_BASE: i16 = 0x500;
    pub const ADC_CHANNEL_COUNT: usize = 8;
    pub const ADC_CTRL: i16 = 0x500;         // ADC 控制
    pub const ADC_CHANNEL: i16 = 0x501;      // 通道选择
    pub const ADC_VALUE: i16 = 0x502;        // 转换结果 (0-4095)

    // SPI (串行外设)
    pub const SPI_BASE: i16 = 0x600;
    pub const SPI_CTRL: i16 = 0x600;         // SPI 控制
    pub const SPI_DATA: i16 = 0x601;         // SPI 数据
    pub const SPI_STATUS: i16 = 0x602;       // SPI 状态

    // I2C (两线接口)
    pub const I2C_BASE: i16 = 0x700;
    pub const I2C_CTRL: i16 = 0x700;         // I2C 控制
    pub const I2C_ADDR: i16 = 0x701;         // 设备地址
    pub const I2C_DATA: i16 = 0x702;         // I2C 数据
    pub const I2C_STATUS: i16 = 0x703;       // I2C 状态
}

/// 硬件API类型
#[derive(Debug, Clone, PartialEq)]
pub enum HardwareModule {
    /// hardy:hw - 通用硬件API
    Core,
    /// hardy:esp32 - ESP32扩展
    Esp32,
    /// hardy:ai - 锻言AI
    Ai,
    /// hardy:sensor - 传感器
    Sensor,
    /// hardy:motor - 电机/舵机控制
    Motor,
}

impl HardwareModule {
    pub fn from_import_source(source: &str) -> Option<Self> {
        match source {
            "hardy:hw" => Some(Self::Core),
            "hardy:esp32" => Some(Self::Esp32),
            "hardy:ai" => Some(Self::Ai),
            "hardy:sensor" => Some(Self::Sensor),
            "hardy:motor" => Some(Self::Motor),
            _ => None,
        }
    }
}

/// GPIO 方向
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpioDirection {
    Input,
    Output,
}

/// GPIO 上拉模式
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpioPullMode {
    None,
    PullUp,
    PullDown,
}

/// 硬件API调用类型（编译器识别后生成对应指令）
#[derive(Debug, Clone, PartialEq)]
pub enum HardwareCall {
    /// GPIO.output(pin) → 设置引脚为输出
    GpioOutput { pin: u8 },
    /// GPIO.input(pin, mode) → 设置引脚为输入
    GpioInput { pin: u8, pull: GpioPullMode },
    /// led.high() → 输出高电平
    GpioHigh { pin: u8 },
    /// led.low() → 输出低电平
    GpioLow { pin: u8 },
    /// led.toggle() → 翻转
    GpioToggle { pin: u8 },
    /// UART.init(baud)
    UartInit { baud: u32 },
    /// UART.write(str)
    UartWrite { text: String },
    /// Timer.delay(ms)
    TimerDelay { ms: u32 },
    /// Timer.interval(ms, callback)
    TimerInterval { ms: u32 },
    /// PWM.output(pin, freq)
    PwmOutput { pin: u8, freq: u32 },
    /// motor.duty(val)
    PwmDuty { pin: u8, duty: u16 },
    /// ADC.channel(n)
    AdcChannel { channel: u8 },
    /// sensor.read()
    AdcRead { channel: u8 },
    // Phase F: AI API
    /// Brain.init(model_path) — 加载锻言Mini模型
    BrainInit { path: String },
    /// Brain.classify(input, labels) — 分类推理
    BrainClassify { n_labels: u8 },
    // Phase 1.2: Motor/Servo API
    /// Servo.attach(pin, min_us, max_us)
    ServoAttach { pin: u8, min_us: u16, max_us: u16 },
    /// Servo.write(pin, angle) — 0-180度
    ServoWrite { pin: u8, angle: u8 },
    /// Motor.init(in1, in2, ena)
    MotorInit { in1: u8, in2: u8, ena: u8 },
    /// Motor.setSpeed(motor_id, speed) — -255~255
    MotorSetSpeed { motor_id: u8, speed: i16 },
    /// Motor.brake(motor_id)
    MotorBrake { motor_id: u8 },
}

/// 解析硬件API调用（编译器使用）
pub fn resolve_hardware_call(call_name: &str, args: &[i64]) -> Option<HardwareCall> {
    match call_name {
        "GPIO.output" if args.len() >= 1 => Some(HardwareCall::GpioOutput { pin: args[0] as u8 }),
        "GPIO.high" if args.len() >= 1 => Some(HardwareCall::GpioHigh { pin: args[0] as u8 }),
        "GPIO.low" if args.len() >= 1 => Some(HardwareCall::GpioLow { pin: args[0] as u8 }),
        "GPIO.toggle" if args.len() >= 1 => Some(HardwareCall::GpioToggle { pin: args[0] as u8 }),
        "UART.init" if args.len() >= 1 => Some(HardwareCall::UartInit { baud: args[0] as u32 }),
        "Timer.delay" if args.len() >= 1 => Some(HardwareCall::TimerDelay { ms: args[0] as u32 }),
        "PWM.duty" if args.len() >= 2 => Some(HardwareCall::PwmDuty { pin: args[0] as u8, duty: args[1] as u16 }),
        "ADC.read" if args.len() >= 1 => Some(HardwareCall::AdcRead { channel: args[0] as u8 }),
        // Phase 1.2: Servo/Motor
        "Servo.attach" if args.len() >= 1 => {
            let min_us = if args.len() >= 2 { args[1] as u16 } else { 500 };
            let max_us = if args.len() >= 3 { args[2] as u16 } else { 2500 };
            Some(HardwareCall::ServoAttach { pin: args[0] as u8, min_us, max_us })
        }
        "Servo.write" if args.len() >= 2 => Some(HardwareCall::ServoWrite { pin: args[0] as u8, angle: args[1] as u8 }),
        "Motor.init" if args.len() >= 3 => Some(HardwareCall::MotorInit { in1: args[0] as u8, in2: args[1] as u8, ena: args[2] as u8 }),
        "Motor.setSpeed" if args.len() >= 2 => Some(HardwareCall::MotorSetSpeed { motor_id: args[0] as u8, speed: args[1] as i16 }),
        "Motor.brake" if args.len() >= 1 => Some(HardwareCall::MotorBrake { motor_id: args[0] as u8 }),
        _ => None,
    }
}

/// 生成硬件API的 COUNPRE64 机器码片段描述
pub fn hardware_call_to_description(call: &HardwareCall) -> String {
    match call {
        HardwareCall::GpioOutput { pin } => format!("GPIO pin {} → OUTPUT", pin),
        HardwareCall::GpioInput { pin, pull } => format!("GPIO pin {} → INPUT ({:?})", pin, pull),
        HardwareCall::GpioHigh { pin } => format!("GPIO pin {} → HIGH", pin),
        HardwareCall::GpioLow { pin } => format!("GPIO pin {} → LOW", pin),
        HardwareCall::GpioToggle { pin } => format!("GPIO pin {} → TOGGLE", pin),
        HardwareCall::UartInit { baud } => format!("UART init @ {} baud", baud),
        HardwareCall::UartWrite { text } => format!("UART write: {}", text),
        HardwareCall::TimerDelay { ms } => format!("Timer delay {}ms", ms),
        HardwareCall::TimerInterval { ms } => format!("Timer interval {}ms", ms),
        HardwareCall::PwmOutput { pin, freq } => format!("PWM pin {} @ {}Hz", pin, freq),
        HardwareCall::PwmDuty { pin, duty } => format!("PWM pin {} duty={}", pin, duty),
        HardwareCall::AdcChannel { channel } => format!("ADC channel {}", channel),
        HardwareCall::AdcRead { channel } => format!("ADC read channel {}", channel),
        HardwareCall::BrainInit { path } => format!("Brain.init: {}", path),
        HardwareCall::BrainClassify { n_labels } => format!("Brain.classify: {} labels", n_labels),
        HardwareCall::ServoAttach { pin, min_us, max_us } => format!("Servo.attach pin {} ({}-{}us)", pin, min_us, max_us),
        HardwareCall::ServoWrite { pin, angle } => format!("Servo.write pin {} → {}°", pin, angle),
        HardwareCall::MotorInit { in1, in2, ena } => format!("Motor.init in1={} in2={} ena={}", in1, in2, ena),
        HardwareCall::MotorSetSpeed { motor_id, speed } => format!("Motor.setSpeed #{} → {}", motor_id, speed),
        HardwareCall::MotorBrake { motor_id } => format!("Motor.brake #{}", motor_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_module_parse() {
        assert_eq!(HardwareModule::from_import_source("hardy:hw"), Some(HardwareModule::Core));
        assert_eq!(HardwareModule::from_import_source("hardy:esp32"), Some(HardwareModule::Esp32));
        assert_eq!(HardwareModule::from_import_source("hardy:ai"), Some(HardwareModule::Ai));
        assert_eq!(HardwareModule::from_import_source("hardy:motor"), Some(HardwareModule::Motor));
        assert_eq!(HardwareModule::from_import_source("unknown"), None);
    }

    #[test]
    fn test_resolve_gpio_output() {
        let call = resolve_hardware_call("GPIO.output", &[13]);
        assert_eq!(call, Some(HardwareCall::GpioOutput { pin: 13 }));
    }

    #[test]
    fn test_resolve_uart_init() {
        let call = resolve_hardware_call("UART.init", &[9600]);
        assert_eq!(call, Some(HardwareCall::UartInit { baud: 9600 }));
    }

    #[test]
    fn test_memory_map_addresses() {
        assert_eq!(counpre64_map::UART_TX_DATA, 0x100);
        assert_eq!(counpre64_map::GPIO_BASE, 0x200);
        assert_eq!(counpre64_map::TIMER_BASE, 0x300);
        assert_eq!(counpre64_map::PWM_BASE, 0x400);
        assert_eq!(counpre64_map::ADC_BASE, 0x500);
    }

    #[test]
    fn test_resolve_servo_attach() {
        let call = resolve_hardware_call("Servo.attach", &[13, 500, 2500]);
        assert_eq!(call, Some(HardwareCall::ServoAttach { pin: 13, min_us: 500, max_us: 2500 }));
    }

    #[test]
    fn test_resolve_servo_attach_defaults() {
        let call = resolve_hardware_call("Servo.attach", &[12]);
        assert_eq!(call, Some(HardwareCall::ServoAttach { pin: 12, min_us: 500, max_us: 2500 }));
    }

    #[test]
    fn test_resolve_servo_write() {
        let call = resolve_hardware_call("Servo.write", &[13, 90]);
        assert_eq!(call, Some(HardwareCall::ServoWrite { pin: 13, angle: 90 }));
    }

    #[test]
    fn test_resolve_motor_init() {
        let call = resolve_hardware_call("Motor.init", &[2, 3, 5]);
        assert_eq!(call, Some(HardwareCall::MotorInit { in1: 2, in2: 3, ena: 5 }));
    }

    #[test]
    fn test_resolve_motor_set_speed() {
        let call = resolve_hardware_call("Motor.setSpeed", &[0, 200]);
        assert_eq!(call, Some(HardwareCall::MotorSetSpeed { motor_id: 0, speed: 200 }));
    }

    #[test]
    fn test_resolve_motor_brake() {
        let call = resolve_hardware_call("Motor.brake", &[0]);
        assert_eq!(call, Some(HardwareCall::MotorBrake { motor_id: 0 }));
    }
}
