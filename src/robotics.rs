//! Phase RB: 具身机器人垂直模块
//!
//! 面向协作机械臂 / 人形机器人 / 四足机器人：
//! - 正逆运动学 (DH 参数 / Jacobian)
//! - 力/位混合控制 (阻抗控制 / 导纳控制)
//! - 多传感器融合 (IMU + 编码器 + 力矩传感器)
//! - 轨迹规划 (五次多项式 / 梯形速度)
//! - 安全协作 (ISO 10218 / ISO/TS 15066)

// =============================================================================
// 运动学 + 轨迹规划（纯数学已下沉至 crate::robot_motion，无门禁单一真源）
// =============================================================================
// v1.6 P2：DHParam/forward_kinematics/compute_jacobian/plan_quintic/sample_quintic/
// plan_trapezoidal 等纯数学移至 robot_motion.rs（默认构建可用），此处再导出，
// 保持 robotics（domains）原有 API 不变；本模块只留阻抗/ISO 15066/传感融合等垂直扩展。
pub use crate::robot_motion::{
    DHParam, JointType, JointState, CartesianPose, forward_kinematics, compute_jacobian,
    QuinticTrajectory, plan_quintic, sample_quintic, TrapezoidalProfile, plan_trapezoidal,
};

// =============================================================================
// 力/位混合控制
// =============================================================================

/// 阻抗控制参数
#[derive(Debug, Clone)]
pub struct ImpedanceParams {
    /// 虚拟质量 (kg)
    pub mass: f64,
    /// 虚拟阻尼 (Ns/m)
    pub damping: f64,
    /// 虚拟刚度 (N/m)
    pub stiffness: f64,
}

/// 阻抗控制输出
#[derive(Debug, Clone)]
pub struct ImpedanceOutput {
    /// 期望力 (N)
    pub force_cmd: [f64; 3],
    /// 位置修正量 (mm)
    pub position_correction: [f64; 3],
    /// 是否稳定 (阻尼比 > 0)
    pub stable: bool,
}

/// 计算阻抗控制律: F = M*x_ddot + D*x_dot + K*(x - x_d)
pub fn impedance_control(
    params: &ImpedanceParams,
    pos_error: [f64; 3],
    vel_error: [f64; 3],
    accel: [f64; 3],
) -> ImpedanceOutput {
    let mut force = [0.0f64; 3];
    for i in 0..3 {
        force[i] = params.mass * accel[i] + params.damping * vel_error[i] + params.stiffness * pos_error[i];
    }

    // 稳定性: 阻尼比 ζ = D / (2*sqrt(M*K))
    let zeta = if params.mass > 0.0 && params.stiffness > 0.0 {
        params.damping / (2.0 * (params.mass * params.stiffness).sqrt())
    } else {
        0.0
    };

    ImpedanceOutput {
        force_cmd: force,
        position_correction: [
            force[0] / params.stiffness.max(0.001),
            force[1] / params.stiffness.max(0.001),
            force[2] / params.stiffness.max(0.001),
        ],
        stable: zeta > 0.0,
    }
}

// =============================================================================
// 轨迹规划（已下沉至 crate::robot_motion，见文件头 pub use 再导出）
// =============================================================================

// =============================================================================
// 多传感器融合
// =============================================================================

/// 传感器融合配置
#[derive(Debug, Clone)]
pub struct SensorFusionConfig {
    /// IMU 采样率 (Hz)
    pub imu_rate_hz: f64,
    /// 编码器分辨率 (counts/rev)
    pub encoder_resolution: u32,
    /// 力矩传感器量程 (Nm)
    pub torque_range_nm: f64,
    /// 互补滤波系数 α (0~1, 越大越信 IMU)
    pub complementary_alpha: f64,
}

/// 姿态估计结果
#[derive(Debug, Clone)]
pub struct AttitudeEstimate {
    /// Roll (rad)
    pub roll: f64,
    /// Pitch (rad)
    pub pitch: f64,
    /// Yaw (rad)
    pub yaw: f64,
    /// 置信度 (0~1)
    pub confidence: f64,
}

/// 互补滤波融合 (IMU 加速度计 + 陀螺仪)
pub fn complementary_filter(
    config: &SensorFusionConfig,
    gyro_rate: f64,      // rad/s
    accel_angle: f64,    // rad (from accelerometer)
    prev_angle: f64,     // rad
    dt: f64,             // s
) -> f64 {
    let alpha = config.complementary_alpha;
    alpha * (prev_angle + gyro_rate * dt) + (1.0 - alpha) * accel_angle
}

// =============================================================================
// 安全协作 (ISO/TS 15066)
// =============================================================================

/// 协作安全配置
#[derive(Debug, Clone)]
pub struct CollaborativeSafetyConfig {
    /// 最大允许力 (N, ISO/TS 15066 Table A.2)
    pub max_force_n: f64,
    /// 最大允许压强 (Pa)
    pub max_pressure_pa: f64,
    /// 最大允许速度 (mm/s)
    pub max_speed_mm_s: f64,
    /// 安全停止距离 (mm)
    pub safe_stop_distance_mm: f64,
    /// 功率/力限制模式
    pub mode: SafetyMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SafetyMode {
    /// 安全停止
    SafetyStop,
    /// 手动引导
    HandGuiding,
    /// 速度与间隔监控
    SpeedSeparation,
    /// 功率/力限制
    PowerForceLimit,
}

/// 安全监控结果
#[derive(Debug, Clone)]
pub struct SafetyMonitorResult {
    /// 当前力 (N)
    pub current_force_n: f64,
    /// 当前速度 (mm/s)
    pub current_speed_mm_s: f64,
    /// 是否违规
    pub violation: bool,
    /// 违规类型
    pub violation_type: Option<String>,
    /// 建议动作
    pub action: SafetyAction,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SafetyAction {
    /// 继续运行
    Continue,
    /// 降速
    ReduceSpeed,
    /// 停止
    Stop,
    /// 紧急停止
    EmergencyStop,
}

/// 安全监控检查
pub fn safety_check(config: &CollaborativeSafetyConfig, force_n: f64, speed_mm_s: f64) -> SafetyMonitorResult {
    let force_ratio = force_n / config.max_force_n;
    let speed_ratio = speed_mm_s / config.max_speed_mm_s;

    let (violation, violation_type, action) = if force_n > config.max_force_n || speed_mm_s > config.max_speed_mm_s {
        (true, Some("Force/Speed limit exceeded".into()), SafetyAction::EmergencyStop)
    } else if force_ratio > 0.8 || speed_ratio > 0.8 {
        (true, Some("Approaching limit (>80%)".into()), SafetyAction::ReduceSpeed)
    } else {
        (false, None, SafetyAction::Continue)
    };

    SafetyMonitorResult {
        current_force_n: force_n,
        current_speed_mm_s: speed_mm_s,
        violation,
        violation_type,
        action,
    }
}

// =============================================================================
// 代码生成
// =============================================================================

/// 生成机器人控制 C 代码
pub fn generate_robot_control_code(dof: usize, impedance: &ImpedanceParams, safety: &CollaborativeSafetyConfig) -> String {
    let mut code = String::new();
    code.push_str(&format!("/* DUANYAN RB: Embodied Robot Control — {} DOF */\n\n", dof));

    // 关节结构
    code.push_str(&format!("#define ROBOT_DOF {}\n\n", dof));
    code.push_str("typedef struct {\n");
    code.push_str("    double position[ROBOT_DOF];  /* rad */\n");
    code.push_str("    double velocity[ROBOT_DOF];  /* rad/s */\n");
    code.push_str("    double torque[ROBOT_DOF];    /* Nm */\n");
    code.push_str("} _joint_state_t;\n\n");

    // 阻抗控制
    code.push_str(&format!("/* Impedance: M={:.1}kg D={:.1}Ns/m K={:.1}N/m */\n", impedance.mass, impedance.damping, impedance.stiffness));
    code.push_str("typedef struct {\n");
    code.push_str("    double force_cmd[3];\n");
    code.push_str("    double pos_correction[3];\n");
    code.push_str("} _impedance_out_t;\n\n");

    code.push_str("static inline _impedance_out_t _impedance_ctrl(\n");
    code.push_str("    const double pos_err[3], const double vel_err[3], const double accel[3]) {\n");
    code.push_str("    _impedance_out_t out;\n");
    code.push_str("    for (int i = 0; i < 3; i++) {\n");
    code.push_str(&format!("        out.force_cmd[i] = {:.1} * accel[i] + {:.1} * vel_err[i] + {:.1} * pos_err[i];\n",
        impedance.mass, impedance.damping, impedance.stiffness));
    code.push_str(&format!("        out.pos_correction[i] = out.force_cmd[i] / {:.1};\n", impedance.stiffness.max(0.001)));
    code.push_str("    }\n");
    code.push_str("    return out;\n");
    code.push_str("}\n\n");

    // 安全监控
    code.push_str(&format!("/* Safety: F_max={:.0}N, v_max={:.0}mm/s (ISO/TS 15066) */\n", safety.max_force_n, safety.max_speed_mm_s));
    code.push_str("typedef enum { SAFE_CONTINUE, SAFE_REDUCE, SAFE_STOP, SAFE_ESTOP } _safety_action_t;\n\n");
    code.push_str("static _safety_action_t _safety_monitor(double force_n, double speed_mm_s) {\n");
    code.push_str(&format!("    if (force_n > {:.1} || speed_mm_s > {:.1}) return SAFE_ESTOP;\n", safety.max_force_n, safety.max_speed_mm_s));
    code.push_str(&format!("    if (force_n > {:.1} || speed_mm_s > {:.1}) return SAFE_REDUCE;\n",
        safety.max_force_n * 0.8, safety.max_speed_mm_s * 0.8));
    code.push_str("    return SAFE_CONTINUE;\n");
    code.push_str("}\n\n");

    // 控制环
    code.push_str("/* Main servo loop (1kHz) */\n");
    code.push_str("void _robot_servo_tick(_joint_state_t *state) {\n");
    code.push_str("    /* 1. Read encoders + force sensors */\n");
    code.push_str("    /* 2. Compute impedance control law */\n");
    code.push_str("    /* 3. Safety check */\n");
    code.push_str("    /* 4. Output torque commands */\n");
    code.push_str("}\n");

    code
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward_kinematics() {
        // 2-DOF planar arm: a1=100mm, a2=80mm
        let dh = vec![
            DHParam { alpha: 0.0, a: 100.0, d: 0.0, theta_offset: 0.0, joint_type: JointType::Revolute },
            DHParam { alpha: 0.0, a: 80.0, d: 0.0, theta_offset: 0.0, joint_type: JointType::Revolute },
        ];
        // θ1=0, θ2=0 → 末端在 (180, 0, 0)
        let pose = forward_kinematics(&dh, &[0.0, 0.0]);
        assert!((pose.position[0] - 180.0).abs() < 0.01);
        assert!(pose.position[1].abs() < 0.01);

        // θ1=π/2, θ2=0 → 末端在 (0, 180, 0)
        let pose2 = forward_kinematics(&dh, &[std::f64::consts::FRAC_PI_2, 0.0]);
        assert!(pose2.position[0].abs() < 0.01);
        assert!((pose2.position[1] - 180.0).abs() < 0.01);
    }

    #[test]
    fn test_jacobian() {
        let dh = vec![
            DHParam { alpha: 0.0, a: 100.0, d: 0.0, theta_offset: 0.0, joint_type: JointType::Revolute },
            DHParam { alpha: 0.0, a: 80.0, d: 0.0, theta_offset: 0.0, joint_type: JointType::Revolute },
        ];
        let jac = compute_jacobian(&dh, &[0.0, 0.0], 0.001);
        assert_eq!(jac.len(), 6);
        assert_eq!(jac[0].len(), 2);
        // J[1][0] ≈ 180 (dy/dθ1 at θ=0)
        assert!((jac[1][0] - 180.0).abs() < 1.0);
    }

    #[test]
    fn test_impedance_control() {
        let params = ImpedanceParams { mass: 2.0, damping: 20.0, stiffness: 100.0 };
        let out = impedance_control(&params, [0.01, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        // F = K * x_err = 100 * 0.01 = 1.0 N
        assert!((out.force_cmd[0] - 1.0).abs() < 0.001);
        assert!(out.stable);
    }

    #[test]
    fn test_quintic_trajectory() {
        let traj = plan_quintic(0.0, 1.0, 2.0);
        // t=0 → pos=0
        let (pos, vel, _acc) = sample_quintic(&traj, 0.0);
        assert!(pos.abs() < 1e-10);
        assert!(vel.abs() < 1e-10);
        // t=T → pos=1
        let (pos, vel, _acc) = sample_quintic(&traj, 2.0);
        assert!((pos - 1.0).abs() < 1e-6);
        assert!(vel.abs() < 1e-6);
        // t=T/2 → pos=0.5 (对称)
        let (pos, _, _) = sample_quintic(&traj, 1.0);
        assert!((pos - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_trapezoidal_profile() {
        let profile = plan_trapezoidal(100.0, 50.0, 100.0);
        assert!(profile.t_const > 0.0); // 能达到最大速度
        let total_time = profile.t_accel + profile.t_const + profile.t_decel;
        assert!(total_time > 2.0); // 至少 2 秒
    }

    #[test]
    fn test_trapezoidal_triangular() {
        // 短距离: 达不到最大速度
        let profile = plan_trapezoidal(1.0, 50.0, 100.0);
        assert_eq!(profile.t_const, 0.0);
        assert!(profile.v_max < 50.0);
    }

    #[test]
    fn test_complementary_filter() {
        let config = SensorFusionConfig {
            imu_rate_hz: 1000.0,
            encoder_resolution: 4096,
            torque_range_nm: 50.0,
            complementary_alpha: 0.98,
        };
        // 静止: 陀螺仪=0, 加速度计=0.1rad
        let angle = complementary_filter(&config, 0.0, 0.1, 0.0, 0.001);
        // 结果应接近 0 (因为 prev=0 且 α 大)
        assert!(angle < 0.01);
    }

    #[test]
    fn test_safety_check() {
        let config = CollaborativeSafetyConfig {
            max_force_n: 150.0,
            max_pressure_pa: 300_000.0,
            max_speed_mm_s: 250.0,
            safe_stop_distance_mm: 50.0,
            mode: SafetyMode::PowerForceLimit,
        };
        // 正常
        let r = safety_check(&config, 50.0, 100.0);
        assert!(!r.violation);
        assert_eq!(r.action, SafetyAction::Continue);

        // 接近限制
        let r = safety_check(&config, 130.0, 100.0);
        assert!(r.violation);
        assert_eq!(r.action, SafetyAction::ReduceSpeed);

        // 超限
        let r = safety_check(&config, 200.0, 100.0);
        assert!(r.violation);
        assert_eq!(r.action, SafetyAction::EmergencyStop);
    }

    #[test]
    fn test_robot_control_code() {
        let impedance = ImpedanceParams { mass: 2.0, damping: 20.0, stiffness: 100.0 };
        let safety = CollaborativeSafetyConfig {
            max_force_n: 150.0,
            max_pressure_pa: 300_000.0,
            max_speed_mm_s: 250.0,
            safe_stop_distance_mm: 50.0,
            mode: SafetyMode::PowerForceLimit,
        };
        let code = generate_robot_control_code(6, &impedance, &safety);
        assert!(code.contains("ROBOT_DOF 6"));
        assert!(code.contains("_impedance_ctrl"));
        assert!(code.contains("_safety_monitor"));
        assert!(code.contains("SAFE_ESTOP"));
        assert!(code.contains("ISO/TS 15066"));
    }
}
