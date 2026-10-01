//! 众灵 ZX361 单总线舵机 ASCII 帧构造/解析（HardyScript v1.6 P0）
//!
//! 移植自 DUANYAN v1.5 `src/bus_zx361.rs`（单一真值口径），差异仅在宿主：
//! DUANYAN 为 no_std（`extern crate alloc`），HardyScript 是 std 宿主，直接用 `std::string::String`。
//! 协议串构造/解析逻辑逐字节一致，测试断言与 DUANYAN 输出完全相同，防两端漂移。
//!
//! 协议源：《总线舵机说明书 V3.1》（杭州众灵）：单线半双工 115200 8N1。
//! - 控制帧 `#ID(3位)P PWM(4位)T 时间(4位)!`，如 `#001P1500T0500!`
//! - 多关节同步包 `{G0000<帧><帧>…}`（整臂位姿联动走这个，一包到全链）
//! - 回读 `#IDPRAD!`→`#IDP1500!`；`#IDPRTE!`→`#IDT<AD4>-<电压>!`
//! - 配置 `#IDPID<new3>!` 改 ID（**改 ID 时严禁串联**，手册第六条）；
//!   `ULK/ULR` 释力/恢复、`DST` 急停、`SMI/SMX` 软限位、`MOD1~8` 工作模式
//! - ID 默认 0，广播 255；供电 4.8~8.4V（推荐 7.4V）
//!
//! 本模块只造/析 ASCII 串，不碰 UART；线上承载走 mode-B 0x30/0x31（见 P3 `modeb_proto`）。

pub const BUS_PWM_MIN: u16 = 500;
pub const BUS_PWM_MAX: u16 = 2500;
pub const BUS_TIME_MAX: u16 = 9999;
pub const BUS_BROADCAST: u8 = 255;

fn clamp(v: u16, lo: u16, hi: u16) -> u16 {
    if v < lo { lo } else if v > hi { hi } else { v }
}

/// 单舵机位置帧：`#001P1500T0500!`（pwm/ms 越界钳位，不拒发——总线舵机自身也有 SMI/SMX 兜底）
pub fn move_cmd(id: u8, pwm: u16, ms: u16) -> String {
    format!("#{:03}P{:04}T{:04}!",
            id,
            clamp(pwm, BUS_PWM_MIN, BUS_PWM_MAX),
            clamp(ms, 0, BUS_TIME_MAX))
}

/// 多舵机同步包：`{G0000#001P..T..!#002P..T..!}`
pub fn sync_cmd(items: &[(u8, u16, u16)]) -> String {
    let mut s = String::from("{G0000");
    for &(id, pw, t) in items {
        s.push_str(&move_cmd(id, pw, t));
    }
    s.push('}');
    s
}

/// 回读/配置查询类别
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Query { Pos, TempVolt, Id, Version, Mode }

/// 查询帧：`#001PRAD!` / `#001PRTE!` / `#001PID!` / `#001PVER!` / `#001PMOD!`
pub fn query_cmd(id: u8, q: Query) -> String {
    let tail = match q {
        Query::Pos => "RAD",
        Query::TempVolt => "RTE",
        Query::Id => "ID",
        Query::Version => "VER",
        Query::Mode => "MOD",
    };
    format!("#{:03}P{}!", id, tail)
}

/// 改 ID 帧（bring-up 逐个上链时用）：`#000PID001!`
pub fn set_id_cmd(old_id: u8, new_id: u8) -> String {
    format!("#{:03}PID{:03}!", old_id, new_id)
}

/// 释力/恢复扭力：`#001PULK!` / `#001PULR!`（torque-off 自然下垂=总线舵机原生安全态）
pub fn torque_cmd(id: u8, on: bool) -> String {
    format!("#{:03}P{}!", id, if on { "ULR" } else { "ULK" })
}

/// 急停当前位：`#001PDST!`
pub fn stop_cmd(id: u8) -> String {
    format!("#{:03}PDST!", id)
}

/// 解析位置回读 `#001P1500!` → (id, pwm)
pub fn parse_pos(reply: &str) -> Option<(u8, u16)> {
    let b = reply.as_bytes();
    if b.len() != 10 || b[0] != b'#' || b[4] != b'P' || b[9] != b'!' {
        return None;
    }
    let id = std::str::from_utf8(&b[1..4]).ok()?.parse().ok()?;
    let pw = std::str::from_utf8(&b[5..9]).ok()?.parse().ok()?;
    Some((id, pw))
}

/// 解析温度/电压回读 `#001T1927-08.1!` → (id, AD, 电压V, 温度°C)
///
/// 温度公式（手册 V3.1 原文）：R = 10.0*AD/2048.0; T = 0.633916*R*R - 13.2675*R + 94.28351
pub fn parse_temp_volt(reply: &str) -> Option<(u8, u16, f32, f32)> {
    let b = reply.as_bytes();
    if b.len() < 15 || b[0] != b'#' || b[4] != b'T' || b[9] != b'-' {
        return None;
    }
    let id = std::str::from_utf8(&b[1..4]).ok()?.parse().ok()?;
    let ad = std::str::from_utf8(&b[5..9]).ok()?.parse().ok()?;
    let vs = std::str::from_utf8(&b[10..b.len() - 1]).ok()?;
    let volt: f32 = vs.parse().ok()?;
    let r = 10.0 * (ad as f32) / 2048.0;
    let t = 0.633916 * r * r - 13.2675 * r + 94.28351;
    Some((id, ad, volt, t))
}

/// 解析 ID 回读/改 ID 应答 `#001P!` → id
pub fn parse_id_echo(reply: &str) -> Option<u8> {
    let b = reply.as_bytes();
    if b.len() != 6 || b[0] != b'#' || b[4] != b'P' || b[5] != b'!' {
        return None;
    }
    std::str::from_utf8(&b[1..4]).ok()?.parse().ok()
}

/// 配置类命令成功应答 `#OK!`
pub fn parse_ok(reply: &str) -> bool {
    reply == "#OK!"
}

#[cfg(test)]
mod tests {
    use super::*;

    // 以下断言与 DUANYAN/src/bus_zx361.rs 逐字节一致（单一真值口径防漂移）
    #[test]
    fn test_move_cmd_format_and_clamp() {
        assert_eq!(move_cmd(1, 1500, 500), "#001P1500T0500!");
        assert_eq!(move_cmd(0, 100, 20), "#000P0500T0020!");   // pwm 下钳
        assert_eq!(move_cmd(254, 9999, 65535), "#254P2500T9999!"); // 双上钳
    }

    #[test]
    fn test_sync_cmd_wrap() {
        let s = sync_cmd(&[(1, 1500, 1000), (2, 2500, 0)]);
        assert_eq!(s, "{G0000#001P1500T1000!#002P2500T0000!}");
        assert_eq!(sync_cmd(&[]), "{G0000}");
    }

    #[test]
    fn test_query_and_config_cmds() {
        assert_eq!(query_cmd(3, Query::Pos), "#003PRAD!");
        assert_eq!(query_cmd(3, Query::TempVolt), "#003PRTE!");
        assert_eq!(set_id_cmd(0, 1), "#000PID001!");
        assert_eq!(torque_cmd(7, false), "#007PULK!");
        assert_eq!(stop_cmd(7), "#007PDST!");
    }

    #[test]
    fn test_parse_pos() {
        assert_eq!(parse_pos("#001P1500!"), Some((1, 1500)));
        assert_eq!(parse_pos("#000P0500!"), Some((0, 500)));
        assert_eq!(parse_pos("#001P1500"), None);   // 缺尾
        assert_eq!(parse_pos("#01P1500!!"), None);  // ID 非 3 位
        assert_eq!(parse_pos("#001PVER!x"), None);
    }

    #[test]
    fn test_parse_temp_volt_manual_example() {
        // 手册原文示例 #000T1927-08.1!：AD=1927, 电压 8.1V
        let (id, ad, volt, t) = parse_temp_volt("#000T1927-08.1!").unwrap();
        assert_eq!((id, ad), (0, 1927));
        assert!((volt - 8.1).abs() < 1e-3);
        // R=9.4092 → T≈25.6°C（室温合理性）
        assert!((24.0..27.0).contains(&t), "T={}", t);
    }

    #[test]
    fn test_parse_id_echo_and_ok() {
        assert_eq!(parse_id_echo("#001P!"), Some(1));
        assert_eq!(parse_id_echo("#OK!"), None);
        assert!(parse_ok("#OK!"));
        assert!(!parse_ok("#001P!"));
    }
}
