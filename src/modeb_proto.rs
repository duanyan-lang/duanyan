//! v1.6 P3: Mode-B 线协议编解码（std 宿主移植，镜像 DUANYAN `mode_b_proto.rs`）
//!
//! 与 ESP32-S3 真机固件实测线协议严格对齐（当前固件 `uart_linkm6.bin`，位啮 UART）：
//!   帧格式:  0xAA 0x55 LEN CMD PAYLOAD[LEN] CRC8
//!   CRC8:    多项式 0x07，初值 0，覆盖 LEN..PAYLOAD（2+LEN 字节）
//!   传输:    每帧前置 'U' 校准突发（位啮自校准），由 C helper `_hardy_modeb_send` 承担
//!
//! 诚实边界：本模块只做「字节级一致」的帧编解码镜像，供
//!   - codegen 吐出的 C 侧 `_hardy_modeb_send`（组帧 + crc8 + UART 写 + 'U' 突发）对照；
//!   - `--target duanyan-spine` 编译路径的协议绑定与单测；
//! 不替换、不重新实现真机固件的位啮时序（那是 uart_linkm6 的领域）。
//!
//! 命令表（v1.5 口径，以实测固件为准）：
//!   0x01 心跳 / 0x02 传感查询 / 0x10 灯效 / 0x11 GPIO / 0x12 查询状态
//!   0x20 关节设置 / 0x21 关节查询 / 0x22 多关节同步 / 0x23 序列回放 / 0x24 播放状态查询
//!   0x30 臂总线同步写 / 0x31 臂总线 ASCII 透传
//!   0x06 ACK / 0x15 NACK / 0x16 坏帧应答

pub const FRAME_HEAD0: u8 = 0xAA;
pub const FRAME_HEAD1: u8 = 0x55;
pub const MAX_PAYLOAD: usize = 64;

// ---- 命令字（与 DUANYAN mode_b_proto 一致）----
pub const CMD_HEARTBEAT: u8 = 0x01;
pub const CMD_SENSOR_QUERY: u8 = 0x02;
pub const CMD_SET_LED: u8 = 0x10;
pub const CMD_GPIO_OUTPUT: u8 = 0x11;
pub const CMD_QUERY_STATUS: u8 = 0x12;
pub const CMD_SERVO_SET: u8 = 0x20;
pub const CMD_SERVO_QUERY: u8 = 0x21;
pub const CMD_SERVO_MULTI: u8 = 0x22;
pub const CMD_SERVO_SEQ: u8 = 0x23;
pub const CMD_PLAY_QUERY: u8 = 0x24;
pub const CMD_ARM_SYNC: u8 = 0x30;
pub const CMD_ARM_ASCII: u8 = 0x31;
pub const CMD_ACK: u8 = 0x06;
pub const CMD_NACK: u8 = 0x15;
pub const CMD_BAD_FRAME: u8 = 0x16;

// ---- v1.5 播放状态与臂总线 ----
pub const PLAY_STATE_IDLE: u8 = 0;
pub const PLAY_STATE_SEQ: u8 = 1;
pub const PLAY_STATE_TRACK: u8 = 2;
/// 0x30 单帧最多舵机项数（payload ≤ 1+8*5=41 < MAX_PAYLOAD）
pub const ARM_SYNC_MAX: usize = 8;
/// 0x31 ASCII 透传最大长度
pub const ARM_ASCII_MAX: usize = 48;
pub const ARM_STATUS_OK: u8 = 0;
pub const ARM_STATUS_BAD_PARAM: u8 = 1;
pub const ARM_STATUS_BUS_FAIL: u8 = 2;

// ---- 传感器 ID ----
pub const SID_YL69_SOIL: u8 = 1;

// ---- 舵机 / 颈四关节（L1.5, PCA9685）----
/// 角度上限 180.0°（单位 0.1°）
pub const SERVO_MAX_ANGLE: u16 = 1800;
/// 颈关节数 = 协议关节号 0..3
pub const ARM_JOINT_COUNT: usize = 4;
/// 合法关节号上限（0x22 对非法关节号整帧丢弃）
pub const JOINT_ID_MAX: u8 = 3;
/// 归中位角值（0.1°），与固件 jt_ang[] 上电初值一致
pub const ARM_HOME_ANGLE: u16 = 900;
/// 关节数上限：1+3n ≤ MAX_PAYLOAD(64) → n ≤ 21
pub const JOINT_MAX: usize = 21;
/// 0x23 序列回放单帧最大步数：2+5·steps ≤ 64 → steps ≤ 12
pub const SEQ_MAX_STEPS: usize = 12;

/// 关节目标角（joint = 协议关节号，由固件映射到 PCA9685 通道）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JointTarget {
    pub joint: u8,
    pub angle: u16,
}

/// 角度(度) → 协议角值（0.1° 分辨率，越界截断到 [0, 180.0]）
pub fn deg_to_servo(deg: f32) -> u16 {
    let v = (deg * 10.0).round() as i32;
    v.clamp(0, SERVO_MAX_ANGLE as i32) as u16
}

/// 协议角值 → 角度(度)
pub fn servo_to_deg(raw: u16) -> f32 {
    raw.min(SERVO_MAX_ANGLE) as f32 / 10.0
}

/// CRC8（poly 0x07, init 0）—— 与真机固件 crc8() 逐位等价
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// 协议帧
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub cmd: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(cmd: u8, payload: Vec<u8>) -> Self {
        Self { cmd, payload }
    }

    /// 编码为线上字节流: AA 55 LEN CMD PAYLOAD... CRC
    pub fn encode(&self) -> Vec<u8> {
        let len = self.payload.len() as u8;
        let mut buf = Vec::with_capacity(5 + self.payload.len());
        buf.push(FRAME_HEAD0);
        buf.push(FRAME_HEAD1);
        buf.push(len);
        buf.push(self.cmd);
        buf.extend_from_slice(&self.payload);
        // CRC 覆盖 LEN, CMD, PAYLOAD
        let c = crc8(&buf[2..]);
        buf.push(c);
        buf
    }

    // ---------- 常用帧构造器（与 DUANYAN 同名同载荷）----------

    /// Pi->S3: 心跳（空载荷，S3 回 ACK 回显）
    pub fn heartbeat(uptime_s: u32) -> Self {
        Self::new(CMD_HEARTBEAT, uptime_s.to_le_bytes().to_vec())
    }

    /// Pi->S3: 灯效（HSV 色轮）hue: 0=红 85=蓝 170=绿; mode: 0=静态 1=彩虹
    pub fn set_led(hue: u8, val: u8, mode: u8) -> Self {
        Self::new(CMD_SET_LED, vec![hue, val, mode])
    }

    /// Pi->S3: GPIO 输出
    pub fn gpio_output(pin: u8, lvl: u8) -> Self {
        Self::new(CMD_GPIO_OUTPUT, vec![pin, lvl])
    }

    /// Pi->S3: 传感查询
    pub fn sensor_query() -> Self {
        Self::new(CMD_SENSOR_QUERY, vec![SID_YL69_SOIL])
    }

    /// Pi->S3: 查询状态（空载荷）
    pub fn query_status() -> Self {
        Self::new(CMD_QUERY_STATUS, vec![])
    }

    /// Pi->S3: 关节设置（L1.5, 0x20）payload[0] = 关节号
    /// ang: 0~1800（0.1°）; sweep: (lo, hi) 扫描区间，lo==hi 为静态定位。
    pub fn joint_set(joint: u8, ang: u16, sweep_lo: u16, sweep_hi: u16) -> Self {
        Self::new(
            CMD_SERVO_SET,
            vec![
                joint,
                (ang >> 8) as u8,
                (ang & 0xFF) as u8,
                (sweep_lo >> 8) as u8,
                (sweep_lo & 0xFF) as u8,
                (sweep_hi >> 8) as u8,
                (sweep_hi & 0xFF) as u8,
            ],
        )
    }

    /// Pi->S3: 关节查询（L1.5, 0x21）载荷 [joint]
    pub fn joint_query(joint: u8) -> Self {
        Self::new(CMD_SERVO_QUERY, vec![joint])
    }

    /// Pi->S3: 播放状态查询（v1.5, 0x24）空载荷
    pub fn play_query() -> Self {
        Self::new(CMD_PLAY_QUERY, vec![])
    }

    /// Pi->S3: 臂总线多舵机同步写（v1.5, 0x30）
    /// payload: [n][n×(id, pw_hi, pw_lo, t_hi, t_lo)]，n≤8；n=0 或 n>8 返回 None。
    pub fn arm_sync(items: &[(u8, u16, u16)]) -> Option<Self> {
        if items.is_empty() || items.len() > ARM_SYNC_MAX {
            return None;
        }
        let mut payload = Vec::with_capacity(1 + items.len() * 5);
        payload.push(items.len() as u8);
        for &(id, pw, t) in items {
            payload.push(id);
            payload.push((pw >> 8) as u8);
            payload.push((pw & 0xFF) as u8);
            payload.push((t >> 8) as u8);
            payload.push((t & 0xFF) as u8);
        }
        Some(Self::new(CMD_ARM_SYNC, payload))
    }

    /// Pi->S3: 臂总线 ASCII 透传（v1.5, 0x31）payload: [len][ascii…]，len≤48。
    pub fn arm_ascii(cmd: &str) -> Option<Self> {
        let b = cmd.as_bytes();
        if b.is_empty() || b.len() > ARM_ASCII_MAX {
            return None;
        }
        let mut payload = Vec::with_capacity(1 + b.len());
        payload.push(b.len() as u8);
        payload.extend_from_slice(b);
        Some(Self::new(CMD_ARM_ASCII, payload))
    }

    /// Pi->S3: 四关节归中位（0x22, 全部 ARM_HOME_ANGLE）
    pub fn arm_home() -> Self {
        let mut payload = Vec::with_capacity(1 + ARM_JOINT_COUNT * 3);
        payload.push(ARM_JOINT_COUNT as u8);
        for j in 0..ARM_JOINT_COUNT as u8 {
            payload.push(j);
            payload.push((ARM_HOME_ANGLE >> 8) as u8);
            payload.push((ARM_HOME_ANGLE & 0xFF) as u8);
        }
        Self::new(CMD_SERVO_MULTI, payload)
    }

    /// Pi->S3: 四关节协同姿态（0x22），angles 下标 = 关节号；长度不符返回 None。
    pub fn arm_pose(angles: &[u16]) -> Option<Self> {
        if angles.len() != ARM_JOINT_COUNT {
            return None;
        }
        let targets: Vec<JointTarget> = angles.iter().enumerate()
            .map(|(j, &a)| JointTarget { joint: j as u8, angle: a })
            .collect();
        Self::servo_multi(&targets)
    }

    /// Pi->S3: 多关节同步设置（0x22）—— 同帧原子更新，协同姿态必须整帧生效。
    /// 载荷: [n][n×(joint,ang_hi,ang_lo)]；角度越界截断到 0~1800；n=0 或 n>21 返回 None。
    pub fn servo_multi(targets: &[JointTarget]) -> Option<Self> {
        if targets.is_empty() || targets.len() > JOINT_MAX {
            return None;
        }
        let mut payload = Vec::with_capacity(1 + targets.len() * 3);
        payload.push(targets.len() as u8);
        for t in targets {
            let ang = t.angle.min(SERVO_MAX_ANGLE);
            payload.push(t.joint);
            payload.push((ang >> 8) as u8);
            payload.push((ang & 0xFF) as u8);
        }
        Some(Self::new(CMD_SERVO_MULTI, payload))
    }

    /// Pi->S3: 序列回放（L1.5, 0x23）—— 一帧上传整段编排，S3 阻塞式自播 loops 遍。
    /// 每步 step = [J1,J2,J3,J4 目标角(整度), dur(100ms 单位)]。
    pub fn servo_seq(loops: u8, steps: &[[u8; 5]]) -> Option<Self> {
        if loops == 0 || steps.is_empty() || steps.len() > SEQ_MAX_STEPS {
            return None;
        }
        let mut payload = Vec::with_capacity(2 + steps.len() * 5);
        payload.push(loops);
        payload.push(steps.len() as u8);
        for s in steps {
            payload.extend_from_slice(s);
        }
        Some(Self::new(CMD_SERVO_SEQ, payload))
    }

    // ---------- 应答解析器（Pi 侧回读，与 DUANYAN 同语义）----------

    /// 解析关节设置 ACK: [0x20回声][ang_hi][ang_lo][status] → (实际角值, status)
    pub fn parse_servo_ack(&self) -> Option<(u16, u8)> {
        if self.cmd == CMD_ACK
            && self.payload.len() >= 4
            && self.payload[0] == CMD_SERVO_SET
        {
            let ang = ((self.payload[1] as u16) << 8) | self.payload[2] as u16;
            return Some((ang, self.payload[3]));
        }
        None
    }

    /// 解析多关节设置 ACK: [0x22回声][applied][status]
    pub fn parse_servo_multi_ack(&self) -> Option<(u8, u8)> {
        if self.cmd == CMD_ACK
            && self.payload.len() >= 3
            && self.payload[0] == CMD_SERVO_MULTI
        {
            return Some((self.payload[1], self.payload[2]));
        }
        None
    }

    /// ACK 回显帧: 取被应答的原 CMD
    pub fn parse_ack_echo(&self) -> Option<u8> {
        if self.cmd != CMD_ACK || self.payload.is_empty() {
            return None;
        }
        Some(self.payload[0])
    }

    /// 解析臂总线同步 ACK: [0x30回声][applied][status]
    pub fn parse_arm_sync_ack(&self) -> Option<(u8, u8)> {
        if self.cmd == CMD_ACK
            && self.payload.len() >= 3
            && self.payload[0] == CMD_ARM_SYNC
        {
            return Some((self.payload[1], self.payload[2]));
        }
        None
    }

    /// 校验一条已编码帧的 CRC（decode 前的宿主侧自检）
    pub fn verify_encoded(buf: &[u8]) -> bool {
        buf.len() >= 5
            && buf[0] == FRAME_HEAD0
            && buf[1] == FRAME_HEAD1
            && buf[2] as usize == buf.len() - 5
            && crc8(&buf[2..buf.len() - 1]) == buf[buf.len() - 1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 黄金帧（DUANYAN 实机验收口径）：心跳 [uptime=0] → AA 55 04 01 00 00 00 00 C6
    #[test]
    fn test_golden_heartbeat() {
        assert_eq!(
            Frame::heartbeat(0).encode(),
            vec![0xAA, 0x55, 0x04, 0x01, 0x00, 0x00, 0x00, 0x00, 0xC6]
        );
    }

    /// 黄金帧：状态查询（空载荷）→ AA 55 00 12 7E
    #[test]
    fn test_golden_query_status() {
        assert_eq!(
            Frame::query_status().encode(),
            vec![0xAA, 0x55, 0x00, 0x12, 0x7E]
        );
    }

    /// 黄金帧：arm_home → AA 55 0D 22 04 00 03 84 01 03 84 02 03 84 03 03 84 88
    #[test]
    fn test_golden_arm_home() {
        assert_eq!(
            Frame::arm_home().encode(),
            vec![0xAA, 0x55, 0x0D, 0x22,
                 0x04,
                 0x00, 0x03, 0x84,
                 0x01, 0x03, 0x84,
                 0x02, 0x03, 0x84,
                 0x03, 0x03, 0x84,
                 0x88]
        );
    }

    /// 黄金帧：arm_sync([(1, 1500, 500)]) payload = [1, 1, 0x05, 0xDC, 0x01, 0xF4]
    /// —— 与 joint_api P0 路由 BusSync 的数值口径一致
    #[test]
    fn test_golden_arm_sync_payload() {
        let f = Frame::arm_sync(&[(1, 1500, 500)]).expect("单项 arm_sync 应成功");
        assert_eq!(f.payload, vec![1, 1, 0x05, 0xDC, 0x01, 0xF4]);
        assert_eq!(f.cmd, CMD_ARM_SYNC);
        assert!(Frame::verify_encoded(&f.encode()));
        // 参数护栏：n=0 / n>8 拒绝
        assert!(Frame::arm_sync(&[]).is_none());
        assert!(Frame::arm_sync(&[(1, 1500, 500); 9]).is_none());
    }

    /// crc8 逐字节一致性：已知黄金帧的 CRC 与 verify_encoded 自洽
    #[test]
    fn test_crc8_golden() {
        // 状态查询帧 CRC 覆盖 [00, 12] → 0x7E
        assert_eq!(crc8(&[0x00, 0x12]), 0x7E);
        // arm_home 帧 CRC 覆盖 LEN..PAYLOAD
        let f = Frame::arm_home();
        let enc = f.encode();
        assert_eq!(enc[enc.len() - 1], 0x88);
        assert!(Frame::verify_encoded(&enc));
        // 篡改一字节应校验失败
        let mut bad = enc.clone();
        bad[5] ^= 0xFF;
        assert!(!Frame::verify_encoded(&bad));
    }

    /// joint_set/joint_query/arm_ascii 载荷结构与 DUANYAN 一致
    #[test]
    fn test_constructors_byte_layout() {
        // joint_set(2, 900, 0, 0) → [2, 0x03, 0x84, 0, 0, 0, 0]
        let f = Frame::joint_set(2, 900, 0, 0);
        assert_eq!(f.cmd, CMD_SERVO_SET);
        assert_eq!(f.payload, vec![2, 0x03, 0x84, 0, 0, 0, 0]);
        // joint_query(3) → [3]
        assert_eq!(Frame::joint_query(3).payload, vec![3]);
        // arm_ascii("#002PRAD!") → [len][ascii…]
        let a = Frame::arm_ascii("#002PRAD!").expect("ASCII 透传应成功");
        assert_eq!(a.cmd, CMD_ARM_ASCII);
        assert_eq!(a.payload[0], 9);
        assert_eq!(&a.payload[1..], b"#002PRAD!");
        // 超长拒绝
        assert!(Frame::arm_ascii(&"x".repeat(ARM_ASCII_MAX + 1)).is_none());
    }

    /// 应答解析器：ACK 回显 / 多关节 applied / arm_sync applied
    #[test]
    fn test_ack_parsers() {
        let ack = Frame::new(CMD_ACK, vec![CMD_SERVO_MULTI, 4, ARM_STATUS_OK]);
        assert_eq!(ack.parse_ack_echo(), Some(CMD_SERVO_MULTI));
        assert_eq!(ack.parse_servo_multi_ack(), Some((4, 0)));
        let sync_ack = Frame::new(CMD_ACK, vec![CMD_ARM_SYNC, 1, ARM_STATUS_BUS_FAIL]);
        assert_eq!(sync_ack.parse_arm_sync_ack(), Some((1, 2)));
        let set_ack = Frame::new(CMD_ACK, vec![CMD_SERVO_SET, 0x03, 0x84, 0]);
        assert_eq!(set_ack.parse_servo_ack(), Some((900, 0)));
        // 非 ACK 帧不误解析
        assert_eq!(Frame::query_status().parse_ack_echo(), None);
    }

    /// 角度换算与常量口径
    #[test]
    fn test_angle_conversion() {
        assert_eq!(deg_to_servo(90.0), 900);
        assert_eq!(deg_to_servo(135.0), 1350);
        assert_eq!(deg_to_servo(999.0), SERVO_MAX_ANGLE);
        assert_eq!(deg_to_servo(-5.0), 0);
        assert_eq!(servo_to_deg(900), 90.0);
        assert_eq!(ARM_HOME_ANGLE, 900);
        assert_eq!(JOINT_ID_MAX, 3);
        assert_eq!(SEQ_MAX_STEPS, 12);
    }
}
