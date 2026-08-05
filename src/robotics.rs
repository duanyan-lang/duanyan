//! Phase RB: 具身机器人垂直模块
//!
//! 面向协作机械臂 / 人形机器人 / 四足机器人：
//! - 正逆运动学 (DH 参数 / Jacobian)
//! - 力/位混合控制 (阻抗控制 / 导纳控制)
//! - 多传感器融合 (IMU + 编码器 + 力矩传感器)
//! - 轨迹规划 (五次多项式 / 梯形速度)
//! - 安全协作 (ISO 10218 / ISO/TS 15066)

// =============================================================================
// 运动学
// =============================================================================

/// DH 参数 (Denavit-Hartenberg)
#[derive(Debug, Clone)]
pub struct DHParam {
    /// 连杆扭转角 α (rad)
    pub alpha: f64,
    /// 连杆长度 a (mm)
    pub a: f64,
    /// 连杆偏距 d (mm)
    pub d: f64,
    /// 关节角 θ 偏移 (rad)
    pub theta_offset: f64,
    /// 关节类型
    pub joint_type: JointType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JointType {
    /// 旋转关节
    Revolute,
    /// 棱柱关节
    Prismatic,
}

/// 关节空间状态
#[derive(Debug, Clone)]
pub struct JointState {
    /// 关节角度 (rad) 或位移 (mm)
    pub positions: Vec<f64>,
    /// 关节速度 (rad/s 或 mm/s)
    pub velocities: Vec<f64>,
    /// 关节力矩 (Nm 或 N)
    pub torques: Vec<f64>,
}

/// 笛卡尔空间位姿
#[derive(Debug, Clone)]
pub struct CartesianPose {
    /// 位置 (x, y, z) mm
    pub position: [f64; 3],
    /// 姿态 (四元数 w, x, y, z)
    pub orientation: [f64; 4],
}

/// 正运动学: 关节角 → 末端位姿
pub fn forward_kinematics(dh_params: &[DHParam], joint_angles: &[f64]) -> CartesianPose {
    // 齐次变换矩阵累乘 (简化: 只计算位置)
    let mut x = 0.0f64;
    let mut y = 0.0f64;
    let mut z = 0.0f64;
    let mut cum_angle = 0.0f64;

    for (i, dh) in dh_params.iter().enumerate() {
        let theta = if dh.joint_type == JointType::Revolute {
            joint_angles.get(i).copied().unwrap_or(0.0) + dh.theta_offset
        } else {
            dh.theta_offset
        };
        let d = if dh.joint_type == JointType::Prismatic {
            joint_angles.get(i).copied().unwrap_or(0.0) + dh.d
        } else {
            dh.d
        };

        cum_angle += theta;
        x += dh.a * cum_angle.cos();
        y += dh.a * cum_angle.sin();
        z += d;
    }

    // 简化姿态: 绕 Z 轴旋转
    let half = cum_angle / 2.0;
    CartesianPose {
        position: [x, y, z],
        orientation: [half.cos(), 0.0, 0.0, half.sin()],
    }
}

/// Jacobian 矩阵 (数值微分)
pub fn compute_jacobian(dh_params: &[DHParam], joint_angles: &[f64], delta: f64) -> Vec<Vec<f64>> {
    let n = joint_angles.len();
    let mut jacobian = vec![vec![0.0f64; n]; 6]; // 6 x n

    let base = forward_kinematics(dh_params, joint_angles);

    for j in 0..n {
        let mut perturbed = joint_angles.to_vec();
        perturbed[j] += delta;
        let fk = forward_kinematics(dh_params, &perturbed);

        // 位置部分
        jacobian[0][j] = (fk.position[0] - base.position[0]) / delta;
        jacobian[1][j] = (fk.position[1] - base.position[1]) / delta;
        jacobian[2][j] = (fk.position[2] - base.position[2]) / delta;
        // 姿态部分 (简化: 四元数差)
        jacobian[3][j] = (fk.orientation[0] - base.orientation[0]) / delta;
        jacobian[4][j] = (fk.orientation[1] - base.orientation[1]) / delta;
        jacobian[5][j] = (fk.orientation[2] - base.orientation[2]) / delta;
    }

    jacobian
}

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
// 轨迹规划
// =============================================================================

/// 五次多项式轨迹 (quintic)
#[derive(Debug, Clone)]
pub struct QuinticTrajectory {
    /// 系数 [a0, a1, a2, a3, a4, a5]
    pub coeffs: [f64; 6],
    /// 总时间 (s)
    pub duration_s: f64,
}

/// 规划五次多项式: q(0)=q0, q(T)=qf, 速度/加速度边界为零
pub fn plan_quintic(q0: f64, qf: f64, t_total: f64) -> QuinticTrajectory {
    let t2 = t_total * t_total;
    let t3 = t2 * t_total;
    let t4 = t3 * t_total;
    let t5 = t4 * t_total;

    let a0 = q0;
    let a1 = 0.0; // v(0) = 0
    let a2 = 0.0; // a(0) = 0
    let a3 = 10.0 * (qf - q0) / t3;
    let a4 = -15.0 * (qf - q0) / t4;
    let a5 = 6.0 * (qf - q0) / t5;

    QuinticTrajectory {
        coeffs: [a0, a1, a2, a3, a4, a5],
        duration_s: t_total,
    }
}

/// 采样轨迹点 (位置, 速度, 加速度)
pub fn sample_quintic(traj: &QuinticTrajectory, t: f64) -> (f64, f64, f64) {
    let [a0, a1, a2, a3, a4, a5] = traj.coeffs;
    let t2 = t * t;
    let t3 = t2 * t;
    let t4 = t3 * t;
    let t5 = t4 * t;

    let pos = a0 + a1 * t + a2 * t2 + a3 * t3 + a4 * t4 + a5 * t5;
    let vel = a1 + 2.0 * a2 * t + 3.0 * a3 * t2 + 4.0 * a4 * t3 + 5.0 * a5 * t4;
    let acc = 2.0 * a2 + 6.0 * a3 * t + 12.0 * a4 * t2 + 20.0 * a5 * t3;

    (pos, vel, acc)
}

/// 梯形速度规划
#[derive(Debug, Clone)]
pub struct TrapezoidalProfile {
    /// 加速时间 (s)
    pub t_accel: f64,
    /// 匀速时间 (s)
    pub t_const: f64,
    /// 减速时间 (s)
    pub t_decel: f64,
    /// 最大速度
    pub v_max: f64,
    /// 加速度
    pub accel: f64,
    /// 总位移
    pub distance: f64,
}

/// 规划梯形速度曲线
pub fn plan_trapezoidal(distance: f64, v_max: f64, a_max: f64) -> TrapezoidalProfile {
    let t_accel = v_max / a_max;
    let d_accel = 0.5 * a_max * t_accel * t_accel;

    if 2.0 * d_accel >= distance {
        // 三角形 (达不到最大速度)
        let t = (distance / a_max).sqrt();
        TrapezoidalProfile {
            t_accel: t,
            t_const: 0.0,
            t_decel: t,
            v_max: a_max * t,
            accel: a_max,
            distance,
        }
    } else {
        let d_const = distance - 2.0 * d_accel;
        let t_const = d_const / v_max;
        TrapezoidalProfile {
            t_accel,
            t_const,
            t_decel: t_accel,
            v_max,
            accel: a_max,
            distance,
        }
    }
}

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
    code.push_str(&format!("/* HardyScript RB: Embodied Robot Control — {} DOF */\n\n", dof));

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
