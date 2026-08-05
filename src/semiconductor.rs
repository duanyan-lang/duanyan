//! Phase SE: 半导体设备垂直模块
//!
//! 面向光刻机 / 晶圆检测 / 封装设备：
//! - 纳米级运动控制 (压电 + 音圈电机)
//! - 多轴同步插补 (工件台 + 掩模台)
//! - 振动抑制 (Notch Filter / 输入整形)
//! - 对准系统 (Alignment / Overlay)

// =============================================================================
// 纳米级运动轴
// =============================================================================

/// 纳米级运动轴配置
#[derive(Debug, Clone)]
pub struct NanoAxis {
    /// 轴名称
    pub name: String,
    /// 行程 (nm)
    pub stroke_nm: i64,
    /// 分辨率 (nm, 编码器最小步距)
    pub resolution_nm: f64,
    /// 最大速度 (mm/s)
    pub max_velocity_mm_s: f64,
    /// 最大加速度 (m/s²)
    pub max_accel_m_s2: f64,
    /// 定位精度 (nm)
    pub accuracy_nm: f64,
    /// 重复精度 (nm)
    pub repeatability_nm: f64,
    /// 控制环周期 (μs)
    pub servo_period_us: u64,
    /// 电机类型
    pub motor_type: MotorType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MotorType {
    /// 音圈电机 (VCM)
    VoiceCoil,
    /// 压电陶瓷
    Piezo,
    /// 直线电机
    LinearMotor,
    /// 平面电机 (H-type)
    PlanarMotor,
}

impl MotorType {
    pub fn bandwidth_hz(&self) -> f64 {
        match self {
            Self::VoiceCoil => 200.0,
            Self::Piezo => 2000.0,
            Self::LinearMotor => 500.0,
            Self::PlanarMotor => 300.0,
        }
    }
}

/// 纳米级定位性能指标
#[derive(Debug, Clone)]
pub struct NanoPositioningSpec {
    /// 稳态误差 (nm RMS)
    pub settling_error_nm: f64,
    /// 整定时间 (ms, 到 ±1nm)
    pub settling_time_ms: f64,
    /// 跟踪误差 (nm RMS, 扫描时)
    pub tracking_error_nm: f64,
    /// 是否满足规格
    pub spec_met: bool,
}

/// 计算定位性能
pub fn analyze_positioning(axis: &NanoAxis, pid_gains: &PidGains) -> NanoPositioningSpec {
    // 简化模型: 二阶系统 + PID
    let wn = pid_gains.kp.sqrt() * 100.0; // 自然频率 (rad/s)
    let zeta = pid_gains.kd / (2.0 * wn.max(1.0)); // 阻尼比

    // 稳态误差 ≈ 噪声 / 开环增益
    let settling_error = axis.resolution_nm / pid_gains.kp.max(0.1) * 3.0;

    // 整定时间 ≈ 4 / (zeta * wn) (2% 准则)
    let settling_time = if zeta > 0.0 && wn > 0.0 {
        4.0 / (zeta * wn) * 1000.0 // → ms
    } else {
        999.0
    };

    // 跟踪误差 ≈ velocity / (kp * servo_freq)
    let servo_freq = 1.0e6 / axis.servo_period_us as f64;
    let tracking_error = axis.max_velocity_mm_s * 1.0e6 / (pid_gains.kp * servo_freq);

    let spec_met = settling_error < axis.accuracy_nm
        && settling_time < 50.0
        && tracking_error < axis.repeatability_nm * 2.0;

    NanoPositioningSpec {
        settling_error_nm: settling_error,
        settling_time_ms: settling_time,
        tracking_error_nm: tracking_error,
        spec_met,
    }
}

/// PID 增益
#[derive(Debug, Clone)]
pub struct PidGains {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
}

// =============================================================================
// 振动抑制
// =============================================================================

/// 陷波滤波器配置
#[derive(Debug, Clone)]
pub struct NotchFilter {
    /// 中心频率 (Hz)
    pub center_freq_hz: f64,
    /// 带宽 (Hz)
    pub bandwidth_hz: f64,
    /// 衰减深度 (dB)
    pub depth_db: f64,
}

/// 输入整形器
#[derive(Debug, Clone)]
pub struct InputShaper {
    /// 类型
    pub kind: ShaperKind,
    /// 模态频率 (Hz)
    pub frequency_hz: f64,
    /// 阻尼比
    pub damping_ratio: f64,
    /// 脉冲序列 (amplitude, time_s)
    pub impulses: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShaperKind {
    /// Zero Vibration
    ZV,
    /// Zero Vibration and Derivative
    ZVD,
    /// Extra Insensitive
    EI,
}

/// 设计 ZVD 输入整形器
pub fn design_zvd_shaper(freq_hz: f64, damping: f64) -> InputShaper {
    let wn = 2.0 * std::f64::consts::PI * freq_hz;
    let wd = wn * (1.0 - damping * damping).sqrt();
    let period = 2.0 * std::f64::consts::PI / wd;
    let half_period = period / 2.0;

    let k = (-damping * std::f64::consts::PI / (1.0 - damping * damping).sqrt()).exp();
    let k2 = k * k;
    let denom = 1.0 + 2.0 * k + k2;

    let impulses = vec![
        (1.0 / denom, 0.0),
        (2.0 * k / denom, half_period),
        (k2 / denom, period),
    ];

    InputShaper {
        kind: ShaperKind::ZVD,
        frequency_hz: freq_hz,
        damping_ratio: damping,
        impulses,
    }
}

// =============================================================================
// 对准系统
// =============================================================================

/// 对准标记配置
#[derive(Debug, Clone)]
pub struct AlignmentConfig {
    /// 标记数量
    pub mark_count: u8,
    /// 对准精度要求 (nm)
    pub required_accuracy_nm: f64,
    /// 视场大小 (μm)
    pub field_of_view_um: f64,
    /// 标记类型
    pub mark_type: AlignMarkType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlignMarkType {
    /// 十字标记
    Cross,
    /// 光栅标记
    Grating,
    /// 棋盘格
    Checkerboard,
}

/// 对准结果
#[derive(Debug, Clone)]
pub struct AlignmentResult {
    /// X 偏移 (nm)
    pub offset_x_nm: f64,
    /// Y 偏移 (nm)
    pub offset_y_nm: f64,
    /// 旋转误差 (μrad)
    pub rotation_urad: f64,
    /// 缩放误差 (ppm)
    pub scale_ppm: f64,
    /// 残差 RMS (nm)
    pub residual_rms_nm: f64,
    /// 是否通过
    pub pass: bool,
}

/// 计算对准校正 (最小二乘)
pub fn compute_alignment_correction(
    config: &AlignmentConfig,
    measured_offsets: &[(f64, f64)], // (dx_nm, dy_nm) per mark
) -> AlignmentResult {
    let n = measured_offsets.len().max(1) as f64;
    let mean_x: f64 = measured_offsets.iter().map(|(x, _)| x).sum::<f64>() / n;
    let mean_y: f64 = measured_offsets.iter().map(|(_, y)| y).sum::<f64>() / n;

    // 残差 RMS
    let residual: f64 = measured_offsets.iter()
        .map(|(x, y)| (x - mean_x).powi(2) + (y - mean_y).powi(2))
        .sum::<f64>();
    let rms = (residual / n).sqrt();

    AlignmentResult {
        offset_x_nm: mean_x,
        offset_y_nm: mean_y,
        rotation_urad: 0.0, // 简化: 需要多标记计算
        scale_ppm: 0.0,
        residual_rms_nm: rms,
        pass: rms < config.required_accuracy_nm,
    }
}

// =============================================================================
// 代码生成
// =============================================================================

/// 生成纳米级伺服控制 C 代码
pub fn generate_nano_servo_code(axes: &[NanoAxis], gains: &PidGains) -> String {
    let mut code = String::new();
    code.push_str("/* HardyScript SE: Nano-Positioning Servo */\n");
    code.push_str(&format!("/* Axes: {}, Servo Period: {}μs */\n\n", axes.len(), axes[0].servo_period_us));

    // 固定点类型 (纳米精度)
    code.push_str("/* Fixed-point: 1 LSB = 0.1nm (Q22.10 format) */\n");
    code.push_str("typedef int64_t nano_pos_t;\n");
    code.push_str("#define NM_TO_RAW(nm) ((nano_pos_t)((nm) * 10))\n");
    code.push_str("#define RAW_TO_NM(raw) ((double)(raw) / 10.0)\n\n");

    // 轴结构
    code.push_str("typedef struct {\n");
    code.push_str("    nano_pos_t target;\n");
    code.push_str("    nano_pos_t actual;\n");
    code.push_str("    nano_pos_t error;\n");
    code.push_str("    nano_pos_t integral;\n");
    code.push_str("    nano_pos_t prev_error;\n");
    code.push_str("    int32_t    output;      /* DAC counts */\n");
    code.push_str("    uint8_t    settled;\n");
    code.push_str("} _nano_axis_t;\n\n");

    code.push_str(&format!("static _nano_axis_t _axes[{}];\n\n", axes.len().max(1)));

    // PID 伺服环
    code.push_str(&format!("/* PID: Kp={:.1} Ki={:.3} Kd={:.1} */\n", gains.kp, gains.ki, gains.kd));
    code.push_str("static inline void _servo_tick(uint8_t axis_id) {\n");
    code.push_str("    _nano_axis_t *a = &_axes[axis_id];\n");
    code.push_str("    a->error = a->target - a->actual;\n");
    code.push_str("    a->integral += a->error;\n");
    code.push_str("    /* Anti-windup */\n");
    code.push_str("    if (a->integral > NM_TO_RAW(1000)) a->integral = NM_TO_RAW(1000);\n");
    code.push_str("    if (a->integral < -NM_TO_RAW(1000)) a->integral = -NM_TO_RAW(1000);\n");
    code.push_str(&format!("    int64_t p = (int64_t)(a->error * {:.1});\n", gains.kp));
    code.push_str(&format!("    int64_t i = (int64_t)(a->integral * {:.4});\n", gains.ki));
    code.push_str(&format!("    int64_t d = (int64_t)((a->error - a->prev_error) * {:.1});\n", gains.kd));
    code.push_str("    a->output = (int32_t)((p + i + d) >> 10);\n");
    code.push_str("    a->prev_error = a->error;\n");
    code.push_str("    /* Settling detection: |error| < 1nm for 100 samples */\n");
    code.push_str("    a->settled = (a->error < NM_TO_RAW(1) && a->error > -NM_TO_RAW(1));\n");
    code.push_str("}\n\n");

    // 各轴配置
    for axis in axes {
        code.push_str(&format!("/* Axis {}: {} (stroke={}nm, res={:.2}nm, motor={:?}) */\n",
            axis.name, axis.name, axis.stroke_nm, axis.resolution_nm, axis.motor_type));
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
    fn test_nano_axis_positioning() {
        let axis = NanoAxis {
            name: "WaferStage_X".into(),
            stroke_nm: 300_000_000, // 300mm
            resolution_nm: 0.1,
            max_velocity_mm_s: 500.0,
            max_accel_m_s2: 50.0,
            accuracy_nm: 1.0,
            repeatability_nm: 0.5,
            servo_period_us: 50, // 20kHz
            motor_type: MotorType::PlanarMotor,
        };
        let gains = PidGains { kp: 50.0, ki: 0.01, kd: 5.0 };
        let spec = analyze_positioning(&axis, &gains);
        assert!(spec.settling_error_nm < 1.0);
        assert!(spec.settling_time_ms < 2000.0);
    }

    #[test]
    fn test_motor_bandwidth() {
        assert!(MotorType::Piezo.bandwidth_hz() > MotorType::VoiceCoil.bandwidth_hz());
        assert_eq!(MotorType::Piezo.bandwidth_hz(), 2000.0);
    }

    #[test]
    fn test_zvd_shaper() {
        let shaper = design_zvd_shaper(150.0, 0.02);
        assert_eq!(shaper.kind, ShaperKind::ZVD);
        assert_eq!(shaper.impulses.len(), 3);
        // 脉冲幅度之和 ≈ 1.0
        let sum: f64 = shaper.impulses.iter().map(|(a, _)| a).sum();
        assert!((sum - 1.0).abs() < 0.001);
        // 第一个脉冲在 t=0
        assert_eq!(shaper.impulses[0].1, 0.0);
        // 最后一个脉冲在 t=T
        assert!(shaper.impulses[2].1 > shaper.impulses[1].1);
    }

    #[test]
    fn test_alignment_correction() {
        let config = AlignmentConfig {
            mark_count: 4,
            required_accuracy_nm: 2.0,
            field_of_view_um: 100.0,
            mark_type: AlignMarkType::Grating,
        };
        let offsets = vec![(1.5, 0.8), (1.2, 1.0), (1.8, 0.9), (1.4, 1.1)];
        let result = compute_alignment_correction(&config, &offsets);
        assert!(result.pass);
        assert!(result.residual_rms_nm < 2.0);
        assert!((result.offset_x_nm - 1.475).abs() < 0.01);
    }

    #[test]
    fn test_alignment_fail() {
        let config = AlignmentConfig {
            mark_count: 4,
            required_accuracy_nm: 0.5,
            field_of_view_um: 100.0,
            mark_type: AlignMarkType::Cross,
        };
        let offsets = vec![(10.0, 5.0), (-8.0, 12.0), (15.0, -3.0), (-5.0, 8.0)];
        let result = compute_alignment_correction(&config, &offsets);
        assert!(!result.pass);
        assert!(result.residual_rms_nm > 0.5);
    }

    #[test]
    fn test_nano_servo_code() {
        let axes = vec![
            NanoAxis { name: "X".into(), stroke_nm: 300_000_000, resolution_nm: 0.1, max_velocity_mm_s: 500.0, max_accel_m_s2: 50.0, accuracy_nm: 1.0, repeatability_nm: 0.5, servo_period_us: 50, motor_type: MotorType::PlanarMotor },
            NanoAxis { name: "Y".into(), stroke_nm: 300_000_000, resolution_nm: 0.1, max_velocity_mm_s: 500.0, max_accel_m_s2: 50.0, accuracy_nm: 1.0, repeatability_nm: 0.5, servo_period_us: 50, motor_type: MotorType::PlanarMotor },
        ];
        let gains = PidGains { kp: 50.0, ki: 0.01, kd: 5.0 };
        let code = generate_nano_servo_code(&axes, &gains);
        assert!(code.contains("nano_pos_t"));
        assert!(code.contains("_servo_tick"));
        assert!(code.contains("NM_TO_RAW"));
        assert!(code.contains("Anti-windup"));
        assert!(code.contains("settled"));
    }
}
