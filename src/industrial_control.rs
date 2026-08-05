//! Phase IC: 工业控制垂直模块
//!
//! 面向 PLC / 运动控制 / 安全仪表系统：
//! - IEC 61131-3 兼容层 (ST 语法子集映射)
//! - EtherCAT 主站循环调度
//! - SIL 3 安全 PLC 架构 (1oo2D)
//! - 多轴运动控制 (S 曲线 / 插补 / 三环级联)

// =============================================================================
// IEC 61131-3 兼容层
// =============================================================================

/// IEC 61131-3 编程语言类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Iec61131Lang {
    /// Structured Text (最接近 HardyScript)
    ST,
    /// Function Block Diagram
    FBD,
    /// Ladder Diagram
    LD,
    /// Instruction List
    IL,
    /// Sequential Function Chart
    SFC,
}

/// PLC 程序组织单元 (POU)
#[derive(Debug, Clone)]
pub struct PlcPou {
    /// POU 名称
    pub name: String,
    /// 类型
    pub kind: PouKind,
    /// 对应 HardyScript 函数名
    pub hs_function: String,
    /// 执行周期 (μs)
    pub cycle_us: u64,
    /// 输入变量
    pub inputs: Vec<PlcVar>,
    /// 输出变量
    pub outputs: Vec<PlcVar>,
    /// 局部变量
    pub locals: Vec<PlcVar>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PouKind {
    /// 程序 (无返回)
    Program,
    /// 函数 (有返回)
    Function,
    /// 功能块 (有状态)
    FunctionBlock,
}

/// PLC 变量
#[derive(Debug, Clone)]
pub struct PlcVar {
    pub name: String,
    pub data_type: PlcType,
    /// 初始值
    pub initial: Option<f64>,
    /// 地址映射 (如 %IX0.0)
    pub address: Option<String>,
}

/// IEC 61131-3 数据类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlcType {
    Bool,
    Int8,
    Int16,
    Int32,
    UInt8,
    UInt16,
    UInt32,
    Real,    // 32-bit float
    LReal,   // 64-bit float
    Time,    // 时间 (ms)
    Timer,   // TON/TOF/TP
    Counter, // CTU/CTD
}

impl PlcType {
    /// 映射到 C 类型
    pub fn to_c_type(&self) -> &'static str {
        match self {
            Self::Bool => "uint8_t",
            Self::Int8 => "int8_t",
            Self::Int16 => "int16_t",
            Self::Int32 => "int32_t",
            Self::UInt8 => "uint8_t",
            Self::UInt16 => "uint16_t",
            Self::UInt32 => "uint32_t",
            Self::Real => "float",
            Self::LReal => "double",
            Self::Time => "uint32_t",
            Self::Timer => "_hardy_timer_t",
            Self::Counter => "_hardy_counter_t",
        }
    }

    /// 字节大小
    pub fn size_bytes(&self) -> usize {
        match self {
            Self::Bool | Self::Int8 | Self::UInt8 => 1,
            Self::Int16 | Self::UInt16 => 2,
            Self::Int32 | Self::UInt32 | Self::Real | Self::Time => 4,
            Self::LReal => 8,
            Self::Timer => 12,
            Self::Counter => 8,
        }
    }
}

/// 从 HardyScript 函数生成 IEC 61131-3 ST 代码
pub fn generate_st_code(pou: &PlcPou, body_logic: &str) -> String {
    let mut code = String::new();

    let kind_str = match pou.kind {
        PouKind::Program => "PROGRAM",
        PouKind::Function => "FUNCTION",
        PouKind::FunctionBlock => "FUNCTION_BLOCK",
    };

    code.push_str(&format!("{} {}\n", kind_str, pou.name));

    // 变量声明
    code.push_str("VAR");
    if !pou.inputs.is_empty() { code.push_str("_INPUT"); }
    code.push('\n');
    for v in &pou.inputs {
        code.push_str(&format!("    {} : {};\n", v.name, plc_type_st(v.data_type)));
    }
    code.push_str("END_VAR\n");

    if !pou.outputs.is_empty() {
        code.push_str("VAR_OUTPUT\n");
        for v in &pou.outputs {
            code.push_str(&format!("    {} : {};\n", v.name, plc_type_st(v.data_type)));
        }
        code.push_str("END_VAR\n");
    }

    if !pou.locals.is_empty() {
        code.push_str("VAR\n");
        for v in &pou.locals {
            let init = v.initial.map(|i| format!(" := {}", i)).unwrap_or_default();
            code.push_str(&format!("    {} : {}{};\n", v.name, plc_type_st(v.data_type), init));
        }
        code.push_str("END_VAR\n");
    }

    code.push('\n');
    code.push_str(body_logic);
    code.push_str(&format!("\nEND_{}\n", kind_str));

    code
}

fn plc_type_st(t: PlcType) -> &'static str {
    match t {
        PlcType::Bool => "BOOL",
        PlcType::Int8 => "SINT",
        PlcType::Int16 => "INT",
        PlcType::Int32 => "DINT",
        PlcType::UInt8 => "USINT",
        PlcType::UInt16 => "UINT",
        PlcType::UInt32 => "UDINT",
        PlcType::Real => "REAL",
        PlcType::LReal => "LREAL",
        PlcType::Time => "TIME",
        PlcType::Timer => "TON",
        PlcType::Counter => "CTU",
    }
}

// =============================================================================
// EtherCAT 主站调度
// =============================================================================

/// EtherCAT 从站
#[derive(Debug, Clone)]
pub struct EcatSlave {
    /// 从站名称
    pub name: String,
    /// 站地址
    pub station: u16,
    /// 供应商 ID
    pub vendor_id: u32,
    /// 产品码
    pub product_code: u32,
    /// 输入字节数
    pub input_bytes: usize,
    /// 输出字节数
    pub output_bytes: usize,
    /// 是否支持分布式时钟
    pub dc_supported: bool,
}

/// EtherCAT 调度配置
#[derive(Debug, Clone)]
pub struct EcatSchedule {
    /// 循环周期 (μs, 典型 1000 或 250)
    pub cycle_us: u64,
    /// 从站列表
    pub slaves: Vec<EcatSlave>,
    /// 过程数据映像大小
    pub pdi_input_bytes: usize,
    pub pdi_output_bytes: usize,
    /// 总线传输时间 (μs)
    pub bus_time_us: f64,
    /// 是否满足周期
    pub cycle_feasible: bool,
    /// 抖动 (μs)
    pub jitter_us: f64,
}

/// 规划 EtherCAT 调度
pub fn plan_ecat_schedule(slaves: &[EcatSlave], cycle_us: u64) -> EcatSchedule {
    let pdi_in: usize = slaves.iter().map(|s| s.input_bytes).sum();
    let pdi_out: usize = slaves.iter().map(|s| s.output_bytes).sum();

    // EtherCAT 100Mbps: 1 byte ≈ 0.08μs + 帧头开销
    let total_bytes = pdi_in + pdi_out + slaves.len() * 12; // 12 bytes/slave 帧头
    let bus_time = total_bytes as f64 * 0.08 + 5.0; // +5μs 帧间间隔

    // 处理时间估算 (每从站 ~2μs)
    let processing_time = slaves.len() as f64 * 2.0;
    let total_time = bus_time + processing_time;

    let feasible = total_time < cycle_us as f64 * 0.7; // 留 30% 余量
    let jitter = if feasible { 1.5 } else { total_time - cycle_us as f64 };

    EcatSchedule {
        cycle_us,
        slaves: slaves.to_vec(),
        pdi_input_bytes: pdi_in,
        pdi_output_bytes: pdi_out,
        bus_time_us: bus_time,
        cycle_feasible: feasible,
        jitter_us: jitter,
    }
}

/// 生成 EtherCAT 循环调度 C 代码
pub fn generate_ecat_cyclic_code(schedule: &EcatSchedule) -> String {
    let mut code = String::new();
    code.push_str("/* HardyScript IC: EtherCAT Cyclic Executive */\n");
    code.push_str(&format!("/* Cycle: {}μs, Slaves: {} */\n\n", schedule.cycle_us, schedule.slaves.len()));

    // 过程数据映像
    code.push_str(&format!("static uint8_t _ecat_output[{}];\n", schedule.pdi_output_bytes.max(1)));
    code.push_str(&format!("static uint8_t _ecat_input[{}];\n\n", schedule.pdi_input_bytes.max(1)));

    // 循环执行函数
    code.push_str("void _ecat_cyclic_task(void) {\n");
    code.push_str("    /* 1. 输出过程数据 → 从站 */\n");
    code.push_str("    _ecat_send_outputs();\n");
    code.push_str("    /* 2. 接收输入过程数据 */\n");
    code.push_str("    _ecat_recv_inputs();\n");
    code.push_str("    /* 3. 执行用户逻辑 */\n");
    code.push_str("    _hardy_plc_cycle();\n");
    code.push_str("    /* 4. 分布式时钟同步 */\n");
    code.push_str("    _ecat_dc_sync();\n");
    code.push_str("}\n\n");

    // 定时器配置
    code.push_str(&format!(
        "/* Timer: {}μs cycle → {}Hz */\n",
        schedule.cycle_us,
        1_000_000 / schedule.cycle_us.max(1)
    ));

    code
}

// =============================================================================
// SIL 3 安全 PLC (1oo2D 架构)
// =============================================================================

/// 安全等级 (IEC 61508)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SilLevel {
    Sil1,
    Sil2,
    Sil3,
    Sil4,
}

impl SilLevel {
    /// 允许的 PFD (Probability of Failure on Demand)
    pub fn pfd_range(&self) -> &'static str {
        match self {
            Self::Sil1 => "10^-1 ~ 10^-2",
            Self::Sil2 => "10^-2 ~ 10^-3",
            Self::Sil3 => "10^-3 ~ 10^-4",
            Self::Sil4 => "10^-4 ~ 10^-5",
        }
    }

    /// 硬件容错要求
    pub fn hft_requirement(&self) -> &'static str {
        match self {
            Self::Sil1 => "HFT=0 (single channel acceptable)",
            Self::Sil2 => "HFT=0 with diagnostics OR HFT=1",
            Self::Sil3 => "HFT=1 (1oo2D required)",
            Self::Sil4 => "HFT=2 (2oo3 required)",
        }
    }
}

/// 1oo2D 安全架构配置
#[derive(Debug, Clone)]
pub struct SafetyPlcConfig {
    /// 系统名称
    pub name: String,
    /// SIL 等级
    pub sil: SilLevel,
    /// 通道 A 任务列表
    pub channel_a_tasks: Vec<String>,
    /// 通道 B 任务列表 (与 A 相同逻辑，不同编译)
    pub channel_b_tasks: Vec<String>,
    /// 诊断覆盖率 (%)
    pub diagnostic_coverage: f64,
    /// 表决周期 (μs)
    pub vote_period_us: u64,
    /// 安全状态输出
    pub safe_outputs: Vec<(String, u32)>,
}

/// 生成 1oo2D 安全架构 C 代码
pub fn generate_safety_plc_code(config: &SafetyPlcConfig) -> String {
    let mut code = String::new();
    code.push_str(&format!("/* HardyScript IC: Safety PLC — {} */\n", config.name));
    code.push_str(&format!("/* Architecture: 1oo2D, SIL {} */\n", match config.sil {
        SilLevel::Sil1 => "1", SilLevel::Sil2 => "2", SilLevel::Sil3 => "3", SilLevel::Sil4 => "4",
    }));
    code.push_str(&format!("/* Diagnostic Coverage: {:.0}% */\n\n", config.diagnostic_coverage));

    // 双通道数据结构
    code.push_str("typedef struct {\n");
    code.push_str("    uint32_t outputs[16];\n");
    code.push_str("    uint32_t crc;\n");
    code.push_str("    uint8_t  alive_counter;\n");
    code.push_str("} _channel_result_t;\n\n");

    code.push_str("static _channel_result_t _ch_a_result;\n");
    code.push_str("static _channel_result_t _ch_b_result;\n\n");

    // 表决器
    code.push_str("/* Voter: compare channel A and B */\n");
    code.push_str("static inline int _safety_vote(void) {\n");
    code.push_str("    if (_ch_a_result.crc != _ch_b_result.crc) return 0; /* MISMATCH */\n");
    code.push_str("    for (int i = 0; i < 16; i++) {\n");
    code.push_str("        if (_ch_a_result.outputs[i] != _ch_b_result.outputs[i]) return 0;\n");
    code.push_str("    }\n");
    code.push_str("    return 1; /* AGREEMENT */\n");
    code.push_str("}\n\n");

    // 安全状态
    code.push_str("static inline void _enter_safe_state(void) {\n");
    for (name, val) in &config.safe_outputs {
        code.push_str(&format!("    /* {} = {} */\n", name, val));
    }
    code.push_str("}\n\n");

    // 主循环
    code.push_str("void _safety_plc_main(void) {\n");
    code.push_str("    /* Channel A execution */\n");
    code.push_str("    _channel_a_run(&_ch_a_result);\n");
    code.push_str("    /* Channel B execution (diverse compilation) */\n");
    code.push_str("    _channel_b_run(&_ch_b_result);\n");
    code.push_str("    /* Vote */\n");
    code.push_str("    if (!_safety_vote()) {\n");
    code.push_str("        _enter_safe_state();\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    code
}

// =============================================================================
// 多轴运动控制
// =============================================================================

/// 运动轴配置
#[derive(Debug, Clone)]
pub struct MotionAxis {
    /// 轴名称
    pub name: String,
    /// 轴编号
    pub id: u8,
    /// 最大速度 (units/s)
    pub max_velocity: f64,
    /// 最大加速度 (units/s²)
    pub max_acceleration: f64,
    /// 最大加加速度 (jerk, units/s³)
    pub max_jerk: f64,
    /// 位置环周期 (μs)
    pub pos_loop_us: u64,
    /// 速度环周期 (μs)
    pub vel_loop_us: u64,
    /// 电流环周期 (μs)
    pub cur_loop_us: u64,
}

/// S 曲线运动规划结果
#[derive(Debug, Clone)]
pub struct SCurveProfile {
    /// 总时间 (s)
    pub total_time: f64,
    /// 各阶段时间 [加速加段, 匀加速段, 加速减段, 匀速段, 减速加段, 匀减速段, 减速减段]
    pub phase_times: [f64; 7],
    /// 峰值速度
    pub peak_velocity: f64,
    /// 峰值加速度
    pub peak_acceleration: f64,
    /// 是否受速度限制
    pub velocity_limited: bool,
}

/// 计算 S 曲线运动规划
pub fn plan_s_curve(distance: f64, axis: &MotionAxis) -> SCurveProfile {
    let v_max = axis.max_velocity;
    let a_max = axis.max_acceleration;
    let j_max = axis.max_jerk;

    // 加加速段时间
    let t_j = (a_max / j_max).sqrt();

    // 加速段总时间
    let t_a = t_j * 2.0 + (v_max - 2.0 * j_max * t_j * t_j) / a_max;

    // 加速段距离
    let d_a = v_max * t_a / 2.0;

    // 判断是否有匀速段
    let velocity_limited = d_a * 2.0 < distance;
    let (peak_v, t_v) = if velocity_limited {
        let t_const = (distance - 2.0 * d_a) / v_max;
        (v_max, t_const)
    } else {
        // 达不到最大速度，重新计算
        let actual_v = (distance * j_max).cbrt() * 2.0;
        (actual_v.min(v_max), 0.0)
    };

    let total = t_a * 2.0 + t_v;

    SCurveProfile {
        total_time: total,
        phase_times: [t_j, t_a - 2.0 * t_j, t_j, t_v, t_j, t_a - 2.0 * t_j, t_j],
        peak_velocity: peak_v,
        peak_acceleration: a_max,
        velocity_limited,
    }
}

/// 插补类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InterpolationKind {
    /// 直线插补
    Linear,
    /// 圆弧插补
    Circular,
    /// 样条插补
    Spline,
}

/// 生成运动控制 C 代码
pub fn generate_motion_control_code(axes: &[MotionAxis]) -> String {
    let mut code = String::new();
    code.push_str("/* HardyScript IC: Multi-Axis Motion Control */\n");
    code.push_str(&format!("/* Axes: {} */\n\n", axes.len()));

    // 轴结构体
    code.push_str("typedef struct {\n");
    code.push_str("    volatile int32_t  position;     /* encoder counts */\n");
    code.push_str("    volatile int32_t  velocity;     /* counts/s */\n");
    code.push_str("    volatile int32_t  torque;       /* PWM duty */\n");
    code.push_str("    int32_t  target_pos;\n");
    code.push_str("    int32_t  target_vel;\n");
    code.push_str("    uint8_t  enabled;\n");
    code.push_str("    uint8_t  fault;\n");
    code.push_str("} _axis_state_t;\n\n");

    code.push_str(&format!("static _axis_state_t _axes[{}];\n\n", axes.len().max(1)));

    // 三环级联
    code.push_str("/* Current loop (innermost, fastest) */\n");
    code.push_str("static inline int32_t _current_loop(int32_t ref, int32_t actual, int32_t integral) {\n");
    code.push_str("    int32_t err = ref - actual;\n");
    code.push_str("    integral += err;\n");
    code.push_str("    return err * 100 + integral * 10; /* P=100, I=10 */\n");
    code.push_str("}\n\n");

    code.push_str("/* Velocity loop */\n");
    code.push_str("static inline int32_t _velocity_loop(int32_t ref, int32_t actual) {\n");
    code.push_str("    int32_t err = ref - actual;\n");
    code.push_str("    return err * 50; /* P=50 */\n");
    code.push_str("}\n\n");

    code.push_str("/* Position loop (outermost, slowest) */\n");
    code.push_str("static inline int32_t _position_loop(int32_t target, int32_t actual) {\n");
    code.push_str("    int32_t err = target - actual;\n");
    code.push_str("    return err * 20; /* P=20, output = velocity ref */\n");
    code.push_str("}\n\n");

    // 各轴配置
    for axis in axes {
        code.push_str(&format!("/* Axis {}: {} (pos:{}μs vel:{}μs cur:{}μs) */\n",
            axis.id, axis.name, axis.pos_loop_us, axis.vel_loop_us, axis.cur_loop_us));
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
    fn test_plc_type_mapping() {
        assert_eq!(PlcType::Bool.to_c_type(), "uint8_t");
        assert_eq!(PlcType::Int32.to_c_type(), "int32_t");
        assert_eq!(PlcType::Real.to_c_type(), "float");
        assert_eq!(PlcType::Int32.size_bytes(), 4);
    }

    #[test]
    fn test_st_code_generation() {
        let pou = PlcPou {
            name: "MotorControl".into(),
            kind: PouKind::FunctionBlock,
            hs_function: "motorCtrl".into(),
            cycle_us: 1000,
            inputs: vec![PlcVar { name: "Enable".into(), data_type: PlcType::Bool, initial: None, address: Some("%IX0.0".into()) }],
            outputs: vec![PlcVar { name: "Running".into(), data_type: PlcType::Bool, initial: None, address: Some("%QX0.0".into()) }],
            locals: vec![PlcVar { name: "Speed".into(), data_type: PlcType::Real, initial: Some(0.0), address: None }],
        };
        let code = generate_st_code(&pou, "    Running := Enable AND (Speed > 0.0);");
        assert!(code.contains("FUNCTION_BLOCK MotorControl"));
        assert!(code.contains("Enable : BOOL"));
        assert!(code.contains("Running : BOOL"));
        assert!(code.contains("Speed : REAL := 0"));
        assert!(code.contains("END_FUNCTION_BLOCK"));
    }

    #[test]
    fn test_ecat_schedule() {
        let slaves = vec![
            EcatSlave { name: "Servo1".into(), station: 1001, vendor_id: 0x1234, product_code: 0x5678, input_bytes: 32, output_bytes: 32, dc_supported: true },
            EcatSlave { name: "Servo2".into(), station: 1002, vendor_id: 0x1234, product_code: 0x5678, input_bytes: 32, output_bytes: 32, dc_supported: true },
            EcatSlave { name: "IO_Module".into(), station: 1003, vendor_id: 0xAAAA, product_code: 0xBBBB, input_bytes: 8, output_bytes: 8, dc_supported: false },
        ];
        let schedule = plan_ecat_schedule(&slaves, 1000);
        assert!(schedule.cycle_feasible, "3 slaves at 1ms should be feasible");
        assert_eq!(schedule.pdi_input_bytes, 72);
        assert_eq!(schedule.pdi_output_bytes, 72);
        assert!(schedule.jitter_us < 10.0);
    }

    #[test]
    fn test_ecat_tight_cycle() {
        let slaves: Vec<EcatSlave> = (0..20).map(|i| EcatSlave {
            name: format!("Slave{}", i), station: 1000 + i, vendor_id: 1, product_code: 1,
            input_bytes: 64, output_bytes: 64, dc_supported: true,
        }).collect();
        let schedule = plan_ecat_schedule(&slaves, 250); // 20 slaves at 250μs = tight
        // 可能不可行
        assert!(schedule.bus_time_us > 10.0);
    }

    #[test]
    fn test_safety_plc_code() {
        let config = SafetyPlcConfig {
            name: "Burner_Controller".into(),
            sil: SilLevel::Sil3,
            channel_a_tasks: vec!["flameDetect".into(), "gasValve".into()],
            channel_b_tasks: vec!["flameDetect".into(), "gasValve".into()],
            diagnostic_coverage: 99.0,
            vote_period_us: 100,
            safe_outputs: vec![("GasValve".into(), 0), ("Burner".into(), 0)],
        };
        let code = generate_safety_plc_code(&config);
        assert!(code.contains("1oo2D"));
        assert!(code.contains("SIL 3"));
        assert!(code.contains("_safety_vote"));
        assert!(code.contains("_enter_safe_state"));
        assert!(code.contains("GasValve = 0"));
    }

    #[test]
    fn test_s_curve_profile() {
        let axis = MotionAxis {
            name: "X".into(), id: 0,
            max_velocity: 1000.0,
            max_acceleration: 5000.0,
            max_jerk: 50000.0,
            pos_loop_us: 1000,
            vel_loop_us: 100,
            cur_loop_us: 50,
        };
        let profile = plan_s_curve(10000.0, &axis);
        assert!(profile.total_time > 0.0);
        assert!(profile.peak_velocity > 0.0);
        assert!(profile.peak_velocity <= axis.max_velocity);
        assert_eq!(profile.phase_times.len(), 7);
    }

    #[test]
    fn test_motion_control_code() {
        let axes = vec![
            MotionAxis { name: "X".into(), id: 0, max_velocity: 100.0, max_acceleration: 500.0, max_jerk: 5000.0, pos_loop_us: 1000, vel_loop_us: 100, cur_loop_us: 50 },
            MotionAxis { name: "Y".into(), id: 1, max_velocity: 100.0, max_acceleration: 500.0, max_jerk: 5000.0, pos_loop_us: 1000, vel_loop_us: 100, cur_loop_us: 50 },
        ];
        let code = generate_motion_control_code(&axes);
        assert!(code.contains("_axis_state_t"));
        assert!(code.contains("_current_loop"));
        assert!(code.contains("_velocity_loop"));
        assert!(code.contains("_position_loop"));
        assert!(code.contains("Axis 0: X"));
        assert!(code.contains("Axis 1: Y"));
    }

    #[test]
    fn test_sil_levels() {
        assert!(SilLevel::Sil3 > SilLevel::Sil2);
        assert!(SilLevel::Sil3.hft_requirement().contains("1oo2D"));
    }
}
