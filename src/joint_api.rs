//! 关节抽象 API（DUANYAN v1.6 P0）
//!
//! 镜像 DuanYanX v1.5 `src/joint_api.rs`（单一真值口径）：「统一入口是关节而非引脚，
//! 每关节独立绑定后端，PWM/总线可混搭」。上位机只见关节号与角度；PCA9685 通道号 /
//! ZX361 舵机 ID / 总线脉宽换算全部由绑定表吸收。
//!
//! 与 DuanYanX 的差异（宿主适配，语义不变）：
//! - `JointSpec.name` 用 `String`（DUANYAN 是 std 宿主，非 no_std `&'static str`）。
//! - DuanYanX 的 `set_deg`/`read_frame` 直接产 `mode_b_proto::Frame`；DUANYAN P0 阶段
//!   `modeb_proto` 尚未落地（P3），故改为产**路由意图** `JointRoute`/`ReadRoute`，
//!   把「PWM→0x20 静态定位载荷」「BUS→0x30 同步项 (id,pwm,ms)」「PWM→0x21 查询」
//!   「BUS→0x31 ASCII 透传」显式表达。数值与 DuanYanX 完全一致（见测试断言），
//!   P3 的 `modeb_proto` 直接消费这些路由意图组帧即可，无需再改本模块。
//!
//! 回读语义统一：
//! - PWM 后端 → 0x21 的 hw 脉冲真值；负载/温度 = 不可用
//! - 总线后端 → 位置/负载/温度真值（ZX361 原生回读）
//!
//! 颈 as-built 绑定（2026-09-15 定案）：J0..J3 = pan/tilt/nod/roll → PCA CH0..CH3。

use crate::bus_zx361;

/// 关节号空间上限（颈 4 + 双臂 12 = 16，三臂满配）
pub const JOINT_API_MAX: usize = 16;

/// 执行器后端
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Backend {
    /// PCA9685 PWM 通道（channel = 物理通道号，仅 as-built 记录用；线上帧走关节号）
    PwmPca9685 { channel: u8 },
    /// 众灵 ZX361 总线舵机（id = 菊花链舵机 ID）
    BusZx361 { id: u8 },
}

/// 回读语义类别
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReadbackKind {
    /// PWM：hw 脉冲真值 + 负载/温度不可用
    PwmHw,
    /// 总线：位置/负载/温度真值
    BusPosLoadTemp,
}

/// 关节规格：名字 + 后端 + 角度包络 + 归中位
#[derive(Debug, Clone, PartialEq)]
pub struct JointSpec {
    pub name: String,
    pub backend: Backend,
    pub min_deg: f32,
    pub max_deg: f32,
    pub home_deg: f32,
}

/// 一次关节指令（关节号 + 目标角 + 行程时间 ms）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointCmd {
    pub joint: u8,
    pub deg: f32,
    pub ms: u16,
}

/// 关节写命令的路由意图（P0 自包含表达，P3 modeb_proto 据此组帧）
#[derive(Debug, Clone, PartialEq)]
pub enum JointRoute {
    /// PWM 后端 → 0x20 静态定位：关节号 + 角度×10（a10 = (deg*10).round()）
    PwmSet { joint: u8, deg_x10: u16 },
    /// 总线后端 → 0x30 同步包：项为 (id, pwm, ms)
    BusSync { items: Vec<(u8, u16, u16)> },
}

/// 关节回读的路由意图
#[derive(Debug, Clone, PartialEq)]
pub enum ReadRoute {
    /// PWM 后端 → 0x21 joint_query(关节号)
    PwmQuery { joint: u8 },
    /// 总线后端 → 0x31 透传 ASCII 查询串（如 `#002PRAD!`）
    BusAscii { ascii: String },
}

/// 关节 API：绑定表 + 路由 + 换算
#[derive(Debug, Clone)]
pub struct JointApi {
    joints: [Option<JointSpec>; JOINT_API_MAX],
}

impl JointApi {
    pub fn new() -> Self {
        Self { joints: std::array::from_fn(|_| None) }
    }

    /// 颈四关节 as-built 绑定（2026-09-15 定案 + 包络实证值）
    pub fn neck_as_built() -> Self {
        let mut a = Self::new();
        a.bind(0, JointSpec { name: "pan".into(),  backend: Backend::PwmPca9685 { channel: 0 },
                             min_deg: 30.0, max_deg: 150.0, home_deg: 90.0 });
        a.bind(1, JointSpec { name: "tilt".into(), backend: Backend::PwmPca9685 { channel: 1 },
                             min_deg: 60.0, max_deg: 125.0, home_deg: 90.0 });
        a.bind(2, JointSpec { name: "nod".into(),  backend: Backend::PwmPca9685 { channel: 2 },
                             min_deg: 45.0, max_deg: 135.0, home_deg: 90.0 });
        a.bind(3, JointSpec { name: "roll".into(), backend: Backend::PwmPca9685 { channel: 3 },
                             min_deg: 20.0, max_deg: 160.0, home_deg: 90.0 });
        a
    }

    /// 绑定关节；非法关节号/倒置包络拒绑
    pub fn bind(&mut self, joint: u8, spec: JointSpec) -> bool {
        if joint as usize >= JOINT_API_MAX || spec.min_deg >= spec.max_deg {
            return false;
        }
        self.joints[joint as usize] = Some(spec);
        true
    }

    pub fn spec(&self, joint: u8) -> Option<&JointSpec> {
        self.joints.get(joint as usize)?.as_ref()
    }

    /// 目标角钳位到包络；未绑定关节返回 None（上层据此跳过而非误动）
    pub fn clamp_deg(&self, joint: u8, deg: f32) -> Option<f32> {
        let s = self.spec(joint)?;
        Some(deg.clamp(s.min_deg, s.max_deg))
    }

    /// 角度 → ZX361 脉宽：包络线性映射到 500..2500（先钳位再换算）
    pub fn deg_to_bus_pwm(deg: f32, s: &JointSpec) -> u16 {
        let d = deg.clamp(s.min_deg, s.max_deg);
        let f = (d - s.min_deg) / (s.max_deg - s.min_deg);
        let span = (bus_zx361::BUS_PWM_MAX - bus_zx361::BUS_PWM_MIN) as f32;
        bus_zx361::BUS_PWM_MIN + (f * span + 0.5) as u16
    }

    pub fn readback_kind(&self, joint: u8) -> Option<ReadbackKind> {
        Some(match self.spec(joint)?.backend {
            Backend::PwmPca9685 { .. } => ReadbackKind::PwmHw,
            Backend::BusZx361 { .. } => ReadbackKind::BusPosLoadTemp,
        })
    }

    /// 单关节目标角命令路由：
    /// - PWM → PwmSet（a10 = (deg*10).round()，对应 0x20 静态定位载荷）
    /// - BUS → BusSync 单项（id + deg_to_bus_pwm + ms，对应 0x30）
    pub fn set_deg(&self, joint: u8, deg: f32, ms: u16) -> Option<JointRoute> {
        let s = self.spec(joint)?;
        let d = deg.clamp(s.min_deg, s.max_deg);
        match s.backend {
            Backend::PwmPca9685 { .. } => {
                let a10 = (d * 10.0).round() as u16;
                Some(JointRoute::PwmSet { joint, deg_x10: a10 })
            }
            Backend::BusZx361 { id } => Some(JointRoute::BusSync {
                items: vec![(id, Self::deg_to_bus_pwm(d, s), ms)],
            }),
        }
    }

    /// 多关节位姿：总线关节合并进 BusSync（满 8 项切包，ZX361 `{G}` 语义），
    /// PWM 关节逐关节 PwmSet
    pub fn set_pose(&self, cmds: &[JointCmd]) -> Vec<JointRoute> {
        let mut routes = Vec::new();
        let mut bus_items: Vec<(u8, u16, u16)> = Vec::new();
        for c in cmds {
            let s = match self.spec(c.joint) { Some(s) => s, None => continue };
            match s.backend {
                Backend::PwmPca9685 { .. } => {
                    if let Some(r) = self.set_deg(c.joint, c.deg, c.ms) {
                        routes.push(r);
                    }
                }
                Backend::BusZx361 { id } => {
                    let d = c.deg.clamp(s.min_deg, s.max_deg);
                    bus_items.push((id, Self::deg_to_bus_pwm(d, s), c.ms));
                    if bus_items.len() >= 8 {
                        routes.push(JointRoute::BusSync { items: bus_items.clone() });
                        bus_items.clear();
                    }
                }
            }
        }
        if !bus_items.is_empty() {
            routes.push(JointRoute::BusSync { items: bus_items });
        }
        routes
    }

    /// 回读路由：PWM → PwmQuery(0x21)；BUS → BusAscii(0x31 透传 `#idPRAD!`)
    pub fn read_frame(&self, joint: u8) -> Option<ReadRoute> {
        let s = self.spec(joint)?;
        match s.backend {
            Backend::PwmPca9685 { .. } => Some(ReadRoute::PwmQuery { joint }),
            Backend::BusZx361 { id } => Some(ReadRoute::BusAscii {
                ascii: bus_zx361::query_cmd(id, bus_zx361::Query::Pos),
            }),
        }
    }
}

impl Default for JointApi {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bus_spec() -> JointSpec {
        JointSpec { name: "shoulder".into(), backend: Backend::BusZx361 { id: 2 },
                    min_deg: 0.0, max_deg: 270.0, home_deg: 135.0 }
    }

    // 以下 7 例镜像 DuanYanX/src/joint_api.rs 测试，数值断言一致（Frame→JointRoute 表达差异）
    #[test]
    fn test_neck_as_built_bindings() {
        let a = JointApi::neck_as_built();
        assert_eq!(a.spec(0).unwrap().name, "pan");
        assert_eq!(a.spec(3).unwrap().backend, Backend::PwmPca9685 { channel: 3 });
        assert!(a.spec(4).is_none());
        assert_eq!(a.readback_kind(1), Some(ReadbackKind::PwmHw));
    }

    #[test]
    fn test_bind_rejects_bad() {
        let mut a = JointApi::new();
        assert!(!a.bind(16, bus_spec()));                    // 关节号越界
        let bad = JointSpec { name: "x".into(), backend: Backend::BusZx361 { id: 1 },
                              min_deg: 100.0, max_deg: 10.0, home_deg: 50.0 };
        assert!(!a.bind(0, bad));                            // 包络倒置
        assert!(a.bind(0, bus_spec()));
    }

    #[test]
    fn test_clamp_deg() {
        let a = JointApi::neck_as_built();
        assert_eq!(a.clamp_deg(0, 200.0), Some(150.0));
        assert_eq!(a.clamp_deg(0, 10.0), Some(30.0));
        assert_eq!(a.clamp_deg(9, 90.0), None);              // 未绑定
    }

    #[test]
    fn test_deg_to_bus_pwm_endpoints() {
        let s = bus_spec();
        assert_eq!(JointApi::deg_to_bus_pwm(-50.0, &s), 500);   // 下钳
        assert_eq!(JointApi::deg_to_bus_pwm(999.0, &s), 2500);  // 上钳
        assert_eq!(JointApi::deg_to_bus_pwm(0.0, &s), 500);
        assert_eq!(JointApi::deg_to_bus_pwm(270.0, &s), 2500);
        assert_eq!(JointApi::deg_to_bus_pwm(135.0, &s), 1500);  // 中位=中值
    }

    #[test]
    fn test_set_deg_routes_backends() {
        let mut a = JointApi::neck_as_built();
        a.bind(8, bus_spec());
        // PWM：45° → a10 = 450（DUANYAN 断言 payload[1..3]==[0x01,0xC2]=450）
        match a.set_deg(0, 45.0, 0).unwrap() {
            JointRoute::PwmSet { joint, deg_x10 } => {
                assert_eq!(joint, 0);        // 关节号（非通道号）
                assert_eq!(deg_x10, 450);    // 45.0*10
            }
            _ => panic!("PWM 关节应路由 PwmSet"),
        }
        // BUS：135°/500ms → id=2, pwm=1500, ms=500
        match a.set_deg(8, 135.0, 500).unwrap() {
            JointRoute::BusSync { items } => {
                assert_eq!(items, vec![(2, 1500, 500)]);
            }
            _ => panic!("BUS 关节应路由 BusSync"),
        }
        assert!(a.set_deg(9, 90.0, 0).is_none());             // 未绑定
    }

    #[test]
    fn test_set_pose_mixed_backends() {
        let mut a = JointApi::neck_as_built();
        a.bind(8, bus_spec());
        let mut s2 = bus_spec();
        s2.backend = Backend::BusZx361 { id: 3 };
        a.bind(9, s2);
        let routes = a.set_pose(&[
            JointCmd { joint: 0, deg: 90.0, ms: 0 },
            JointCmd { joint: 8, deg: 0.0, ms: 300 },
            JointCmd { joint: 9, deg: 270.0, ms: 300 },
            JointCmd { joint: 77, deg: 90.0, ms: 0 },        // 未绑定跳过
        ]);
        assert_eq!(routes.len(), 2);                          // 1×PwmSet + 1×BusSync(两项)
        assert!(matches!(routes[0], JointRoute::PwmSet { .. }));
        match &routes[1] {
            JointRoute::BusSync { items } => assert_eq!(items.len(), 2),
            _ => panic!("第二项应为 BusSync"),
        }
    }

    #[test]
    fn test_read_frame_routes() {
        let mut a = JointApi::neck_as_built();
        a.bind(8, bus_spec());
        assert_eq!(a.read_frame(1).unwrap(), ReadRoute::PwmQuery { joint: 1 });
        match a.read_frame(8).unwrap() {
            ReadRoute::BusAscii { ascii } => assert_eq!(ascii, "#002PRAD!"),
            _ => panic!("BUS 回读应为 BusAscii"),
        }
        assert_eq!(a.readback_kind(8), Some(ReadbackKind::BusPosLoadTemp));
    }
}
