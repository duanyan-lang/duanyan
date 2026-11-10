//! 机器人纯运动学/轨迹数学（DUANYAN v1.6 P2）
//!
//! 从 `robotics.rs`（domains 门禁）下沉的**纯数学单一真源**：DH 正运动学、数值 Jacobian、
//! 五次多项式轨迹、梯形速度规划。下沉后：
//! - **默认构建（无 domains）即可用/可测**，供 `duanyan:robot` 编译器路径（`Traj.*`/`Joint.glide`）复用；
//! - `robotics.rs`（domains）改为 `pub use crate::robot_motion::*`，只保留阻抗/ISO 15066/传感融合等垂直扩展。
//!
//! 语义与原 `robotics.rs` 逐位一致（见测试断言），防下沉回归。

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

#[cfg(test)]
mod tests {
    use super::*;

    // 镜像原 robotics.rs 纯数学测试，防下沉回归
    #[test]
    fn test_forward_kinematics() {
        let dh = vec![
            DHParam { alpha: 0.0, a: 100.0, d: 0.0, theta_offset: 0.0, joint_type: JointType::Revolute },
            DHParam { alpha: 0.0, a: 80.0, d: 0.0, theta_offset: 0.0, joint_type: JointType::Revolute },
        ];
        let pose = forward_kinematics(&dh, &[0.0, 0.0]);
        assert!((pose.position[0] - 180.0).abs() < 0.01);
        assert!(pose.position[1].abs() < 0.01);

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
        assert!((jac[1][0] - 180.0).abs() < 1.0);
    }

    #[test]
    fn test_quintic_trajectory() {
        let traj = plan_quintic(0.0, 1.0, 2.0);
        let (pos, vel, _acc) = sample_quintic(&traj, 0.0);
        assert!(pos.abs() < 1e-10);
        assert!(vel.abs() < 1e-10);
        let (pos, vel, _acc) = sample_quintic(&traj, 2.0);
        assert!((pos - 1.0).abs() < 1e-6);
        assert!(vel.abs() < 1e-6);
        let (pos, _, _) = sample_quintic(&traj, 1.0);
        assert!((pos - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_trapezoidal_profile() {
        let profile = plan_trapezoidal(100.0, 50.0, 100.0);
        assert!(profile.t_const > 0.0);
        let total_time = profile.t_accel + profile.t_const + profile.t_decel;
        assert!(total_time > 2.0);
    }

    #[test]
    fn test_trapezoidal_triangular() {
        let profile = plan_trapezoidal(1.0, 50.0, 100.0);
        assert_eq!(profile.t_const, 0.0);
        assert!(profile.v_max < 50.0);
    }
}
