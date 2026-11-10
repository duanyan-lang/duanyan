//! Phase W3: 汽车通信确定性
//!
//! 面向 ECU 级确定性通信：
//! - CAN 消息调度表生成 (TTCAN: Time-Triggered CAN)
//! - CAN 帧优先级仲裁分析
//! - 端到端链路延迟证明 (sensor → compute → actuator)
//! - 看门狗集成
//! - 故障安全状态机

// =============================================================================
// CAN 消息描述
// =============================================================================

/// CAN 消息定义
#[derive(Debug, Clone)]
pub struct CanMessage {
    /// 消息名称
    pub name: String,
    /// CAN ID (11-bit standard 或 29-bit extended)
    pub can_id: u32,
    /// 是否扩展帧
    pub extended: bool,
    /// 数据长度 (bytes, 0-8)
    pub dlc: u8,
    /// 发送周期 (μs), 0 = 事件触发
    pub period_us: u64,
    /// 发送任务名
    pub tx_task: String,
    /// 信号列表
    pub signals: Vec<CanSignal>,
    /// 截止期 (μs, 从触发到总线发送完成)
    pub deadline_us: u64,
}

/// CAN 信号 (消息内的字段)
#[derive(Debug, Clone)]
pub struct CanSignal {
    pub name: String,
    /// 起始位
    pub start_bit: u8,
    /// 位长度
    pub bit_length: u8,
    /// 缩放因子
    pub factor: f64,
    /// 偏移
    pub offset: f64,
    /// 物理单位
    pub unit: String,
}

impl CanMessage {
    pub fn new(name: &str, can_id: u32, period_us: u64, tx_task: &str) -> Self {
        Self {
            name: name.to_string(),
            can_id,
            extended: can_id > 0x7FF,
            dlc: 8,
            period_us,
            tx_task: tx_task.to_string(),
            signals: Vec::new(),
            deadline_us: period_us / 2,
        }
    }

    pub fn with_dlc(mut self, dlc: u8) -> Self {
        self.dlc = dlc.min(8);
        self
    }

    pub fn with_signal(mut self, name: &str, start_bit: u8, bit_length: u8) -> Self {
        self.signals.push(CanSignal {
            name: name.to_string(),
            start_bit,
            bit_length,
            factor: 1.0,
            offset: 0.0,
            unit: String::new(),
        });
        self
    }

    /// CAN 帧优先级 (ID 越小优先级越高)
    pub fn priority(&self) -> u32 {
        self.can_id
    }

    /// 帧传输时间 (μs, 500kbps CAN)
    pub fn transmission_time_us(&self, baudrate_kbps: u32) -> f64 {
        // 标准帧: 47 + 8*DLC bits, 扩展帧: 67 + 8*DLC bits
        let bits = if self.extended { 67 } else { 47 } + 8 * self.dlc as u32;
        // 加上 stuff bits (最坏 +15%)
        let worst_bits = (bits as f64 * 1.15) as u32;
        worst_bits as f64 / baudrate_kbps as f64 * 1000.0
    }
}

// =============================================================================
// TTCAN 调度表
// =============================================================================

/// TTCAN 调度表条目
#[derive(Debug, Clone)]
pub struct TtcanSlot {
    /// 时间槽编号
    pub slot_id: usize,
    /// 槽起始时间 (μs, 相对周期起点)
    pub offset_us: u64,
    /// 消息名
    pub message_name: String,
    /// CAN ID
    pub can_id: u32,
    /// 传输时间 (μs)
    pub tx_time_us: f64,
    /// 槽长度 (μs, 含保护间隔)
    pub slot_length_us: u64,
}

/// TTCAN 调度表
#[derive(Debug, Clone)]
pub struct TtcanSchedule {
    /// 基本周期 (μs, 所有消息周期的 LCM)
    pub base_cycle_us: u64,
    /// 时间槽列表
    pub slots: Vec<TtcanSlot>,
    /// 总线利用率
    pub bus_utilization: f64,
    /// 是否可行 (无冲突)
    pub feasible: bool,
    /// 最大抖动 (μs)
    pub max_jitter_us: f64,
}

/// 生成 TTCAN 调度表
pub fn generate_ttcan_schedule(messages: &[CanMessage], baudrate_kbps: u32) -> TtcanSchedule {
    if messages.is_empty() {
        return TtcanSchedule {
            base_cycle_us: 0,
            slots: vec![],
            bus_utilization: 0.0,
            feasible: true,
            max_jitter_us: 0.0,
        };
    }

    // 计算基本周期 (所有周期的 LCM)
    let base_cycle = messages.iter()
        .filter(|m| m.period_us > 0)
        .map(|m| m.period_us)
        .fold(1u64, |acc, p| lcm(acc, p));

    // 按 CAN ID 优先级排序 (ID 小 = 优先级高)
    let mut sorted: Vec<&CanMessage> = messages.iter()
        .filter(|m| m.period_us > 0)
        .collect();
    sorted.sort_by_key(|m| m.can_id);

    // 贪心分配时间槽
    let mut slots = Vec::new();
    let mut current_offset: u64 = 0;
    let guard_band: u64 = 5; // 5μs 保护间隔

    for msg in &sorted {
        let tx_time = msg.transmission_time_us(baudrate_kbps);
        let slot_len = (tx_time.ceil() as u64) + guard_band;

        slots.push(TtcanSlot {
            slot_id: slots.len(),
            offset_us: current_offset,
            message_name: msg.name.clone(),
            can_id: msg.can_id,
            tx_time_us: tx_time,
            slot_length_us: slot_len,
        });

        current_offset += slot_len;
    }

    // 计算总线利用率
    let total_tx: f64 = slots.iter().map(|s| s.tx_time_us).sum();
    let utilization = if base_cycle > 0 {
        total_tx / base_cycle as f64
    } else {
        0.0
    };

    let feasible = current_offset < base_cycle;
    let max_jitter = if feasible { guard_band as f64 } else { f64::MAX };

    TtcanSchedule {
        base_cycle_us: base_cycle,
        slots,
        bus_utilization: utilization,
        feasible,
        max_jitter_us: max_jitter,
    }
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn lcm(a: u64, b: u64) -> u64 {
    if a == 0 || b == 0 { return 0; }
    a / gcd(a, b) * b
}

// =============================================================================
// 端到端延迟证明
// =============================================================================

/// 链路节点
#[derive(Debug, Clone)]
pub struct LinkNode {
    /// 节点名称
    pub name: String,
    /// 节点类型
    pub kind: LinkNodeKind,
    /// 延迟 (μs)
    pub latency_us: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinkNodeKind {
    /// 传感器采样
    SensorAcquire,
    /// CAN 传输
    CanTransmit,
    /// 计算/控制
    Compute,
    /// CAN 接收
    CanReceive,
    /// 执行器输出
    ActuatorOutput,
}

/// 端到端链路
#[derive(Debug, Clone)]
pub struct EndToEndLink {
    /// 链路名称 (如 "转向控制链路")
    pub name: String,
    /// 节点列表 (按数据流顺序)
    pub nodes: Vec<LinkNode>,
    /// 截止期 (μs)
    pub deadline_us: f64,
}

/// 端到端分析结果
#[derive(Debug, Clone)]
pub struct E2eAnalysis {
    /// 链路名
    pub link_name: String,
    /// 各节点延迟
    pub node_latencies: Vec<(String, f64)>,
    /// 总延迟 (μs)
    pub total_latency_us: f64,
    /// 截止期 (μs)
    pub deadline_us: f64,
    /// 余量 (μs)
    pub margin_us: f64,
    /// 是否满足
    pub met: bool,
    /// 证明文本
    pub proof: String,
}

/// 执行端到端延迟分析
pub fn analyze_e2e_latency(link: &EndToEndLink) -> E2eAnalysis {
    let total: f64 = link.nodes.iter().map(|n| n.latency_us).sum();
    let margin = link.deadline_us - total;
    let met = margin >= 0.0;

    let proof = format!(
        "E2E PROOF [{}]: {} = {:.2}μs {} deadline {:.2}μs → {}",
        link.name,
        link.nodes.iter().map(|n| format!("{:.1}", n.latency_us)).collect::<Vec<_>>().join(" + "),
        total,
        if met { "<=" } else { ">" },
        link.deadline_us,
        if met { "SATISFIED" } else { "VIOLATED" },
    );

    E2eAnalysis {
        link_name: link.name.clone(),
        node_latencies: link.nodes.iter().map(|n| (n.name.clone(), n.latency_us)).collect(),
        total_latency_us: total,
        deadline_us: link.deadline_us,
        margin_us: margin,
        met,
        proof,
    }
}

// =============================================================================
// 看门狗集成
// =============================================================================

/// 看门狗配置
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// 超时 (ms)
    pub timeout_ms: u32,
    /// 喂狗任务名
    pub feeder_task: String,
    /// 喂狗周期 (ms, 必须 < timeout)
    pub feed_period_ms: u32,
    /// 超时后动作
    pub timeout_action: WatchdogAction,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WatchdogAction {
    /// 系统复位
    SystemReset,
    /// 进入安全状态
    SafeState,
    /// 触发中断
    Interrupt,
}

/// 生成看门狗 C 代码
pub fn generate_watchdog_code(config: &WatchdogConfig, target: &crate::c_codegen::CTarget) -> String {
    let mut code = String::new();
    code.push_str("/* DUANYAN W3: Watchdog Integration */\n");

    match target {
        crate::c_codegen::CTarget::Esp32 => {
            code.push_str("#include \"esp_task_wdt.h\"\n\n");
            code.push_str(&format!(
                "static inline void _duanyan_wdt_init(void) {{\n    esp_task_wdt_init({}, true);\n    esp_task_wdt_add(NULL);\n}}\n\n",
                config.timeout_ms / 1000
            ));
            code.push_str("static inline void _duanyan_wdt_feed(void) {\n    esp_task_wdt_reset();\n}\n\n");
        }
        crate::c_codegen::CTarget::Stm32 => {
            code.push_str("/* STM32 IWDG */\n");
            code.push_str(&format!(
                "static inline void _duanyan_wdt_init(void) {{\n    IWDG->KR = 0x5555;\n    IWDG->PR = 4; /* /64 */\n    IWDG->RLR = {};\n    IWDG->KR = 0xCCCC; /* start */\n}}\n\n",
                config.timeout_ms * 40 / 1000 // 40kHz LSI / 64
            ));
            code.push_str("static inline void _duanyan_wdt_feed(void) {\n    IWDG->KR = 0xAAAA;\n}\n\n");
        }
        _ => {
            code.push_str("/* Generic watchdog (software) */\n");
            code.push_str(&format!(
                "static volatile uint32_t _duanyan_wdt_counter = 0;\nstatic const uint32_t _DUANYAN_WDT_LIMIT = {};\n\n",
                config.timeout_ms / config.feed_period_ms
            ));
            code.push_str("static inline void _duanyan_wdt_feed(void) {\n    _duanyan_wdt_counter = 0;\n}\n\n");
        }
    }

    code
}

// =============================================================================
// 故障安全状态机
// =============================================================================

/// 安全状态定义
#[derive(Debug, Clone)]
pub struct SafeStateConfig {
    /// 系统名称
    pub system_name: String,
    /// 安全输出值 (GPIO/PWM/DAC)
    pub safe_outputs: Vec<SafeOutput>,
    /// 触发条件
    pub triggers: Vec<FaultTrigger>,
    /// 恢复条件 (None = 需手动复位)
    pub auto_recover: bool,
}

#[derive(Debug, Clone)]
pub struct SafeOutput {
    pub name: String,
    pub kind: SafeOutputKind,
    pub safe_value: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SafeOutputKind {
    GpioLow,
    GpioHigh,
    PwmDuty,
    DacValue,
    MotorStop,
}

#[derive(Debug, Clone)]
pub struct FaultTrigger {
    pub name: String,
    pub kind: FaultKind,
    pub threshold: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FaultKind {
    /// WCET 超时
    WcetTimeout,
    /// 通信丢失
    CommLoss,
    /// 传感器异常
    SensorFault,
    /// 看门狗超时
    WatchdogTimeout,
    /// 电压异常
    VoltageFault,
}

/// 生成故障安全状态机 C 代码
pub fn generate_safestate_code(config: &SafeStateConfig) -> String {
    let mut code = String::new();
    code.push_str(&format!("/* DUANYAN W3: Fail-Safe State Machine — {} */\n\n", config.system_name));

    code.push_str("typedef enum {\n    SYS_NORMAL = 0,\n    SYS_FAULT_DETECTED,\n    SYS_SAFE_STATE,\n} _duanyan_sys_state_t;\n\n");
    code.push_str("static volatile _duanyan_sys_state_t _duanyan_state = SYS_NORMAL;\n\n");

    // 进入安全状态函数
    code.push_str("static inline void _duanyan_enter_safe_state(void) {\n");
    code.push_str("    _duanyan_state = SYS_SAFE_STATE;\n");
    for out in &config.safe_outputs {
        match out.kind {
            SafeOutputKind::GpioLow => code.push_str(&format!("    /* {} → LOW */\n", out.name)),
            SafeOutputKind::GpioHigh => code.push_str(&format!("    /* {} → HIGH */\n", out.name)),
            SafeOutputKind::PwmDuty => code.push_str(&format!("    /* {} → duty {}% */\n", out.name, out.safe_value)),
            SafeOutputKind::DacValue => code.push_str(&format!("    /* {} → DAC {} */\n", out.name, out.safe_value)),
            SafeOutputKind::MotorStop => code.push_str(&format!("    /* {} → STOP */\n", out.name)),
        }
    }
    code.push_str("}\n\n");

    // 故障检测函数
    code.push_str("static inline void _duanyan_check_faults(void) {\n");
    code.push_str("    if (_duanyan_state != SYS_NORMAL) return;\n");
    for trigger in &config.triggers {
        code.push_str(&format!("    /* Check: {} ({:?}) */\n", trigger.name, trigger.kind));
    }
    code.push_str("}\n\n");

    code
}

// =============================================================================
// CAN 调度报告
// =============================================================================

/// 生成 CAN 调度报告
pub fn generate_can_report(schedule: &TtcanSchedule, baudrate: u32) -> String {
    let mut r = String::new();
    r.push_str("=== DUANYAN CAN Schedule (Phase W3) ===\n");
    r.push_str(&format!("Baudrate: {} kbps\n", baudrate));
    r.push_str(&format!("Base cycle: {} μs\n", schedule.base_cycle_us));
    r.push_str(&format!("Bus utilization: {:.1}%\n", schedule.bus_utilization * 100.0));
    r.push_str(&format!("Feasible: {}\n", if schedule.feasible { "YES" } else { "NO" }));
    r.push_str(&format!("Max jitter: {:.1} μs\n\n", schedule.max_jitter_us));

    r.push_str(&format!("{:>6}{:>10}{:>18}{:>8}{:>10}{:>10}\n",
        "Slot", "Offset", "Message", "CAN ID", "TX(us)", "Slot(us)"));
    r.push_str(&"─".repeat(66));
    r.push('\n');

    for slot in &schedule.slots {
        r.push_str(&format!("{:>6}{:>10}{:>18}{:>8}{:>10.1}{:>10}\n",
            slot.slot_id, slot.offset_us, slot.message_name,
            format!("0x{:03X}", slot.can_id), slot.tx_time_us, slot.slot_length_us));
    }

    r
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::c_codegen::CTarget;

    #[test]
    fn test_can_message_transmission_time() {
        let msg = CanMessage::new("MotorCmd", 0x100, 10000, "motorTask").with_dlc(8);
        let time = msg.transmission_time_us(500); // 500kbps
        // 标准帧 8 bytes: (47 + 64) * 1.15 / 500 ≈ 0.255μs... wait
        // 111 bits * 1.15 = 127.65 bits / 500 kbps = 255.3 μs? No...
        // 500 kbps = 500 bits/ms = 0.5 bits/μs → 127.65 / 0.5 = 255.3 μs? That's wrong
        // Actually: 500 kbps = 500,000 bits/s = 0.5 bits/μs
        // time = bits / (baudrate_kbps) * 1000 = 127.65 / 500 * 1000 = 255.3 μs
        // Hmm that seems high. Let me recalculate:
        // 500 kbps → 1 bit = 2μs. 127 bits → 254μs. That's correct for CAN!
        assert!(time > 200.0 && time < 300.0, "CAN frame time should be ~255μs at 500kbps, got {}", time);
    }

    #[test]
    fn test_ttcan_schedule_generation() {
        let messages = vec![
            CanMessage::new("MotorCmd", 0x100, 1000, "motorTask").with_dlc(4),
            CanMessage::new("SensorData", 0x200, 5000, "sensorTask").with_dlc(8),
            CanMessage::new("DiagInfo", 0x700, 100000, "diagTask").with_dlc(2),
        ];
        let schedule = generate_ttcan_schedule(&messages, 500);
        assert!(schedule.feasible);
        assert_eq!(schedule.slots.len(), 3);
        // 按 CAN ID 排序: 0x100, 0x200, 0x700
        assert_eq!(schedule.slots[0].can_id, 0x100);
        assert_eq!(schedule.slots[1].can_id, 0x200);
        assert!(schedule.bus_utilization < 1.0);
    }

    #[test]
    fn test_e2e_latency_analysis() {
        let link = EndToEndLink {
            name: "Steering Control".into(),
            nodes: vec![
                LinkNode { name: "SteeringAngleSensor".into(), kind: LinkNodeKind::SensorAcquire, latency_us: 50.0 },
                LinkNode { name: "CAN_TX".into(), kind: LinkNodeKind::CanTransmit, latency_us: 255.0 },
                LinkNode { name: "ECU_Compute".into(), kind: LinkNodeKind::Compute, latency_us: 42.0 },
                LinkNode { name: "CAN_RX".into(), kind: LinkNodeKind::CanReceive, latency_us: 255.0 },
                LinkNode { name: "MotorDriver".into(), kind: LinkNodeKind::ActuatorOutput, latency_us: 20.0 },
            ],
            deadline_us: 1000.0,
        };
        let result = analyze_e2e_latency(&link);
        assert!(result.met);
        assert!((result.total_latency_us - 622.0).abs() < 0.1);
        assert!(result.margin_us > 300.0);
        assert!(result.proof.contains("SATISFIED"));
    }

    #[test]
    fn test_e2e_latency_violated() {
        let link = EndToEndLink {
            name: "Brake".into(),
            nodes: vec![
                LinkNode { name: "PedalSensor".into(), kind: LinkNodeKind::SensorAcquire, latency_us: 100.0 },
                LinkNode { name: "CAN".into(), kind: LinkNodeKind::CanTransmit, latency_us: 500.0 },
                LinkNode { name: "ABS_ECU".into(), kind: LinkNodeKind::Compute, latency_us: 200.0 },
                LinkNode { name: "HydraulicValve".into(), kind: LinkNodeKind::ActuatorOutput, latency_us: 300.0 },
            ],
            deadline_us: 1000.0,
        };
        let result = analyze_e2e_latency(&link);
        // 100+500+200+300 = 1100 > 1000 → violated
        assert!(!result.met);
    }

    #[test]
    fn test_watchdog_esp32() {
        let config = WatchdogConfig {
            timeout_ms: 500,
            feeder_task: "mainLoop".into(),
            feed_period_ms: 100,
            timeout_action: WatchdogAction::SystemReset,
        };
        let code = generate_watchdog_code(&config, &CTarget::Esp32);
        assert!(code.contains("esp_task_wdt_init"));
        assert!(code.contains("_duanyan_wdt_feed"));
    }

    #[test]
    fn test_safestate_generation() {
        let config = SafeStateConfig {
            system_name: "MotorController".into(),
            safe_outputs: vec![
                SafeOutput { name: "PWM_A".into(), kind: SafeOutputKind::PwmDuty, safe_value: 0 },
                SafeOutput { name: "Relay".into(), kind: SafeOutputKind::GpioLow, safe_value: 0 },
                SafeOutput { name: "Motor".into(), kind: SafeOutputKind::MotorStop, safe_value: 0 },
            ],
            triggers: vec![
                FaultTrigger { name: "WCET overrun".into(), kind: FaultKind::WcetTimeout, threshold: 100 },
                FaultTrigger { name: "CAN timeout".into(), kind: FaultKind::CommLoss, threshold: 50 },
            ],
            auto_recover: false,
        };
        let code = generate_safestate_code(&config);
        assert!(code.contains("SYS_SAFE_STATE"));
        assert!(code.contains("_duanyan_enter_safe_state"));
        assert!(code.contains("MotorController"));
        assert!(code.contains("PWM_A"));
    }

    #[test]
    fn test_can_report() {
        let messages = vec![
            CanMessage::new("Cmd", 0x010, 1000, "ctrl").with_dlc(4),
        ];
        let schedule = generate_ttcan_schedule(&messages, 500);
        let report = generate_can_report(&schedule, 500);
        assert!(report.contains("CAN Schedule"));
        assert!(report.contains("500 kbps"));
        assert!(report.contains("Cmd"));
    }
}
