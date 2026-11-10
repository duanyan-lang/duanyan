//! 硬件API模块 (duanyan:hw)
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
    /// duanyan:hw - 通用硬件API
    Core,
    /// duanyan:esp32 - ESP32扩展
    Esp32,
    /// duanyan:ai - 锻言AI
    Ai,
    /// duanyan:sensor - 传感器
    Sensor,
    /// duanyan:motor - 电机/舵机控制
    Motor,
    /// duanyan:servo - 关节/舵机 (PCA9685 通道 + ZX361 总线 + 回读, v1.6 P0)
    Servo,
    /// duanyan:robot - 机器人轨迹/运动学 (五次/梯形 + glide, v1.6 P2)
    Robot,
    /// duanyan:modeb - Mode-B 线协议直发 (帧头 AA 55 + crc8 + 'U' 突发, v1.6 P3)
    ModeB,
}

impl HardwareModule {
    pub fn from_import_source(source: &str) -> Option<Self> {
        match source {
            "duanyan:hw" => Some(Self::Core),
            "duanyan:esp32" => Some(Self::Esp32),
            "duanyan:ai" => Some(Self::Ai),
            "duanyan:sensor" => Some(Self::Sensor),
            "duanyan:motor" => Some(Self::Motor),
            "duanyan:servo" => Some(Self::Servo),
            "duanyan:robot" => Some(Self::Robot),
            "duanyan:modeb" => Some(Self::ModeB),
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
    // v1.6 P0: 关节抽象 (对齐 DUANYAN joint_api; 关节号为中心, 后端可混搭)
    /// Joint.bind(joint, backend, chan_id, min, max, home) — backend: 0=pca9685, 1=zx361
    /// (角度包络为整数度; 运行期精钳位由 C helper 承担)
    JointBind { joint: u8, backend: u8, chan_id: u8, min_deg: i16, max_deg: i16, home_deg: i16 },
    /// Joint.write(joint, deg[, ms]) — 目标角/时间多为运行期表达式, 此处仅承载关节号与行程
    JointWrite { joint: u8, ms: u16 },
    /// Joint.read(joint) — 回读 (PWM: hw 脉冲; 总线: 位置/负载/温度)
    JointRead { joint: u8 },
    /// Joint.home() — 全关节归中
    JointHome,
    // v1.6 P1: 执行器安全护栏 (对齐 DUANYAN safety_policy)
    /// Safety.check(load, temp_c, stall_ms) — 优先级 temp>stall>load (温度不可常量化, 只载 load/stall)
    SafetyCheck { load: u16, stall_ms: u32 },
    /// Safety.limits(load_max, temp_max_c, stall_ms) — 运行期限额覆盖
    SafetyLimits { load_max: u16, stall_ms: u32 },
    // v1.6 P2: 机器人轨迹/运动学 (下沉自 robotics.rs 纯数学)
    /// Traj.quintic(q0, qf, T) — 五次多项式轨迹规划 (边界速度/加速度为零)
    TrajQuintic,
    /// Traj.sample(handle, t) — 采样轨迹点 (pos/vel/acc)
    TrajSample,
    /// Traj.trapezoid(distance, v_max, a_max) — 梯形速度规划
    TrajTrapezoid,
    /// Joint.glide(idx, from, to, T, steps) — 五次采样平滑多点移动 (呼应真机 0x23 track)
    JointGlide { joint: u8, steps: u16 },
    // v1.6 P3: Mode-B 线协议直发 (对齐 DUANYAN mode_b_proto; 帧 AA 55 LEN CMD PL CRC8 + 'U' 突发)
    /// ModeB.send(cmd, payload, len) — 原语: 组帧 + crc8 + UART 写 (payload/len 为运行期表达式)
    ModeBSend { cmd: u8 },
    /// ModeB.frame(cmd, len) — 返回值: 组一帧并发送, 回写入总字节数
    ModeBFrame { cmd: u8 },
    /// ModeB.jointSet(joint, ang, sweep_lo, sweep_hi) — 0x20 关节设置 (ang/lo/hi 运行期)
    ModeBJointSet { joint: u8 },
    /// ModeB.armSync(items, n) — 0x30 臂总线同步写 (items 运行期数组, n 项数)
    ModeBArmSync,
    /// ModeB.armHome() — 0x22 四关节归中 (黄金帧字节固定, 全 900)
    ModeBArmHome,
    /// ModeB.heartbeat(uptime) — 0x01 心跳 (uptime 秒, LE 4 字节)
    ModeBHeartbeat,
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
        // v1.6 P0: Joint.*
        "Joint.bind" if args.len() >= 6 => Some(HardwareCall::JointBind {
            joint: args[0] as u8, backend: args[1] as u8, chan_id: args[2] as u8,
            min_deg: args[3] as i16, max_deg: args[4] as i16, home_deg: args[5] as i16,
        }),
        "Joint.write" if args.len() >= 1 => Some(HardwareCall::JointWrite {
            joint: args[0] as u8,
            ms: if args.len() >= 3 { args[2] as u16 } else { 0 },
        }),
        "Joint.read" if args.len() >= 1 => Some(HardwareCall::JointRead { joint: args[0] as u8 }),
        "Joint.home" => Some(HardwareCall::JointHome),
        // v1.6 P1: Safety.*
        "Safety.check" if args.len() >= 3 => Some(HardwareCall::SafetyCheck { load: args[0] as u16, stall_ms: args[2] as u32 }),
        "Safety.limits" if args.len() >= 3 => Some(HardwareCall::SafetyLimits { load_max: args[0] as u16, stall_ms: args[2] as u32 }),
        // v1.6 P2: Traj.* / Joint.glide
        "Traj.quintic" if args.len() >= 3 => Some(HardwareCall::TrajQuintic),
        "Traj.sample" if args.len() >= 2 => Some(HardwareCall::TrajSample),
        "Traj.trapezoid" if args.len() >= 3 => Some(HardwareCall::TrajTrapezoid),
        "Joint.glide" if args.len() >= 5 => Some(HardwareCall::JointGlide { joint: args[0] as u8, steps: args[4] as u16 }),
        // v1.6 P3: ModeB.* (duanyan:modeb)
        "ModeB.send" if args.len() >= 3 => Some(HardwareCall::ModeBSend { cmd: args[0] as u8 }),
        "ModeB.frame" if args.len() >= 3 => Some(HardwareCall::ModeBFrame { cmd: args[0] as u8 }),
        "ModeB.jointSet" if args.len() >= 4 => Some(HardwareCall::ModeBJointSet { joint: args[0] as u8 }),
        "ModeB.armSync" if args.len() >= 2 => Some(HardwareCall::ModeBArmSync),
        "ModeB.armHome" => Some(HardwareCall::ModeBArmHome),
        "ModeB.heartbeat" if args.len() >= 1 => Some(HardwareCall::ModeBHeartbeat),
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
        HardwareCall::JointBind { joint, backend, chan_id, min_deg, max_deg, home_deg } => {
            let be = if *backend == 1 { "zx361" } else { "pca9685" };
            format!("Joint.bind #{} → {} ch/id={} [{},{}] home={}", joint, be, chan_id, min_deg, max_deg, home_deg)
        }
        HardwareCall::JointWrite { joint, ms } => format!("Joint.write #{} ({}ms)", joint, ms),
        HardwareCall::JointRead { joint } => format!("Joint.read #{}", joint),
        HardwareCall::JointHome => "Joint.home (all)".to_string(),
        HardwareCall::SafetyCheck { load, stall_ms } => format!("Safety.check load={} stall={}ms", load, stall_ms),
        HardwareCall::SafetyLimits { load_max, stall_ms } => format!("Safety.limits load_max={} stall={}ms", load_max, stall_ms),
        HardwareCall::TrajQuintic => "Traj.quintic (五次轨迹)".to_string(),
        HardwareCall::TrajSample => "Traj.sample".to_string(),
        HardwareCall::TrajTrapezoid => "Traj.trapezoid".to_string(),
        HardwareCall::JointGlide { joint, steps } => format!("Joint.glide #{} ({} 点平滑)", joint, steps),
        HardwareCall::ModeBSend { cmd } => format!("Mode-B send cmd 0x{:02X} (AA 55 + crc8 + 'U' 突发)", cmd),
        HardwareCall::ModeBFrame { cmd } => format!("Mode-B frame cmd 0x{:02X}", cmd),
        HardwareCall::ModeBJointSet { joint } => format!("Mode-B joint_set #{} (0x20)", joint),
        HardwareCall::ModeBArmSync => "Mode-B arm_sync (0x30 臂总线同步写)".to_string(),
        HardwareCall::ModeBArmHome => "Mode-B arm_home (0x22, 四关节 @ 90°)".to_string(),
        HardwareCall::ModeBHeartbeat => "Mode-B heartbeat (0x01)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_module_parse() {
        assert_eq!(HardwareModule::from_import_source("duanyan:hw"), Some(HardwareModule::Core));
        assert_eq!(HardwareModule::from_import_source("duanyan:esp32"), Some(HardwareModule::Esp32));
        assert_eq!(HardwareModule::from_import_source("duanyan:ai"), Some(HardwareModule::Ai));
        assert_eq!(HardwareModule::from_import_source("duanyan:motor"), Some(HardwareModule::Motor));
        assert_eq!(HardwareModule::from_import_source("duanyan:servo"), Some(HardwareModule::Servo));
        assert_eq!(HardwareModule::from_import_source("duanyan:robot"), Some(HardwareModule::Robot));
        assert_eq!(HardwareModule::from_import_source("duanyan:modeb"), Some(HardwareModule::ModeB));
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

    // v1.6 P0: Joint.* 解析
    #[test]
    fn test_resolve_joint_bind() {
        // Joint.bind(0, 0=pca9685, ch0, 30, 150, 90)
        let call = resolve_hardware_call("Joint.bind", &[0, 0, 0, 30, 150, 90]);
        assert_eq!(call, Some(HardwareCall::JointBind {
            joint: 0, backend: 0, chan_id: 0, min_deg: 30, max_deg: 150, home_deg: 90 }));
        // 参不足不解析
        assert_eq!(resolve_hardware_call("Joint.bind", &[0, 0, 0]), None);
    }

    #[test]
    fn test_resolve_joint_write_read_home() {
        assert_eq!(resolve_hardware_call("Joint.write", &[4, 120, 500]),
                   Some(HardwareCall::JointWrite { joint: 4, ms: 500 }));
        assert_eq!(resolve_hardware_call("Joint.write", &[0, 90]),
                   Some(HardwareCall::JointWrite { joint: 0, ms: 0 }));
        assert_eq!(resolve_hardware_call("Joint.read", &[4]),
                   Some(HardwareCall::JointRead { joint: 4 }));
        assert_eq!(resolve_hardware_call("Joint.home", &[]),
                   Some(HardwareCall::JointHome));
    }

    #[test]
    fn test_joint_description() {
        let d = hardware_call_to_description(&HardwareCall::JointBind {
            joint: 4, backend: 1, chan_id: 2, min_deg: 0, max_deg: 240, home_deg: 120 });
        assert!(d.contains("zx361"));
        assert!(d.contains("#4"));
    }

    // v1.6 P1: Safety.* 解析
    #[test]
    fn test_resolve_safety() {
        assert_eq!(resolve_hardware_call("Safety.check", &[300, 40, 1000]),
                   Some(HardwareCall::SafetyCheck { load: 300, stall_ms: 1000 }));
        assert_eq!(resolve_hardware_call("Safety.limits", &[900, 65, 3000]),
                   Some(HardwareCall::SafetyLimits { load_max: 900, stall_ms: 3000 }));
        assert_eq!(resolve_hardware_call("Safety.check", &[300]), None);
    }

    // v1.6 P2: Traj.* / Joint.glide 解析 + 模块
    #[test]
    fn test_resolve_robot_motion() {
        assert_eq!(HardwareModule::from_import_source("duanyan:robot"), Some(HardwareModule::Robot));
        assert_eq!(resolve_hardware_call("Traj.quintic", &[30, 120, 2]), Some(HardwareCall::TrajQuintic));
        assert_eq!(resolve_hardware_call("Traj.trapezoid", &[100, 50, 100]), Some(HardwareCall::TrajTrapezoid));
        assert_eq!(resolve_hardware_call("Joint.glide", &[0, 30, 120, 2, 20]),
                   Some(HardwareCall::JointGlide { joint: 0, steps: 20 }));
    }

    // v1.6 P3: ModeB.* 解析 + 模块
    #[test]
    fn test_resolve_modeb() {
        assert_eq!(HardwareModule::from_import_source("duanyan:modeb"), Some(HardwareModule::ModeB));
        assert_eq!(resolve_hardware_call("ModeB.send", &[0x12, 0, 0]),
                   Some(HardwareCall::ModeBSend { cmd: 0x12 }));
        assert_eq!(resolve_hardware_call("ModeB.frame", &[0x20, 0, 7]),
                   Some(HardwareCall::ModeBFrame { cmd: 0x20 }));
        assert_eq!(resolve_hardware_call("ModeB.jointSet", &[2, 900, 0, 0]),
                   Some(HardwareCall::ModeBJointSet { joint: 2 }));
        assert_eq!(resolve_hardware_call("ModeB.armSync", &[0, 1]), Some(HardwareCall::ModeBArmSync));
        assert_eq!(resolve_hardware_call("ModeB.armHome", &[]), Some(HardwareCall::ModeBArmHome));
        assert_eq!(resolve_hardware_call("ModeB.heartbeat", &[0]), Some(HardwareCall::ModeBHeartbeat));
        // 参数不足拒绝
        assert_eq!(resolve_hardware_call("ModeB.send", &[0x12]), None);
        assert_eq!(resolve_hardware_call("ModeB.jointSet", &[2, 900]), None);
    }
}
