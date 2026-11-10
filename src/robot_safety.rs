//! 执行器安全护栏（DUANYAN v1.6 P1）
//!
//! 移植自 DuanYanX v1.5 `src/safety_policy.rs`（单一真值口径）：把「臂/关节还能不能动」
//! 收拢成单一策略点——总线回读（负载/温度/堵转）→ `evaluate()` 出动作
//! （TorqueOff/Stop/AlarmLed/EscalateHost）。数值/优先级/迟滞与 daemon 侧完全一致
//! （见测试断言），防两端漂移。差异仅：`EscalatePi`→`EscalateHost`（去项目专名）。
//!
//! 优先级：temp > stall > load（过热最伤硬件；堵转次之；负载高只是「别硬拉」）。
//! 迟滞：故障后需连续 NORMAL_RECOVER 次正常读数才解除告警态，防抖动式误恢复。
//!
//! **与 `robotics.rs` 的关系（互补，非替代）**：
//! - `robotics::safety_check` = ISO/TS 15066 **人机碰撞**层（力/速/压强，协作臂标准，domains 门禁）。
//! - `robot_safety`（本模块）= **执行器**层（负载/温度/堵转，总线舵机保护，无门禁）。
//! 真机固件默认带本层护栏；重工业协作场景可叠加 robotics 层。
//!
//! 诚实边界：本期不自动断电源域（EscalateHost = 落日志/上报，人来决断）。

/// 连续正常读数达到此值才从故障态恢复（迟滞防抖）
pub const NORMAL_RECOVER: u8 = 3;
/// 状态表覆盖的最大总线 id 数（ZX361 id 0~15 足够；超界 id 走无状态判定）
const MAX_TRACKED: usize = 16;

/// 安全阈值（总线域量纲：load 为 ZX361 回读负载原始值，temp 为摄氏度）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SafetyLimits {
    /// 负载上限：超过 → Stop（别再硬拉，允许保持力矩）
    pub load_max: u16,
    /// 温度上限（℃）：超过 → TorqueOff（过热最伤，卸力矩散热）
    pub temp_max_c: f32,
    /// 堵转时长上限（ms）：命令后该时长内位置不回一致 → Stop + AlarmLed
    pub stall_ms: u32,
}

impl Default for SafetyLimits {
    /// 缺省阈值：ZX361 手册未给负载量纲定标，先保守取经验值，
    /// bring-up 实测回读分布后再校（与 DUANYAN 同值）。
    fn default() -> Self {
        Self { load_max: 900, temp_max_c: 65.0, stall_ms: 3000 }
    }
}

/// 安全动作（按严重度递减）
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SafetyAction {
    /// 一切正常，放行
    None,
    /// 卸力矩：透传 `#<id>PULK!`（过热保护，最高级）
    TorqueOff(u8),
    /// 停止运动保持力矩：透传 `#<id>PDST!`（堵转/过载）
    Stop(u8),
    /// 告警灯
    AlarmLed,
    /// 上报宿主：日志/CSV（不自动断电源域）
    EscalateHost,
}

/// 单 id 故障状态：faulted + 连续正常计数（迟滞恢复）
#[derive(Debug, Clone, Copy)]
struct IdState {
    faulted: bool,
    normal_run: u8,
}

/// 安全策略器：持阈值 + 每 id 迟滞状态（定长表）
pub struct SafetyPolicy {
    pub limits: SafetyLimits,
    states: [IdState; MAX_TRACKED],
}

impl SafetyPolicy {
    pub fn new(limits: SafetyLimits) -> Self {
        Self {
            limits,
            states: [IdState { faulted: false, normal_run: 0 }; MAX_TRACKED],
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(SafetyLimits::default())
    }

    /// 该 id 是否处于故障告警态
    pub fn is_faulted(&self, id: u8) -> bool {
        self.states
            .get(id as usize)
            .map(|s| s.faulted)
            .unwrap_or(false)
    }

    /// 综合判定（无状态部分）：优先级 temp > stall > load。
    /// stall_ms == 0 表示「无堵转证据」（u32::MAX 视为未观测 → 跳过该项）。
    pub fn evaluate(
        &self,
        id: u8,
        load: u16,
        temp_c: f32,
        stall_ms: u32,
        lim: &SafetyLimits,
    ) -> SafetyAction {
        if temp_c > lim.temp_max_c {
            return SafetyAction::TorqueOff(id);
        }
        if stall_ms != 0 && stall_ms != u32::MAX && stall_ms > lim.stall_ms {
            return SafetyAction::Stop(id);
        }
        if load > lim.load_max {
            return SafetyAction::Stop(id);
        }
        SafetyAction::None
    }

    /// 带迟滞的状态化判定：喂入一次回读样本，返回动作。
    /// 故障样本 → 置 faulted、连续正常清零、返回对应动作；
    /// 正常样本 → 若曾 faulted，需连续 NORMAL_RECOVER 次正常才解除，
    /// 解除前仍持续返回告警（EscalateHost）让上层保持警觉。
    pub fn check(&mut self, id: u8, load: u16, temp_c: f32, stall_ms: u32) -> SafetyAction {
        let act = self.evaluate(id, load, temp_c, stall_ms, &self.limits);
        let Some(st) = self.states.get_mut(id as usize) else {
            return act; // 超界 id：无状态直判（不落迟滞表）
        };
        match act {
            SafetyAction::None => {
                if st.faulted {
                    st.normal_run = st.normal_run.saturating_add(1);
                    if st.normal_run >= NORMAL_RECOVER {
                        st.faulted = false;
                        st.normal_run = 0;
                        SafetyAction::None
                    } else {
                        SafetyAction::EscalateHost // 恢复观察期：仍上报
                    }
                } else {
                    st.normal_run = 0;
                    SafetyAction::None
                }
            }
            _ => {
                st.faulted = true;
                st.normal_run = 0;
                act
            }
        }
    }

    /// 强制解除（维护命令用，如人工排除故障后）
    pub fn clear(&mut self, id: u8) {
        if let Some(st) = self.states.get_mut(id as usize) {
            st.faulted = false;
            st.normal_run = 0;
        }
    }
}

/// 便捷函数：一次性无状态判定（单测/脚本式调用）
pub fn evaluate(
    id: u8,
    load: u16,
    temp_c: f32,
    stall_ms: u32,
    lim: &SafetyLimits,
) -> SafetyAction {
    SafetyPolicy::with_defaults().evaluate(id, load, temp_c, stall_ms, lim)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lim() -> SafetyLimits {
        SafetyLimits { load_max: 900, temp_max_c: 65.0, stall_ms: 3000 }
    }

    // 以下 6 例镜像 DuanYanX/src/safety_policy.rs 测试（EscalatePi→EscalateHost），数值一致
    #[test]
    fn test_normal_passes() {
        let p = SafetyPolicy::with_defaults();
        assert_eq!(p.evaluate(1, 300, 40.0, 1000, &lim()), SafetyAction::None);
        // 未观测堵转（u32::MAX / 0）不误判
        assert_eq!(p.evaluate(1, 300, 40.0, u32::MAX, &lim()), SafetyAction::None);
        assert_eq!(p.evaluate(1, 300, 40.0, 0, &lim()), SafetyAction::None);
    }

    #[test]
    fn test_priority_temp_over_stall_over_load() {
        let p = SafetyPolicy::with_defaults();
        // 三项全超 → 温度最高级
        assert_eq!(p.evaluate(2, 1200, 80.0, 5000, &lim()), SafetyAction::TorqueOff(2));
        // 负载+堵转超、温度正常 → Stop
        assert_eq!(p.evaluate(2, 1200, 40.0, 5000, &lim()), SafetyAction::Stop(2));
        // 仅负载超 → Stop
        assert_eq!(p.evaluate(2, 901, 40.0, 0, &lim()), SafetyAction::Stop(2));
        // 仅堵转超 → Stop
        assert_eq!(p.evaluate(2, 100, 40.0, 3001, &lim()), SafetyAction::Stop(2));
    }

    #[test]
    fn test_boundary_values_pass() {
        let p = SafetyPolicy::with_defaults();
        // 恰好等于阈值 = 未超（> 判定）
        assert_eq!(p.evaluate(1, 900, 65.0, 3000, &lim()), SafetyAction::None);
    }

    #[test]
    fn test_hysteresis_recover_after_3_normals() {
        let mut p = SafetyPolicy::with_defaults();
        // 故障：置 faulted
        assert_eq!(p.check(3, 1200, 40.0, 0), SafetyAction::Stop(3));
        assert!(p.is_faulted(3));
        // 恢复观察期：前 2 次正常仍 EscalateHost
        assert_eq!(p.check(3, 100, 40.0, 0), SafetyAction::EscalateHost);
        assert_eq!(p.check(3, 100, 40.0, 0), SafetyAction::EscalateHost);
        assert!(p.is_faulted(3));
        // 第 3 次正常 → 解除
        assert_eq!(p.check(3, 100, 40.0, 0), SafetyAction::None);
        assert!(!p.is_faulted(3));
    }

    #[test]
    fn test_fault_during_recovery_resets_counter() {
        let mut p = SafetyPolicy::with_defaults();
        assert_eq!(p.check(4, 1200, 40.0, 0), SafetyAction::Stop(4));
        assert_eq!(p.check(4, 100, 40.0, 0), SafetyAction::EscalateHost); // normal_run=1
        // 抖动：又来一次故障 → 计数清零重来
        assert_eq!(p.check(4, 1200, 40.0, 0), SafetyAction::Stop(4));
        assert_eq!(p.check(4, 100, 40.0, 0), SafetyAction::EscalateHost);
        assert_eq!(p.check(4, 100, 40.0, 0), SafetyAction::EscalateHost);
        assert_eq!(p.check(4, 100, 40.0, 0), SafetyAction::None);
        assert!(!p.is_faulted(4));
    }

    #[test]
    fn test_ids_independent_and_clear() {
        let mut p = SafetyPolicy::with_defaults();
        assert_eq!(p.check(5, 1200, 40.0, 0), SafetyAction::Stop(5));
        assert!(p.is_faulted(5));
        assert!(!p.is_faulted(6)); // 各 id 独立
        p.clear(5);
        assert!(!p.is_faulted(5)); // 维护命令强制解除
        // 超界 id：无状态直判不落表
        assert_eq!(p.check(200, 1200, 40.0, 0), SafetyAction::Stop(200));
        assert!(!p.is_faulted(200));
    }
}
