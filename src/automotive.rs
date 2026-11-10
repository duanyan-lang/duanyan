//! Phase AD: 自动驾驶垂直模块
//!
//! 面向 ECU / 域控制器 / 中央计算平台：
//! - CAN FD 双波特率通信
//! - 混合关键性调度 (ASIL 隔离)
//! - Fail-Operational 双通道架构
//! - AUTOSAR SWC 接口生成

// =============================================================================
// CAN FD 通信
// =============================================================================

/// CAN FD 帧配置
#[derive(Debug, Clone)]
pub struct CanFdConfig {
    /// 仲裁段波特率 (kbps, 典型 500)
    pub arbitration_baudrate: u32,
    /// 数据段波特率 (kbps, 典型 2000/5000)
    pub data_baudrate: u32,
    /// 是否启用 BRS (Bit Rate Switch)
    pub brs_enabled: bool,
    /// 最大 DLC
    pub max_dlc: u8,
}

impl CanFdConfig {
    /// 典型配置: 500k/2M
    pub fn standard() -> Self {
        Self { arbitration_baudrate: 500, data_baudrate: 2000, brs_enabled: true, max_dlc: 64 }
    }

    /// 高性能配置: 1M/5M
    pub fn high_speed() -> Self {
        Self { arbitration_baudrate: 1000, data_baudrate: 5000, brs_enabled: true, max_dlc: 64 }
    }

    /// 单比特时间 (ns, 仲裁段)
    pub fn arb_bit_time_ns(&self) -> f64 {
        1_000_000.0 / self.arbitration_baudrate as f64
    }

    /// 单比特时间 (ns, 数据段)
    pub fn data_bit_time_ns(&self) -> f64 {
        1_000_000.0 / self.data_baudrate as f64
    }
}

/// CAN FD 消息
#[derive(Debug, Clone)]
pub struct CanFdMessage {
    /// 消息名称
    pub name: String,
    /// 消息 ID
    pub id: u32,
    /// 是否扩展帧
    pub extended: bool,
    /// DLC (0-64)
    pub dlc: u8,
    /// 发送周期 (ms)
    pub period_ms: f64,
    /// 发送节点
    pub sender: String,
    /// ASIL 等级
    pub asil: AsilGrade,
    /// 信号列表
    pub signals: Vec<CanSignal>,
}

/// CAN 信号
#[derive(Debug, Clone)]
pub struct CanSignal {
    pub name: String,
    /// 起始位
    pub start_bit: u8,
    /// 长度 (bits)
    pub length: u8,
    /// 缩放因子
    pub factor: f64,
    /// 偏移
    pub offset: f64,
    /// 物理单位
    pub unit: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AsilGrade {
    QM,
    AsilA,
    AsilB,
    AsilC,
    AsilD,
}

impl AsilGrade {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::QM => "QM", Self::AsilA => "ASIL-A", Self::AsilB => "ASIL-B",
            Self::AsilC => "ASIL-C", Self::AsilD => "ASIL-D",
        }
    }
}

/// 计算 CAN FD 总线负载率
pub fn calculate_canfd_bus_load(messages: &[CanFdMessage], config: &CanFdConfig) -> f64 {
    let mut total_bits_per_sec = 0.0f64;

    for msg in messages {
        let period_s = msg.period_ms / 1000.0;
        if period_s <= 0.0 { continue; }
        let freq = 1.0 / period_s;

        // 仲裁段: SOF(1) + ID(29) + RTR(1) + IDE(1) + FDF(1) + BRS(1) + DLC(4) + CRC(17+delim)
        let arb_bits = if msg.extended { 47.0 } else { 34.0 };
        // 数据段: data + CRC + ACK + EOF
        let data_bytes = msg.dlc as f64;
        let data_bits = data_bytes * 8.0 + 21.0 + 2.0 + 7.0; // CRC17+SBC + ACK + EOF

        // 仲裁段时间 + 数据段时间
        let frame_time_ns = arb_bits * config.arb_bit_time_ns() + data_bits * config.data_bit_time_ns();
        let frame_bits_equiv = frame_time_ns / config.arb_bit_time_ns();

        total_bits_per_sec += frame_bits_equiv * freq;
    }

    let bus_capacity = config.arbitration_baudrate as f64 * 1000.0;
    total_bits_per_sec / bus_capacity * 100.0
}

// =============================================================================
// 混合关键性调度
// =============================================================================

/// 混合关键性任务
#[derive(Debug, Clone)]
pub struct MixedCritTask {
    pub name: String,
    pub asil: AsilGrade,
    /// WCET (μs)
    pub wcet_us: u64,
    /// 周期 (μs)
    pub period_us: u64,
    /// 截止期 (μs)
    pub deadline_us: u64,
    /// 分配的核 (多核 SoC)
    pub core_id: u8,
}

/// 混合关键性分析结果
#[derive(Debug, Clone)]
pub struct MixedCritAnalysis {
    /// 各 ASIL 等级利用率
    pub utilization_by_asil: Vec<(AsilGrade, f64)>,
    /// 总利用率
    pub total_utilization: f64,
    /// 是否可调度
    pub schedulable: bool,
    /// 时间分区方案
    pub partitions: Vec<TimePartition>,
    /// 隔离建议
    pub isolation_notes: Vec<String>,
}

/// 时间分区
#[derive(Debug, Clone)]
pub struct TimePartition {
    pub name: String,
    pub asil: AsilGrade,
    /// 分区起始 (μs, 相对主时间帧)
    pub offset_us: u64,
    /// 分区长度 (μs)
    pub duration_us: u64,
    /// 包含的任务
    pub tasks: Vec<String>,
}

/// 分析混合关键性调度
pub fn analyze_mixed_criticality(tasks: &[MixedCritTask]) -> MixedCritAnalysis {
    let mut util_by_asil: Vec<(AsilGrade, f64)> = Vec::new();
    let grades = [AsilGrade::QM, AsilGrade::AsilA, AsilGrade::AsilB, AsilGrade::AsilC, AsilGrade::AsilD];

    for &g in &grades {
        let u: f64 = tasks.iter()
            .filter(|t| t.asil == g)
            .map(|t| t.wcet_us as f64 / t.period_us as f64)
            .sum();
        if u > 0.0 {
            util_by_asil.push((g, u));
        }
    }

    let total_u: f64 = tasks.iter()
        .map(|t| t.wcet_us as f64 / t.period_us as f64)
        .sum();

    // 按 ASIL 等级生成时间分区
    let major_frame = tasks.iter().map(|t| t.period_us).max().unwrap_or(10000);
    let mut partitions = Vec::new();
    let mut offset = 0u64;

    // 高 ASIL 优先分配
    for &g in grades.iter().rev() {
        let group: Vec<&MixedCritTask> = tasks.iter().filter(|t| t.asil == g).collect();
        if group.is_empty() { continue; }
        let duration: u64 = group.iter().map(|t| t.wcet_us).sum::<u64>() + 50; // +50μs guard band
        partitions.push(TimePartition {
            name: format!("Partition_{}", g.as_str()),
            asil: g,
            offset_us: offset,
            duration_us: duration,
            tasks: group.iter().map(|t| t.name.clone()).collect(),
        });
        offset += duration;
    }

    let schedulable = total_u < 0.7 && offset <= major_frame;

    let mut notes = Vec::new();
    if total_u >= 0.7 {
        notes.push(format!("Total utilization {:.1}% exceeds 70% safety margin", total_u * 100.0));
    }
    // 检查 QM 和 ASIL-D 是否在同一核
    let qm_cores: Vec<u8> = tasks.iter().filter(|t| t.asil == AsilGrade::QM).map(|t| t.core_id).collect();
    let d_cores: Vec<u8> = tasks.iter().filter(|t| t.asil == AsilGrade::AsilD).map(|t| t.core_id).collect();
    for c in &d_cores {
        if qm_cores.contains(c) {
            notes.push(format!("Core {} shared by QM and ASIL-D tasks — MPU isolation required", c));
        }
    }

    MixedCritAnalysis {
        utilization_by_asil: util_by_asil,
        total_utilization: total_u,
        schedulable,
        partitions,
        isolation_notes: notes,
    }
}

// =============================================================================
// Fail-Operational 架构
// =============================================================================

/// 降级策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DegradationLevel {
    /// 全功能 (ASIL-D)
    Full,
    /// 降级运行 (ASIL-B, 限速/限功能)
    Degraded,
    /// 最小风险状态 (MRC: 安全停车)
    MinimalRisk,
}

/// Fail-Operational 配置
#[derive(Debug, Clone)]
pub struct FailOpConfig {
    /// 系统名称
    pub system_name: String,
    /// 主通道 ASIL
    pub primary_asil: AsilGrade,
    /// 备份通道 ASIL
    pub backup_asil: AsilGrade,
    /// 心跳周期 (ms)
    pub heartbeat_ms: u64,
    /// 故障检测超时 (ms)
    pub fault_timeout_ms: u64,
    /// 降级后限速 (km/h, 0=不限)
    pub degraded_speed_limit: u32,
    /// 安全停车时间预算 (s)
    pub mrc_time_budget_s: f64,
}

/// 生成 Fail-Operational C 代码
pub fn generate_failop_code(config: &FailOpConfig) -> String {
    let mut code = String::new();
    code.push_str(&format!("/* DUANYAN AD: Fail-Operational — {} */\n", config.system_name));
    code.push_str(&format!("/* Primary: {}, Backup: {} */\n\n", config.primary_asil.as_str(), config.backup_asil.as_str()));

    // 状态机
    code.push_str("typedef enum {\n");
    code.push_str("    SYS_STATE_NORMAL = 0,\n");
    code.push_str("    SYS_STATE_DEGRADED,\n");
    code.push_str("    SYS_STATE_MRC,        /* Minimal Risk Condition */\n");
    code.push_str("    SYS_STATE_FAULT\n");
    code.push_str("} _sys_state_t;\n\n");

    code.push_str("static volatile _sys_state_t _sys_state = SYS_STATE_NORMAL;\n");
    code.push_str(&format!("static volatile uint32_t _heartbeat_counter = 0;\n"));
    code.push_str(&format!("#define HEARTBEAT_TIMEOUT_MS {}\n", config.fault_timeout_ms));
    code.push_str(&format!("#define DEGRADED_SPEED_LIMIT {}\n\n", config.degraded_speed_limit));

    // 心跳监控
    code.push_str("/* Health Monitor: heartbeat check */\n");
    code.push_str("void _health_monitor_tick(void) {\n");
    code.push_str("    _heartbeat_counter++;\n");
    code.push_str("    if (_heartbeat_counter > HEARTBEAT_TIMEOUT_MS) {\n");
    code.push_str("        _trigger_degradation();\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");

    // 降级逻辑
    code.push_str("static void _trigger_degradation(void) {\n");
    code.push_str("    switch (_sys_state) {\n");
    code.push_str("    case SYS_STATE_NORMAL:\n");
    code.push_str("        _sys_state = SYS_STATE_DEGRADED;\n");
    code.push_str("        _activate_backup_channel();\n");
    code.push_str("        break;\n");
    code.push_str("    case SYS_STATE_DEGRADED:\n");
    code.push_str("        _sys_state = SYS_STATE_MRC;\n");
    code.push_str("        _execute_minimal_risk();\n");
    code.push_str("        break;\n");
    code.push_str("    default:\n");
    code.push_str("        _sys_state = SYS_STATE_FAULT;\n");
    code.push_str("        break;\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");

    // 安全停车
    code.push_str(&format!("/* MRC: safe stop within {:.1}s */\n", config.mrc_time_budget_s));
    code.push_str("static void _execute_minimal_risk(void) {\n");
    code.push_str("    /* 1. 减速到 0 */\n");
    code.push_str("    /* 2. 开启双闪 */\n");
    code.push_str("    /* 3. 锁定转向 */\n");
    code.push_str("    /* 4. 通知远程监控 */\n");
    code.push_str("}\n\n");

    // 程序流监控 (PFM)
    code.push_str("/* Program Flow Monitoring */\n");
    code.push_str("static volatile uint32_t _pfm_signature = 0;\n");
    code.push_str("static inline void _pfm_checkpoint(uint32_t expected) {\n");
    code.push_str("    _pfm_signature ^= expected;\n");
    code.push_str("}\n");
    code.push_str("static inline int _pfm_verify(uint32_t final_sig) {\n");
    code.push_str("    return _pfm_signature == final_sig;\n");
    code.push_str("}\n");

    code
}

// =============================================================================
// AUTOSAR SWC 接口
// =============================================================================

/// AUTOSAR Runnable 定义
#[derive(Debug, Clone)]
pub struct AutosarRunnable {
    /// Runnable 名称
    pub name: String,
    /// 对应 DUANYAN 函数
    pub hs_function: String,
    /// 触发类型
    pub trigger: RunnableTrigger,
    /// 周期 (ms, 仅 TimingEvent)
    pub period_ms: f64,
    /// ASIL 等级
    pub asil: AsilGrade,
    /// 读取的数据端口
    pub read_ports: Vec<String>,
    /// 写入的数据端口
    pub write_ports: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunnableTrigger {
    /// 周期触发
    TimingEvent,
    /// 数据接收触发
    DataReceivedEvent,
    /// 操作调用触发
    OperationInvoked,
}

/// AUTOSAR SWC 配置
#[derive(Debug, Clone)]
pub struct AutosarSwc {
    /// 组件名称
    pub name: String,
    /// Runnables
    pub runnables: Vec<AutosarRunnable>,
    /// 发送端口 (Sender-Receiver)
    pub sender_ports: Vec<(String, String)>, // (port_name, type)
    /// 接收端口
    pub receiver_ports: Vec<(String, String)>,
}

/// 生成 AUTOSAR SWC 骨架 C 代码
pub fn generate_autosar_swc(swc: &AutosarSwc) -> String {
    let mut code = String::new();
    code.push_str(&format!("/* DUANYAN AD: AUTOSAR SWC — {} */\n", swc.name));
    code.push_str(&format!("/* Runnables: {} */\n\n", swc.runnables.len()));

    // RTE 类型头
    code.push_str("#include \"Rte.h\"\n");
    code.push_str(&format!("#include \"Rte_{}.h\"\n\n", swc.name));

    // 端口声明
    if !swc.sender_ports.is_empty() {
        code.push_str("/* Sender Ports */\n");
        for (port, typ) in &swc.sender_ports {
            code.push_str(&format!("/* {} : {} */\n", port, typ));
        }
        code.push('\n');
    }
    if !swc.receiver_ports.is_empty() {
        code.push_str("/* Receiver Ports */\n");
        for (port, typ) in &swc.receiver_ports {
            code.push_str(&format!("/* {} : {} */\n", port, typ));
        }
        code.push('\n');
    }

    // Runnable 实现
    for r in &swc.runnables {
        let trigger_str = match r.trigger {
            RunnableTrigger::TimingEvent => format!("TimingEvent period={}ms", r.period_ms),
            RunnableTrigger::DataReceivedEvent => "DataReceivedEvent".into(),
            RunnableTrigger::OperationInvoked => "OperationInvoked".into(),
        };
        code.push_str(&format!("/* Runnable: {} [{}] ASIL={} */\n", r.name, trigger_str, r.asil.as_str()));
        code.push_str(&format!("void {}(void) {{\n", r.name));

        // 读取端口
        for port in &r.read_ports {
            code.push_str(&format!("    /* {}_Read() */\n", port));
        }

        // 调用 DUANYAN 编译的函数
        code.push_str(&format!("    _duanyan_{}();\n", r.hs_function));

        // 写入端口
        for port in &r.write_ports {
            code.push_str(&format!("    /* {}_Write() */\n", port));
        }

        code.push_str("}\n\n");
    }

    code
}

/// 生成 AUTOSAR 调度表 (BSW Scheduler)
pub fn generate_autosar_schedule(runnables: &[AutosarRunnable]) -> String {
    let mut code = String::new();
    code.push_str("/* DUANYAN AD: AUTOSAR OS Schedule Table */\n\n");

    // 按周期排序
    let mut sorted: Vec<&AutosarRunnable> = runnables.iter()
        .filter(|r| r.trigger == RunnableTrigger::TimingEvent)
        .collect();
    sorted.sort_by(|a, b| a.period_ms.partial_cmp(&b.period_ms).unwrap_or(std::cmp::Ordering::Equal));

    // 主时间帧 = 最大周期
    let major_frame = sorted.last().map(|r| r.period_ms).unwrap_or(10.0);
    code.push_str(&format!("/* Major Time Frame: {}ms */\n", major_frame));
    code.push_str(&format!("/* OS Alarms: {} */\n\n", sorted.len()));

    for r in &sorted {
        code.push_str(&format!("/* Alarm: {} → {}() every {}ms [{}] */\n",
            r.name, r.name, r.period_ms, r.asil.as_str()));
    }

    code
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canfd_config() {
        let cfg = CanFdConfig::standard();
        assert_eq!(cfg.arbitration_baudrate, 500);
        assert_eq!(cfg.data_baudrate, 2000);
        assert!(cfg.arb_bit_time_ns() > cfg.data_bit_time_ns());
    }

    #[test]
    fn test_canfd_bus_load() {
        let cfg = CanFdConfig::standard();
        let msgs = vec![
            CanFdMessage { name: "Radar".into(), id: 0x100, extended: false, dlc: 64, period_ms: 20.0, sender: "RadarECU".into(), asil: AsilGrade::AsilB, signals: vec![] },
            CanFdMessage { name: "Steering".into(), id: 0x200, extended: false, dlc: 8, period_ms: 10.0, sender: "SteerECU".into(), asil: AsilGrade::AsilD, signals: vec![] },
            CanFdMessage { name: "Brake".into(), id: 0x300, extended: false, dlc: 16, period_ms: 10.0, sender: "BrakeECU".into(), asil: AsilGrade::AsilD, signals: vec![] },
        ];
        let load = calculate_canfd_bus_load(&msgs, &cfg);
        assert!(load > 0.0);
        assert!(load < 100.0, "bus load should be under 100%: got {}", load);
    }

    #[test]
    fn test_mixed_criticality() {
        let tasks = vec![
            MixedCritTask { name: "BrakeCtrl".into(), asil: AsilGrade::AsilD, wcet_us: 200, period_us: 5000, deadline_us: 5000, core_id: 0 },
            MixedCritTask { name: "SteerCtrl".into(), asil: AsilGrade::AsilD, wcet_us: 150, period_us: 5000, deadline_us: 5000, core_id: 0 },
            MixedCritTask { name: "Infotainment".into(), asil: AsilGrade::QM, wcet_us: 2000, period_us: 10000, deadline_us: 10000, core_id: 1 },
            MixedCritTask { name: "HMI".into(), asil: AsilGrade::AsilA, wcet_us: 500, period_us: 10000, deadline_us: 10000, core_id: 1 },
        ];
        let analysis = analyze_mixed_criticality(&tasks);
        assert!(analysis.schedulable);
        assert!(analysis.total_utilization < 0.7);
        assert!(!analysis.partitions.is_empty());
        // ASIL-D 分区应排在前面
        assert_eq!(analysis.partitions[0].asil, AsilGrade::AsilD);
    }

    #[test]
    fn test_mixed_crit_core_conflict() {
        let tasks = vec![
            MixedCritTask { name: "SafeCtrl".into(), asil: AsilGrade::AsilD, wcet_us: 100, period_us: 5000, deadline_us: 5000, core_id: 0 },
            MixedCritTask { name: "Logging".into(), asil: AsilGrade::QM, wcet_us: 300, period_us: 10000, deadline_us: 10000, core_id: 0 },
        ];
        let analysis = analyze_mixed_criticality(&tasks);
        assert!(analysis.isolation_notes.iter().any(|n| n.contains("MPU isolation")));
    }

    #[test]
    fn test_failop_code() {
        let config = FailOpConfig {
            system_name: "SteeringController".into(),
            primary_asil: AsilGrade::AsilD,
            backup_asil: AsilGrade::AsilB,
            heartbeat_ms: 10,
            fault_timeout_ms: 50,
            degraded_speed_limit: 30,
            mrc_time_budget_s: 5.0,
        };
        let code = generate_failop_code(&config);
        assert!(code.contains("SYS_STATE_MRC"));
        assert!(code.contains("_trigger_degradation"));
        assert!(code.contains("_pfm_checkpoint"));
        assert!(code.contains("DEGRADED_SPEED_LIMIT 30"));
    }

    #[test]
    fn test_autosar_swc() {
        let swc = AutosarSwc {
            name: "LaneKeeping".into(),
            runnables: vec![
                AutosarRunnable {
                    name: "LKAS_Calc".into(), hs_function: "laneKeepCalc".into(),
                    trigger: RunnableTrigger::TimingEvent, period_ms: 10.0,
                    asil: AsilGrade::AsilB,
                    read_ports: vec!["CameraLane".into(), "YawRate".into()],
                    write_ports: vec!["SteerTorque".into()],
                },
            ],
            sender_ports: vec![("SteerTorque".into(), "Float32".into())],
            receiver_ports: vec![("CameraLane".into(), "LaneInfo".into()), ("YawRate".into(), "Float32".into())],
        };
        let code = generate_autosar_swc(&swc);
        assert!(code.contains("AUTOSAR SWC"));
        assert!(code.contains("LaneKeeping"));
        assert!(code.contains("LKAS_Calc"));
        assert!(code.contains("_duanyan_laneKeepCalc"));
        assert!(code.contains("Rte_LaneKeeping.h"));
    }

    #[test]
    fn test_autosar_schedule() {
        let runnables = vec![
            AutosarRunnable { name: "Fast_Ctrl".into(), hs_function: "fastCtrl".into(), trigger: RunnableTrigger::TimingEvent, period_ms: 5.0, asil: AsilGrade::AsilD, read_ports: vec![], write_ports: vec![] },
            AutosarRunnable { name: "Slow_HMI".into(), hs_function: "slowHmi".into(), trigger: RunnableTrigger::TimingEvent, period_ms: 100.0, asil: AsilGrade::QM, read_ports: vec![], write_ports: vec![] },
        ];
        let code = generate_autosar_schedule(&runnables);
        assert!(code.contains("Major Time Frame: 100ms"));
        assert!(code.contains("Fast_Ctrl"));
    }
}
