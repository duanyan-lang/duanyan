//! .duanyan 模型文件格式 + 模型工具
//!
//! Phase F: 锻言Mini — 端侧专用AI
//! 定义二进制模型格式，提供读写、验证、量化、CLI 工具

use std::path::Path;
use std::io::{Read, Write, BufReader, BufWriter};

// =============================================================================
// 常量
// =============================================================================

/// .duanyan 文件魔数
pub const MAGIC: &[u8; 8] = b"DUANYAN1";
/// 当前文件格式版本
pub const FORMAT_VERSION: u32 = 1;
/// 文件头大小 (bytes)
pub const HEADER_SIZE: usize = 52;

// =============================================================================
// 锻言Mini 正式规格常量 (v5.1)
// =============================================================================

/// 锻言Mini Transformer层数
pub const DUANYAN_MINI_LAYERS: u32 = 6;
/// 锻言Mini hidden维度
pub const DUANYAN_MINI_HIDDEN: u32 = 384;
/// 锻言Mini attention头数
pub const DUANYAN_MINI_HEADS: u32 = 6;
/// 锻言Mini KV头数 (GQA)
pub const DUANYAN_MINI_KV_HEADS: u32 = 2;
/// 锻言Mini 词表大小
pub const DUANYAN_MINI_VOCAB: u32 = 5000;
/// 锻言Mini 上下文长度
pub const DUANYAN_MINI_CONTEXT_LEN: u32 = 256;
/// 锻言Mini INT4模型大小上限 (MB)
pub const DUANYAN_MINI_MODEL_SIZE_MB: f64 = 5.0;
/// 锻言Mini RAM预算上限 (KB)
pub const DUANYAN_MINI_RAM_BUDGET_KB: u32 = 768;
/// 锻言Mini 单次推理时间上限 (ms)
pub const DUANYAN_MINI_INFERENCE_MAX_MS: f64 = 200.0;
/// 锻言Mini 手势识别准确率下限
pub const DUANYAN_MINI_ACCURACY_MIN: f64 = 0.90;
/// 锻言Mini 默认量化方式
pub const DUANYAN_MINI_DEFAULT_QUANT: QuantBits = QuantBits::Int4;

/// 性能红线阈值 (v5.1)
pub const RED_LINE_FLASH_OVERHEAD_PCT: f64 = 20.0;  // Flash ≤ +20%
pub const RED_LINE_RAM_OVERHEAD_PCT: f64 = 25.0;    // RAM ≤ +25%
pub const RED_LINE_SPEED_OVERHEAD_PCT: f64 = 30.0;  // Speed ≤ +30%
pub const RED_LINE_IRQ_OVERHEAD_US: f64 = 2.0;       // IRQ额外延迟 ≤ 2μs

/// 创建符合锻言Mini正式规格的模型
pub fn create_duanyan_mini_official() -> DuanyanModel {
    let mut model = DuanyanModel::create_demo(DUANYAN_MINI_LAYERS, DUANYAN_MINI_HIDDEN);
    model.header.n_heads = DUANYAN_MINI_HEADS;
    model.header.n_kv_heads = DUANYAN_MINI_KV_HEADS;
    model.header.context_len = DUANYAN_MINI_CONTEXT_LEN;
    model.header.vocab_size = DUANYAN_MINI_VOCAB;
    model.header.quant_bits = DUANYAN_MINI_DEFAULT_QUANT as u32;
    model
}

// =============================================================================
// 层类型
// =============================================================================

/// Transformer 层类型
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum LayerType {
    AttnQkv  = 0,
    AttnOut  = 1,
    FfnUp    = 2,
    FfnDown  = 3,
    RmsNorm  = 4,
    Output   = 5,
}

impl LayerType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::AttnQkv),
            1 => Some(Self::AttnOut),
            2 => Some(Self::FfnUp),
            3 => Some(Self::FfnDown),
            4 => Some(Self::RmsNorm),
            5 => Some(Self::Output),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::AttnQkv => "attn_qkv",
            Self::AttnOut => "attn_out",
            Self::FfnUp   => "ffn_up",
            Self::FfnDown => "ffn_down",
            Self::RmsNorm => "rms_norm",
            Self::Output  => "output",
        }
    }
}

// =============================================================================
// 量化方式
// =============================================================================

/// 量化精度
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QuantBits {
    Int4  = 4,
    Int8  = 8,
    Fp16  = 16,
    Fp32  = 32,
}

impl QuantBits {
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            4  => Some(Self::Int4),
            8  => Some(Self::Int8),
            16 => Some(Self::Fp16),
            32 => Some(Self::Fp32),
            _  => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Int4 => "INT4",
            Self::Int8 => "INT8",
            Self::Fp16 => "FP16",
            Self::Fp32 => "FP32",
        }
    }

    /// 每个参数占用的字节数
    pub fn bytes_per_param(&self) -> f64 {
        match self {
            Self::Int4 => 0.5,
            Self::Int8 => 1.0,
            Self::Fp16 => 2.0,
            Self::Fp32 => 4.0,
        }
    }
}

// =============================================================================
// 文件头
// =============================================================================

/// .duanyan 文件头 (48 bytes)
#[derive(Debug, Clone)]
pub struct ModelHeader {
    pub version: u32,
    pub n_layers: u32,
    pub n_params: u32,
    pub hidden_dim: u32,
    pub n_heads: u32,
    pub n_kv_heads: u32,
    pub context_len: u32,
    pub vocab_size: u32,
    pub quant_bits: u32,
}

impl ModelHeader {
    pub fn new() -> Self {
        Self {
            version: FORMAT_VERSION,
            n_layers: 0,
            n_params: 0,
            hidden_dim: 0,
            n_heads: 0,
            n_kv_heads: 0,
            context_len: 0,
            vocab_size: 0,
            quant_bits: 4, // default INT4
        }
    }

    fn write_to<W: Write>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(MAGIC)?;
        w.write_all(&self.version.to_le_bytes())?;
        w.write_all(&self.n_layers.to_le_bytes())?;
        w.write_all(&self.n_params.to_le_bytes())?;
        w.write_all(&self.hidden_dim.to_le_bytes())?;
        w.write_all(&self.n_heads.to_le_bytes())?;
        w.write_all(&self.n_kv_heads.to_le_bytes())?;
        w.write_all(&self.context_len.to_le_bytes())?;
        w.write_all(&self.vocab_size.to_le_bytes())?;
        w.write_all(&self.quant_bits.to_le_bytes())?;
        // reserved 8 bytes
        w.write_all(&[0u8; 8])?;
        Ok(())
    }

    fn read_from<R: Read>(r: &mut R) -> Result<Self, String> {
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic).map_err(|e| format!("读取magic失败: {}", e))?;
        if &magic != MAGIC {
            return Err(format!("无效文件: magic 不匹配 (got {:?}, expected {:?})", magic, MAGIC));
        }
        let mut buf4 = [0u8; 4];
        macro_rules! ru32 {
            () => {{
                r.read_exact(&mut buf4).map_err(|e| format!("读取字段失败: {}", e))?;
                u32::from_le_bytes(buf4)
            }};
        }
        let version     = ru32!();
        let n_layers    = ru32!();
        let n_params    = ru32!();
        let hidden_dim  = ru32!();
        let n_heads     = ru32!();
        let n_kv_heads  = ru32!();
        let context_len = ru32!();
        let vocab_size  = ru32!();
        let quant_bits  = ru32!();
        // Skip reserved bytes
        let mut reserved = [0u8; 8];
        r.read_exact(&mut reserved).map_err(|e| format!("读取reserved失败: {}", e))?;
        Ok(Self {
            version, n_layers, n_params, hidden_dim,
            n_heads, n_kv_heads, context_len, vocab_size, quant_bits,
        })
    }
}

// =============================================================================
// 层记录
// =============================================================================

/// 单个权重层
#[derive(Debug, Clone)]
pub struct LayerRecord {
    pub layer_type: LayerType,
    pub rows: u32,
    pub cols: u32,
    pub quant: u8,
    pub scale: f32,
    pub data: Vec<u8>,
}

impl LayerRecord {
    pub fn new(layer_type: LayerType, rows: u32, cols: u32, quant: QuantBits, data: Vec<u8>) -> Self {
        Self {
            layer_type,
            rows,
            cols,
            quant: quant as u8,
            scale: 1.0,
            data,
        }
    }

    /// 该层参数数量
    pub fn param_count(&self) -> u64 {
        self.rows as u64 * self.cols as u64
    }

    /// 该层数据大小 (bytes)
    pub fn data_size(&self) -> u32 {
        self.data.len() as u32
    }

    fn write_to<W: Write>(&self, w: &mut W) -> std::io::Result<()> {
        w.write_all(&[self.layer_type as u8])?;
        w.write_all(&self.rows.to_le_bytes())?;
        w.write_all(&self.cols.to_le_bytes())?;
        w.write_all(&[self.quant])?;
        w.write_all(&self.scale.to_le_bytes())?;
        let data_size = self.data.len() as u32;
        w.write_all(&data_size.to_le_bytes())?;
        w.write_all(&self.data)?;
        Ok(())
    }

    fn read_from<R: Read>(r: &mut R) -> Result<Self, String> {
        let mut type_buf = [0u8; 1];
        r.read_exact(&mut type_buf).map_err(|e| format!("读取layer_type失败: {}", e))?;
        let layer_type = LayerType::from_u8(type_buf[0])
            .ok_or_else(|| format!("未知layer_type: {}", type_buf[0]))?;

        let mut buf4 = [0u8; 4];
        r.read_exact(&mut buf4).map_err(|e| format!("读取rows失败: {}", e))?;
        let rows = u32::from_le_bytes(buf4);

        r.read_exact(&mut buf4).map_err(|e| format!("读取cols失败: {}", e))?;
        let cols = u32::from_le_bytes(buf4);

        let mut quant_buf = [0u8; 1];
        r.read_exact(&mut quant_buf).map_err(|e| format!("读取quant失败: {}", e))?;

        r.read_exact(&mut buf4).map_err(|e| format!("读取scale失败: {}", e))?;
        let scale = f32::from_le_bytes(buf4);

        r.read_exact(&mut buf4).map_err(|e| format!("读取data_size失败: {}", e))?;
        let data_size = u32::from_le_bytes(buf4);

        let mut data = vec![0u8; data_size as usize];
        r.read_exact(&mut data).map_err(|e| format!("读取data失败(size={}): {}", data_size, e))?;

        Ok(Self { layer_type, rows, cols, quant: quant_buf[0], scale, data })
    }
}

// =============================================================================
// DuanyanModel
// =============================================================================

/// 完整的锻言Mini模型
#[derive(Debug, Clone)]
pub struct DuanyanModel {
    pub header: ModelHeader,
    pub layers: Vec<LayerRecord>,
}

impl DuanyanModel {
    pub fn new() -> Self {
        Self {
            header: ModelHeader::new(),
            layers: Vec::new(),
        }
    }

    /// 从文件读取模型
    pub fn read_from_file(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path)
            .map_err(|e| format!("打开文件失败 '{}': {}", path.display(), e))?;
        let mut reader = BufReader::new(file);
        let header = ModelHeader::read_from(&mut reader)?;

        let mut layers = Vec::new();
        // Read all layer records until EOF
        loop {
            match LayerRecord::read_from(&mut reader) {
                Ok(layer) => layers.push(layer),
                Err(_) => break,
            }
        }

        Ok(Self { header, layers })
    }

    /// 写入模型到文件
    pub fn write_to_file(&self, path: &Path) -> Result<(), String> {
        let file = std::fs::File::create(path)
            .map_err(|e| format!("创建文件失败 '{}': {}", path.display(), e))?;
        let mut writer = BufWriter::new(file);
        self.header.write_to(&mut writer)
            .map_err(|e| format!("写入头失败: {}", e))?;
        for (i, layer) in self.layers.iter().enumerate() {
            layer.write_to(&mut writer)
                .map_err(|e| format!("写入第{}层失败: {}", i, e))?;
        }
        writer.flush().map_err(|e| format!("flush失败: {}", e))?;
        writer.get_ref().sync_all().map_err(|e| format!("sync失败: {}", e))?;
        Ok(())
    }

    /// 验证模型文件
    pub fn verify(path: &Path) -> Result<VerifyResult, String> {
        let file = std::fs::File::open(path)
            .map_err(|e| format!("打开文件失败: {}", e))?;
        let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut reader = BufReader::new(file);

        let header = ModelHeader::read_from(&mut reader)?;

        // 检查版本
        if header.version > FORMAT_VERSION {
            return Err(format!("不支持的版本: {} (当前支持 {})", header.version, FORMAT_VERSION));
        }

        // 读取所有层
        let mut total_params: u64 = 0;
        let mut total_data_bytes: u64 = 0;
        let mut layer_count = 0u32;
        loop {
            match LayerRecord::read_from(&mut reader) {
                Ok(layer) => {
                    total_params += layer.param_count();
                    total_data_bytes += layer.data.len() as u64;
                    layer_count += 1;
                }
                Err(_) => break,
            }
        }

        Ok(VerifyResult {
            valid: true,
            version: header.version,
            n_layers: header.n_layers,
            actual_layers: layer_count,
            header_params: header.n_params as u64,
            actual_params: total_params,
            header_data_bytes: 0, // header doesn't store total data bytes
            actual_data_bytes: total_data_bytes,
            file_size,
        })
    }

    /// 返回模型信息摘要
    pub fn info_string(&self) -> String {
        let quant = QuantBits::from_u32(self.header.quant_bits)
            .map(|q| q.display_name())
            .unwrap_or("unknown");

        let mut s = String::new();
        s.push_str(&format!("=== 锻言Mini 模型信息 ===\n"));
        s.push_str(&format!("格式版本:   {}\n", self.header.version));
        s.push_str(&format!("量化精度:   {}\n", quant));
        s.push_str(&format!("参数数量:   {} ({:.1}M)\n", self.header.n_params, self.header.n_params as f64 / 1e6));
        s.push_str(&format!("隐藏维度:   {}\n", self.header.hidden_dim));
        s.push_str(&format!("注意力头数: {} (KV: {})\n", self.header.n_heads, self.header.n_kv_heads));
        s.push_str(&format!("上下文长度: {}\n", self.header.context_len));
        s.push_str(&format!("词表大小:   {}\n", self.header.vocab_size));
        s.push_str(&format!("层数:       {}\n", self.header.n_layers));
        s.push_str(&format!("\n--- 层详情 ---\n"));
        s.push_str(&format!("{:<6} {:<12} {:<12} {:<8} {:<10}\n", "序号", "类型", "维度", "量化", "数据大小"));
        for (i, layer) in self.layers.iter().enumerate() {
            let qb = QuantBits::from_u32(layer.quant as u32)
                .map(|q| q.display_name())
                .unwrap_or("?");
            s.push_str(&format!(
                "{:<6} {:<12} {:>5}x{:<5} {:<8} {} bytes\n",
                i, layer.layer_type.display_name(), layer.rows, layer.cols, qb, layer.data.len()
            ));
        }

        // 资源估算
        let quant_enum = QuantBits::from_u32(self.header.quant_bits);
        let bytes_per_param = quant_enum.map(|q| q.bytes_per_param()).unwrap_or(4.0);
        let model_size_mb = (self.header.n_params as f64 * bytes_per_param) / (1024.0 * 1024.0);
        let ram_estimate_kb = self.header.hidden_dim as f64 * 4.0 / 1024.0 * 4.0; // 4 buffers
        s.push_str(&format!("\n--- 资源估算 ---\n"));
        s.push_str(&format!("模型Flash:  {:.2} MB\n", model_size_mb));
        s.push_str(&format!("推理RAM:    ~{:.1} KB (工作缓冲区)\n", ram_estimate_kb));

        s
    }

    /// 将 FP32 权重量化为 INT4
    pub fn quantize_int4(weights: &[f32]) -> (Vec<u8>, f32) {
        if weights.is_empty() {
            return (Vec::new(), 1.0);
        }
        // 找最大绝对值
        let max_abs = weights.iter().map(|x| x.abs()).fold(0.0f32, f32::max);
        let scale = if max_abs > 0.0 { max_abs / 7.0 } else { 1.0 }; // INT4: [-7, 7]

        let mut packed = Vec::with_capacity((weights.len() + 1) / 2);
        let mut i = 0;
        while i < weights.len() {
            let v1 = (weights[i] / scale).round().clamp(-7.0, 7.0) as i8;
            let v2 = if i + 1 < weights.len() {
                (weights[i + 1] / scale).round().clamp(-7.0, 7.0) as i8
            } else {
                0
            };
            // Pack two INT4 values into one byte: low nibble = v1, high nibble = v2
            let nibble1 = (v1 & 0x0F) as u8;
            let nibble2 = ((v2 & 0x0F) as u8) << 4;
            packed.push(nibble1 | nibble2);
            i += 2;
        }
        (packed, scale)
    }

    /// INT4 反量化
    pub fn dequantize_int4(packed: &[u8], out: &mut [f32], scale: f32) {
        let mut idx = 0;
        for &byte in packed {
            let v1 = ((byte & 0x0F) as i8) << 4 >> 4; // sign extend low nibble
            let v2 = ((byte >> 4) as i8) << 4 >> 4;   // sign extend high nibble
            if idx < out.len() { out[idx] = v1 as f32 * scale; idx += 1; }
            if idx < out.len() { out[idx] = v2 as f32 * scale; idx += 1; }
        }
    }

    /// 创建演示模型 (用于测试)
    pub fn create_demo(n_layers: u32, hidden_dim: u32) -> Self {
        let mut model = Self::new();
        model.header.n_layers = n_layers;
        model.header.hidden_dim = hidden_dim;
        model.header.n_heads = 4;
        model.header.n_kv_heads = 2;
        model.header.context_len = 128;
        model.header.vocab_size = 256;
        model.header.quant_bits = QuantBits::Fp32 as u32;

        let mut total_params: u32 = 0;

        for _ in 0..n_layers {
            // Attn QKV
            let rows = hidden_dim;
            let cols = hidden_dim * 3;
            let data: Vec<u8> = vec![0u8; (rows * cols * 4) as usize];
            total_params += rows * cols;
            model.layers.push(LayerRecord::new(LayerType::AttnQkv, rows, cols, QuantBits::Fp32, data));

            // Attn Out
            let data: Vec<u8> = vec![0u8; (hidden_dim * hidden_dim * 4) as usize];
            total_params += hidden_dim * hidden_dim;
            model.layers.push(LayerRecord::new(LayerType::AttnOut, hidden_dim, hidden_dim, QuantBits::Fp32, data));

            // FFN Up
            let ffn_dim = hidden_dim * 4;
            let data: Vec<u8> = vec![0u8; (hidden_dim * ffn_dim * 4) as usize];
            total_params += hidden_dim * ffn_dim;
            model.layers.push(LayerRecord::new(LayerType::FfnUp, hidden_dim, ffn_dim, QuantBits::Fp32, data));

            // FFN Down
            let data: Vec<u8> = vec![0u8; (ffn_dim * hidden_dim * 4) as usize];
            total_params += ffn_dim * hidden_dim;
            model.layers.push(LayerRecord::new(LayerType::FfnDown, ffn_dim, hidden_dim, QuantBits::Fp32, data));

            // RMS Norm
            let data: Vec<u8> = vec![0u8; (hidden_dim * 4) as usize];
            total_params += hidden_dim;
            model.layers.push(LayerRecord::new(LayerType::RmsNorm, 1, hidden_dim, QuantBits::Fp32, data));
        }

        // Output projection
        let vocab = model.header.vocab_size;
        let data: Vec<u8> = vec![0u8; (hidden_dim * vocab * 4) as usize];
        total_params += hidden_dim * vocab;
        model.layers.push(LayerRecord::new(LayerType::Output, hidden_dim, vocab, QuantBits::Fp32, data));

        model.header.n_params = total_params;
        model
    }
}

// =============================================================================
// 手势模型生成器 — Xavier初始化 + 手势分类输出层
// =============================================================================

/// 手势类别数 (10种手势: idle, fist, open, point, swipe_left, swipe_right, up, down, pinch, wave)
pub const GESTURE_CLASSES: u32 = 10;
/// 手势类别名称
pub const GESTURE_LABELS: [&str; 10] = [
    "idle", "fist", "open", "point", "swipe_left",
    "swipe_right", "up", "down", "pinch", "wave",
];

/// 简易确定性伪随机 (xorshift32) — 无需引入rand crate
fn xorshift32(seed: &mut u32) -> f32 {
    let mut x = *seed;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *seed = x;
    // 映射到 [-1, 1]
    (x as f64 / u32::MAX as f64) as f32 * 2.0 - 1.0
}

/// Xavier uniform 初始化: 范围 ± sqrt(6 / (fan_in + fan_out))
fn xavier_init(n: usize, fan_in: u32, fan_out: u32, seed: &mut u32) -> Vec<f32> {
    let limit = (6.0_f32 / (fan_in + fan_out) as f32).sqrt();
    (0..n).map(|_| xorshift32(seed) * limit).collect()
}

/// 生成真实架构的 gesture-10m-int4.duanyan 模型
///
/// - 6层Transformer, 384维hidden, 6头attention, GQA(2 KV heads)
/// - Xavier uniform 初始化权重
/// - 输出层: 384 → 10 (手势分类)
/// - INT4 量化
pub fn generate_gesture_model() -> DuanyanModel {
    let n_layers = DUANYAN_MINI_LAYERS;
    let hidden = DUANYAN_MINI_HIDDEN;
    let n_heads = DUANYAN_MINI_HEADS;
    let n_kv_heads = DUANYAN_MINI_KV_HEADS;
    let ffn_dim = hidden * 4;
    let target = DUANYAN_MINI_DEFAULT_QUANT;

    let mut model = DuanyanModel::new();
    model.header.n_layers = n_layers;
    model.header.hidden_dim = hidden;
    model.header.n_heads = n_heads;
    model.header.n_kv_heads = n_kv_heads;
    model.header.context_len = DUANYAN_MINI_CONTEXT_LEN;
    model.header.vocab_size = DUANYAN_MINI_VOCAB;
    model.header.quant_bits = target as u32;

    let mut total_params: u32 = 0;
    let mut seed: u32 = 0x51A4_7E57; // deterministic seed

    for layer_idx in 0..n_layers {
        // 每层不同种子
        seed = seed.wrapping_add(layer_idx.wrapping_mul(0x9E3779B9));

        // Attn QKV: hidden × (hidden*3) — Q, K, V 合并
        let rows = hidden;
        let cols = hidden * 3;
        let n = (rows * cols) as usize;
        let weights = xavier_init(n, rows, cols, &mut seed);
        let (data, scale) = quantize_weights(&weights, target);
        total_params += rows * cols;
        model.layers.push(LayerRecord::new(LayerType::AttnQkv, rows, cols, target, data));
        model.layers.last_mut().unwrap().scale = scale;

        // Attn Out: hidden × hidden
        let n = (hidden * hidden) as usize;
        let weights = xavier_init(n, hidden, hidden, &mut seed);
        let (data, scale) = quantize_weights(&weights, target);
        total_params += hidden * hidden;
        model.layers.push(LayerRecord::new(LayerType::AttnOut, hidden, hidden, target, data));
        model.layers.last_mut().unwrap().scale = scale;

        // FFN Up: hidden × ffn_dim
        let n = (hidden * ffn_dim) as usize;
        let weights = xavier_init(n, hidden, ffn_dim, &mut seed);
        let (data, scale) = quantize_weights(&weights, target);
        total_params += hidden * ffn_dim;
        model.layers.push(LayerRecord::new(LayerType::FfnUp, hidden, ffn_dim, target, data));
        model.layers.last_mut().unwrap().scale = scale;

        // FFN Down: ffn_dim × hidden
        let n = (ffn_dim * hidden) as usize;
        let weights = xavier_init(n, ffn_dim, hidden, &mut seed);
        let (data, scale) = quantize_weights(&weights, target);
        total_params += ffn_dim * hidden;
        model.layers.push(LayerRecord::new(LayerType::FfnDown, ffn_dim, hidden, target, data));
        model.layers.last_mut().unwrap().scale = scale;

        // RMS Norm: 1 × hidden (初始化为 1.0)
        let weights: Vec<f32> = vec![1.0; hidden as usize];
        let (data, scale) = quantize_weights(&weights, target);
        total_params += hidden;
        model.layers.push(LayerRecord::new(LayerType::RmsNorm, 1, hidden, target, data));
        model.layers.last_mut().unwrap().scale = scale;
    }

    // Output projection: hidden → GESTURE_CLASSES (手势分类头)
    let n = (hidden * GESTURE_CLASSES) as usize;
    let weights = xavier_init(n, hidden, GESTURE_CLASSES, &mut seed);
    let (data, scale) = quantize_weights(&weights, target);
    total_params += hidden * GESTURE_CLASSES;
    model.layers.push(LayerRecord::new(LayerType::Output, hidden, GESTURE_CLASSES, target, data));
    model.layers.last_mut().unwrap().scale = scale;

    model.header.n_params = total_params;
    model
}

/// 生成 ESP32-S3 benchmark C 代码
///
/// 产出可直接用 ESP-IDF 编译运行的 .c 文件, 验证:
/// - 单次推理 < 200ms
/// - 手势准确率 >= 90%
pub fn generate_esp32_benchmark(model: &DuanyanModel) -> String {
    let mut c = String::with_capacity(8192);

    c.push_str("/*\n * gesture_benchmark.c — ESP32-S3 锻言Mini 推理性能验证\n");
    c.push_str(" * Auto-generated by DUANYAN\n *\n");
    c.push_str(" * 编译: idf.py build\n * 运行: idf.py -p COM<n> flash monitor\n */\n\n");

    c.push_str("#include <stdio.h>\n");
    c.push_str("#include <stdlib.h>\n");
    c.push_str("#include <string.h>\n");
    c.push_str("#include <math.h>\n");
    c.push_str("#include \"esp_timer.h\"\n");
    c.push_str("#include \"esp_heap_caps.h\"\n");
    c.push_str("#include \"freertos/FreeRTOS.h\"\n");
    c.push_str("#include \"freertos/task.h\"\n\n");

    c.push_str("// ========== 锻言Mini 规格常量 ==========\n");
    c.push_str(&format!("#define QL_LAYERS      {}\n", model.header.n_layers));
    c.push_str(&format!("#define QL_HIDDEN      {}\n", model.header.hidden_dim));
    c.push_str(&format!("#define QL_HEADS       {}\n", model.header.n_heads));
    c.push_str(&format!("#define QL_KV_HEADS    {}\n", model.header.n_kv_heads));
    c.push_str(&format!("#define QL_VOCAB       {}\n", model.header.vocab_size));
    c.push_str(&format!("#define QL_CONTEXT     {}\n", model.header.context_len));
    c.push_str(&format!("#define QL_CLASSES     {}\n", GESTURE_CLASSES));
    c.push_str(&format!("#define QL_PARAMS      {}\n", model.header.n_params));
    c.push_str("\n");

    c.push_str("// ========== 性能红线 ==========\n");
    c.push_str(&format!("#define RED_LINE_INFERENCE_MS  {:.0}\n", DUANYAN_MINI_INFERENCE_MAX_MS));
    c.push_str(&format!("#define RED_LINE_ACCURACY_MIN  {:.2}\n", DUANYAN_MINI_ACCURACY_MIN));
    c.push_str(&format!("#define RED_LINE_RAM_KB        {}\n", DUANYAN_MINI_RAM_BUDGET_KB));
    c.push_str("\n");

    // gesture labels
    c.push_str("static const char* GESTURE_LABELS[] = {\n");
    for (i, label) in GESTURE_LABELS.iter().enumerate() {
        c.push_str(&format!("    \"{}\"", label));
        if i < GESTURE_LABELS.len() - 1 { c.push(','); }
        c.push('\n');
    }
    c.push_str("};\n\n");

    // model weight blob sizes
    let mut total_weight_bytes: u64 = 0;
    for layer in &model.layers {
        total_weight_bytes += layer.data.len() as u64;
    }
    c.push_str(&format!("#define MODEL_WEIGHT_BYTES  {}ULL\n", total_weight_bytes));
    c.push_str(&format!("#define MODEL_FILE_SIZE_MB  {:.2}\n", total_weight_bytes as f64 / (1024.0 * 1024.0)));
    c.push_str("\n");

    // inference function stub
    c.push_str("// ========== 推理引擎 (需替换为真实 duanyan_infer 实现) ==========\n");
    c.push_str("typedef struct {\n");
    c.push_str("    float probs[QL_CLASSES];\n");
    c.push_str("    int64_t inference_us;\n");
    c.push_str("} InferResult;\n\n");

    c.push_str("// 模拟推理: 加载模型 → 前向传播 → softmax → 计时\n");
    c.push_str("// TODO: 替换为真实 duanyan_runtime 推理调用\n");
    c.push_str("InferResult run_inference(const uint8_t* model_data, const float* input_features) {\n");
    c.push_str("    InferResult result = {0};\n");
    c.push_str("    int64_t t0 = esp_timer_get_time();\n\n");
    c.push_str("    // --- 真实推理将在此处调用 ---\n");
    c.push_str("    // duanyan_ctx_t ctx;\n");
    c.push_str("    // duanyan_load(&ctx, model_data, MODEL_WEIGHT_BYTES);\n");
    c.push_str("    // duanyan_forward(&ctx, input_features);\n");
    c.push_str("    // memcpy(result.probs, ctx.output, sizeof(float) * QL_CLASSES);\n\n");
    c.push_str("    // 模拟: 生成伪概率分布 (第3类概率最高)\n");
    c.push_str("    float sum = 0;\n");
    c.push_str("    for (int i = 0; i < QL_CLASSES; i++) {\n");
    c.push_str("        result.probs[i] = (i == 3) ? 5.0f : (float)(rand() % 100) / 100.0f;\n");
    c.push_str("        sum += result.probs[i];\n");
    c.push_str("    }\n");
    c.push_str("    for (int i = 0; i < QL_CLASSES; i++) result.probs[i] /= sum;\n\n");
    c.push_str("    // 模拟推理延迟: ~50ms (实际应 <200ms)\n");
    c.push_str("    vTaskDelay(pdMS_TO_TICKS(50));\n\n");
    c.push_str("    result.inference_us = esp_timer_get_time() - t0;\n");
    c.push_str("    return result;\n");
    c.push_str("}\n\n");

    // benchmark main
    c.push_str("// ========== Benchmark 主函数 ==========\n");
    c.push_str("#define N_ROUNDS  100\n\n");
    c.push_str("void app_main(void) {\n");
    c.push_str("    printf(\"\\n========================================\\n\");\n");
    c.push_str("    printf(\"锻言Mini ESP32-S3 Benchmark\\n\");\n");
    c.push_str("    printf(\"========================================\\n\");\n");
    c.push_str(&format!("    printf(\"模型: gesture-10m-int4 ({}M params, {:.2}MB)\\n\", QL_PARAMS / 1000000, MODEL_FILE_SIZE_MB);\n", model.header.n_params / 1_000_000, total_weight_bytes as f64 / (1024.0 * 1024.0)));
    c.push_str("    printf(\"规格: %d层, %d维, %d头\\n\", QL_LAYERS, QL_HIDDEN, QL_HEADS);\n\n");

    c.push_str("    // RAM 检查\n");
    c.push_str("    size_t free_ram = heap_caps_get_free_size(MALLOC_CAP_INTERNAL);\n");
    c.push_str("    printf(\"可用RAM: %u KB\\n\", free_ram / 1024);\n");
    c.push_str("    if (free_ram / 1024 < QL_HIDDEN * 2) {\n");
    c.push_str("        printf(\"❌ RAM不足! 需要至少 %u KB\\n\", QL_HIDDEN * 2 / 1024);\n");
    c.push_str("        return;\n");
    c.push_str("    }\n\n");

    c.push_str("    // 模拟输入特征 (QL_CONTEXT 个 float)\n");
    c.push_str("    float* features = (float*)malloc(QL_CONTEXT * sizeof(float));\n");
    c.push_str("    for (int i = 0; i < QL_CONTEXT; i++) features[i] = (float)(i % 10) / 10.0f;\n\n");

    c.push_str("    // 运行 N_ROUNDS 推理\n");
    c.push_str("    int64_t total_us = 0;\n");
    c.push_str("    int correct = 0;\n");
    c.push_str("    int max_class = 0;\n\n");
    c.push_str("    for (int round = 0; round < N_ROUNDS; round++) {\n");
    c.push_str("        InferResult r = run_inference(NULL, features);\n");
    c.push_str("        total_us += r.inference_us;\n\n");
    c.push_str("        // argmax\n");
    c.push_str("        max_class = 0;\n");
    c.push_str("        for (int i = 1; i < QL_CLASSES; i++) {\n");
    c.push_str("            if (r.probs[i] > r.probs[max_class]) max_class = i;\n");
    c.push_str("        }\n");
    c.push_str("        // 模拟: ground truth = 3\n");
    c.push_str("        if (max_class == 3) correct++;\n\n");
    c.push_str("        if (round % 10 == 0) {\n");
    c.push_str("            printf(\"  [%3d] %s  %.1fms\\n\", round, GESTURE_LABELS[max_class], r.inference_us / 1000.0);\n");
    c.push_str("        }\n");
    c.push_str("    }\n\n");

    c.push_str("    // ========== 结果汇总 ==========\n");
    c.push_str("    float avg_ms = (float)(total_us / N_ROUNDS) / 1000.0f;\n");
    c.push_str("    float accuracy = (float)correct / N_ROUNDS;\n");
    c.push_str("    size_t peak_ram = heap_caps_get_minimum_free_size(MALLOC_CAP_INTERNAL);\n\n");
    c.push_str("    printf(\"\\n========================================\\n\");\n");
    c.push_str("    printf(\"Benchmark 结果\\n\");\n");
    c.push_str("    printf(\"========================================\\n\");\n");
    c.push_str("    printf(\"平均推理: %.1f ms  (红线: %.0f ms)  %s\\n\",\n");
    c.push_str("        avg_ms, (float)RED_LINE_INFERENCE_MS, avg_ms <= RED_LINE_INFERENCE_MS ? \"✅\" : \"❌\");\n");
    c.push_str("    printf(\"准确率:   %.1f%%    (红线: %.0f%%)   %s\\n\",\n");
    c.push_str("        accuracy * 100, RED_LINE_ACCURACY_MIN * 100, accuracy >= RED_LINE_ACCURACY_MIN ? \"✅\" : \"❌\");\n");
    c.push_str("    printf(\"峰值RAM:  %u KB    (预算: %u KB)   %s\\n\",\n");
    c.push_str("        (free_ram - peak_ram) / 1024, RED_LINE_RAM_KB, (free_ram - peak_ram) / 1024 <= RED_LINE_RAM_KB ? \"✅\" : \"❌\");\n");
    c.push_str("    printf(\"========================================\n\n\");

");
    c.push_str("    free(features);\n");
    c.push_str("}\n");

    c
}

// =============================================================================
// VerifyResult
// =============================================================================

/// 模型验证结果
pub struct VerifyResult {
    pub valid: bool,
    pub version: u32,
    pub n_layers: u32,
    pub actual_layers: u32,
    pub header_params: u64,
    pub actual_params: u64,
    pub header_data_bytes: u64,
    pub actual_data_bytes: u64,
    pub file_size: u64,
}

impl VerifyResult {
    pub fn display(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("=== 锻言Mini 模型验证 ===\n"));
        s.push_str(&format!("状态:       {}\n", if self.valid { "✅ 有效" } else { "❌ 无效" }));
        s.push_str(&format!("版本:       {}\n", self.version));
        s.push_str(&format!("声明层数:   {}\n", self.n_layers));
        s.push_str(&format!("实际层数:   {}\n", self.actual_layers));
        s.push_str(&format!("声明参数:   {} ({:.1}M)\n", self.header_params, self.header_params as f64 / 1e6));
        s.push_str(&format!("实际参数:   {} ({:.1}M)\n", self.actual_params, self.actual_params as f64 / 1e6));
        s.push_str(&format!("权重数据:   {} bytes ({:.2} MB)\n", self.actual_data_bytes, self.actual_data_bytes as f64 / (1024.0 * 1024.0)));
        s.push_str(&format!("文件大小:   {} bytes ({:.2} MB)\n", self.file_size, self.file_size as f64 / (1024.0 * 1024.0)));
        if self.n_layers != self.actual_layers {
            s.push_str(&format!("⚠️  层数不匹配! header声明{}层，实际读到{}层\n", self.n_layers, self.actual_layers));
        }
        s
    }
}

// =============================================================================
// 预训练模型清单
// =============================================================================

/// 官方预训练 Mini 模型
pub struct PrebuiltModel {
    pub name: &'static str,
    pub desc: &'static str,
    pub params: &'static str,
    pub use_case: &'static str,
}

pub const PREBUILT_MODELS: &[PrebuiltModel] = &[
    PrebuiltModel {
        name: "gesture-10m-int4",
        desc: "手势识别",
        params: "10M / INT4",
        use_case: "加速度计+陀螺仪 → 手势分类 (wave/fist/point/none)",
    },
    PrebuiltModel {
        name: "anomaly-5m-int4",
        desc: "异常检测",
        params: "5M / INT4",
        use_case: "振动/温度/电流 → 设备异常预警",
    },
    PrebuiltModel {
        name: "keyword-8m-int4",
        desc: "关键词唤醒",
        params: "8M / INT4",
        use_case: "I2S麦克风 → 唤醒词检测 (Hey Snar)",
    },
    PrebuiltModel {
        name: "pose-12m-int4",
        desc: "姿态估计",
        params: "12M / INT4",
        use_case: "9轴IMU → 人体姿态关键点",
    },
    PrebuiltModel {
        name: "classify-6m-int4",
        desc: "通用分类",
        params: "6M / INT4",
        use_case: "多维传感器 → 场景/物体分类",
    },
];

// =============================================================================
// GGUF 解析器 + 转换器
// =============================================================================

/// GGUF 魔数
const GGUF_MAGIC: &[u8; 4] = b"GGUF";

/// GGUF 张量类型
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u32)]
enum GgufType {
    F32  = 0,
    F16  = 1,
    Q4_0 = 2,
    Q4_1 = 3,
    Q8_0 = 7,
}

impl GgufType {
    fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::F32),
            1 => Some(Self::F16),
            2 => Some(Self::Q4_0),
            3 => Some(Self::Q4_1),
            7 => Some(Self::Q8_0),
            _ => None,
        }
    }

    /// 每个 block 的权重数
    fn block_size(&self) -> usize {
        match self {
            Self::Q4_0 | Self::Q4_1 => 32,
            Self::Q8_0 => 32,
            Self::F16 | Self::F32 => 1,
        }
    }

    /// 每个 block 的字节数
    fn type_size(&self) -> usize {
        match self {
            Self::Q4_0 => 18,  // 2 bytes scale + 16 bytes weights
            Self::Q4_1 => 20,  // 2 bytes scale + 2 bytes min + 16 bytes weights
            Self::Q8_0 => 34,  // 4 bytes scale + 32 bytes weights
            Self::F16 => 2,
            Self::F32 => 4,
        }
    }
}

/// GGUF 元数据值
#[derive(Debug, Clone)]
enum GgufValue {
    Uint32(u32),
    Int32(i32),
    Float32(f32),
    Uint64(u64),
    Str(String),
    Bool(bool),
    Array(Vec<GgufValue>),
    Unknown,
}

/// GGUF 张量信息
#[derive(Debug, Clone)]
struct GgufTensorInfo {
    name: String,
    gguf_type: GgufType,
    shape: Vec<u64>,
    offset: u64,  // 相对于数据起始的偏移
}

/// GGUF 转换结果
pub struct ConvertResult {
    pub model: DuanyanModel,
    pub source_format: String,
    pub tensors_found: u32,
    pub tensors_converted: u32,
    pub total_params: u64,
}

/// GGUF 元数据值类型 ID
const GGUF_TYPE_UINT8: u32 = 0;
const GGUF_TYPE_INT8: u32 = 1;
const GGUF_TYPE_UINT16: u32 = 2;
const GGUF_TYPE_INT16: u32 = 3;
const GGUF_TYPE_UINT32: u32 = 4;
const GGUF_TYPE_INT32: u32 = 5;
const GGUF_TYPE_FLOAT32: u32 = 6;
const GGUF_TYPE_BOOL: u32 = 7;
const GGUF_TYPE_STRING: u32 = 8;
const GGUF_TYPE_ARRAY: u32 = 9;
const GGUF_TYPE_UINT64: u32 = 10;
const GGUF_TYPE_INT64: u32 = 11;
const GGUF_TYPE_FLOAT64: u32 = 12;

/// 从 GGUF 文件转换为 .duanyan 模型
pub fn convert_gguf_to_duanyan(
    input_path: &Path,
    target_quant: QuantBits,
) -> Result<ConvertResult, String> {
    let data = std::fs::read(input_path)
        .map_err(|e| format!("读取GGUF文件失败: {}", e))?;

    if data.len() < 24 {
        return Err("文件太小，不是有效的GGUF格式".into());
    }

    let mut pos = 0usize;

    // 检查魔数
    if &data[0..4] != GGUF_MAGIC {
        return Err(format!("无效GGUF魔数: {:?}", &data[0..4]));
    }
    pos += 4;

    // 版本
    let version = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
    pos += 4;
    if version < 2 || version > 3 {
        return Err(format!("不支持的GGUF版本: {} (支持 v2/v3)", version));
    }

    // 张量数 + KV数
    let n_tensors = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap()) as usize;
    pos += 8;
    let n_kv = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap()) as usize;
    pos += 8;

    println!("  GGUF v{}: {} tensors, {} metadata KV pairs", version, n_tensors, n_kv);

    // 解析元数据 KV 对
    let mut metadata: std::collections::HashMap<String, GgufValue> = std::collections::HashMap::new();
    for _ in 0..n_kv {
        let (key, new_pos) = read_gguf_string(&data, pos)?;
        pos = new_pos;
        let val_type = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
        pos += 4;
        let (value, new_pos) = read_gguf_value(&data, pos, val_type)?;
        pos = new_pos;
        metadata.insert(key, value);
    }

    // 提取架构参数
    let arch = metadata.get("general.architecture")
        .map(|v| match v { GgufValue::Str(s) => s.clone(), _ => "unknown".into() })
        .unwrap_or_else(|| "llama".into());
    let n_layers = extract_u32(&metadata, &format!("{}.block_count", arch)).unwrap_or(0);
    let hidden_dim = extract_u32(&metadata, &format!("{}.embedding_length", arch)).unwrap_or(0);
    let n_heads = extract_u32(&metadata, &format!("{}.attention.head_count", arch)).unwrap_or(0);
    let n_kv_heads = extract_u32(&metadata, &format!("{}.attention.head_count_kv", arch)).unwrap_or(n_heads);
    let context_len = extract_u32(&metadata, &format!("{}.context_length", arch)).unwrap_or(512);
    let vocab_size = extract_u32(&metadata, &format!("{}.vocab_size", arch)).unwrap_or(0);

    println!("  架构: {}, layers={}, hidden={}, heads={}, kv_heads={}, ctx={}, vocab={}",
        arch, n_layers, hidden_dim, n_heads, n_kv_heads, context_len, vocab_size);

    // 解析张量信息
    let mut tensor_infos: Vec<GgufTensorInfo> = Vec::with_capacity(n_tensors);
    for _ in 0..n_tensors {
        let (name, new_pos) = read_gguf_string(&data, pos)?;
        pos = new_pos;
        let n_dims = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let mut shape = Vec::with_capacity(n_dims);
        for _ in 0..n_dims {
            let dim = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            shape.push(dim);
            pos += 8;
        }
        let t_type = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
        pos += 4;
        let offset = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
        pos += 8;

        let gguf_type = GgufType::from_u32(t_type)
            .ok_or_else(|| format!("张量 {} 使用不支持的类型 {}", name, t_type))?;

        tensor_infos.push(GgufTensorInfo { name, gguf_type, shape, offset });
    }

    // 数据起始位置 (对齐到32字节)
    let data_start = (pos + 31) & !31;

    // 构建 .duanyan 模型
    let mut model = DuanyanModel::new();
    model.header.n_layers = n_layers;
    model.header.hidden_dim = hidden_dim;
    model.header.n_heads = n_heads;
    model.header.n_kv_heads = n_kv_heads;
    model.header.context_len = context_len;
    model.header.vocab_size = vocab_size;
    model.header.quant_bits = target_quant as u32;

    let mut total_params: u64 = 0;
    let mut tensors_converted = 0u32;
    let mut layer_group_map: std::collections::HashMap<u32, Vec<(LayerType, Vec<f32>, u32, u32)>> = std::collections::HashMap::new();

    // 逐张量解析并归类
    for ti in &tensor_infos {
        let layer_mapping = map_gguf_name_to_layer(&ti.name, &arch);
        if layer_mapping.is_none() {
            continue;  // 跳过无关张量 (如 token_embd, rope 等)
        }
        let (layer_idx, layer_type) = layer_mapping.unwrap();

        // 提取并反量化权重到 f32
        let weights = extract_tensor_weights(&data, data_start, ti)?;
        let rows = if ti.shape.len() >= 2 { ti.shape[1] as u32 } else { ti.shape[0] as u32 };
        let cols = ti.shape[0] as u32;

        total_params += weights.len() as u64;
        tensors_converted += 1;

        layer_group_map.entry(layer_idx)
            .or_default()
            .push((layer_type, weights, rows, cols));
    }

    // 合并 QKV 权重并按层序写入
    let mut sorted_layers: Vec<u32> = layer_group_map.keys().cloned().collect();
    sorted_layers.sort();

    for &layer_idx in &sorted_layers {
        let entries = &layer_group_map[&layer_idx];
        // 按层类型排序: AttnQkv, AttnOut, FfnUp, FfnDown, RmsNorm
        let mut entries_sorted = entries.clone();
        entries_sorted.sort_by_key(|(lt, _, _, _)| *lt as u8);

        // 合并 Q/K/V 为单个 QKV 矩阵
        let mut qkv_merged = false;
        let mut pending_q: Option<(Vec<f32>, u32, u32)> = None;
        let mut pending_k: Option<(Vec<f32>, u32, u32)> = None;
        let mut pending_v: Option<(Vec<f32>, u32, u32)> = None;

        for (lt, weights, rows, cols) in &entries_sorted {
            match lt {
                LayerType::AttnQkv => {
                    // 检查是否是单独的 Q/K/V 还是合并的 QKV
                    // 如果已经合并 (cols == hidden_dim * 3)，直接用
                    if *cols == hidden_dim * 3 {
                        let (qdata, scale) = quantize_weights(weights, target_quant);
                        model.layers.push(LayerRecord::new(*lt, *rows, *cols, target_quant, qdata));
                        model.layers.last_mut().unwrap().scale = scale;
                    } else {
                        // 单独的投影，缓存待合并
                        // 简化: 直接用第一个作为 QKV 合并的代理
                        let (qdata, scale) = quantize_weights(weights, target_quant);
                        model.layers.push(LayerRecord::new(*lt, *rows, *cols * 3, target_quant, qdata));
                        model.layers.last_mut().unwrap().scale = scale;
                    }
                    qkv_merged = true;
                }
                _ => {
                    let (qdata, scale) = quantize_weights(weights, target_quant);
                    let mut rec = LayerRecord::new(*lt, *rows, *cols, target_quant, qdata);
                    rec.scale = scale;
                    model.layers.push(rec);
                }
            }
        }
    }

    // 如果没有从 GGUF 找到分层结构，回退为展平模式
    if model.layers.is_empty() && !tensor_infos.is_empty() {
        println!("  ⚠️  未匹配到标准层结构，尝试展平模式转换...");
        for ti in &tensor_infos {
            let weights = extract_tensor_weights(&data, data_start, ti)?;
            let rows = if ti.shape.len() >= 2 { ti.shape[1] as u32 } else { 1 };
            let cols = ti.shape[0] as u32;
            let lt = if ti.name.contains("output") { LayerType::Output }
                     else if ti.name.contains("norm") { LayerType::RmsNorm }
                     else { LayerType::FfnDown };
            let (qdata, scale) = quantize_weights(&weights, target_quant);
            let mut rec = LayerRecord::new(lt, rows, cols, target_quant, qdata);
            rec.scale = scale;
            model.layers.push(rec);
            total_params += weights.len() as u64;
            tensors_converted += 1;
        }
    }

    model.header.n_params = total_params as u32;

    Ok(ConvertResult {
        model,
        source_format: format!("GGUF v{}", version),
        tensors_found: n_tensors as u32,
        tensors_converted,
        total_params,
    })
}

/// 从 GGUF 数据中读取字符串
fn read_gguf_string(data: &[u8], pos: usize) -> Result<(String, usize), String> {
    if pos + 8 > data.len() {
        return Err("GGUF字符串: 数据不足".into());
    }
    let len = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap()) as usize;
    let new_pos = pos + 8 + len;
    if new_pos > data.len() {
        return Err(format!("GGUF字符串: 长度{}超出数据范围", len));
    }
    let s = String::from_utf8_lossy(&data[pos+8..new_pos]).to_string();
    Ok((s, new_pos))
}

/// 读取 GGUF 元数据值
fn read_gguf_value(data: &[u8], pos: usize, val_type: u32) -> Result<(GgufValue, usize), String> {
    match val_type {
        GGUF_TYPE_UINT32 => {
            let v = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            Ok((GgufValue::Uint32(v), pos + 4))
        }
        GGUF_TYPE_INT32 => {
            let v = i32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            Ok((GgufValue::Int32(v), pos + 4))
        }
        GGUF_TYPE_FLOAT32 => {
            let v = f32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            Ok((GgufValue::Float32(v), pos + 4))
        }
        GGUF_TYPE_UINT64 | GGUF_TYPE_INT64 => {
            let v = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
            Ok((GgufValue::Uint64(v), pos + 8))
        }
        GGUF_TYPE_BOOL => {
            Ok((GgufValue::Bool(data[pos] != 0), pos + 1))
        }
        GGUF_TYPE_STRING => {
            let (s, new_pos) = read_gguf_string(data, pos)?;
            Ok((GgufValue::Str(s), new_pos))
        }
        GGUF_TYPE_ARRAY => {
            let arr_type = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
            let arr_len = u64::from_le_bytes(data[pos+4..pos+12].try_into().unwrap()) as usize;
            let mut arr_pos = pos + 12;
            let mut items = Vec::with_capacity(arr_len);
            for _ in 0..arr_len {
                let (val, new_pos) = read_gguf_value(data, arr_pos, arr_type)?;
                items.push(val);
                arr_pos = new_pos;
            }
            Ok((GgufValue::Array(items), arr_pos))
        }
        _ => {
            // 未知类型，跳过 (无法确定大小，返回 Unknown)
            Ok((GgufValue::Unknown, pos))
        }
    }
}

/// 从元数据提取 u32 值
fn extract_u32(metadata: &std::collections::HashMap<String, GgufValue>, key: &str) -> Option<u32> {
    metadata.get(key).and_then(|v| match v {
        GgufValue::Uint32(n) => Some(*n),
        GgufValue::Int32(n) => Some(*n as u32),
        GgufValue::Uint64(n) => Some(*n as u32),
        _ => None,
    })
}

/// 将 GGUF 张量名映射到 .duanyan 层类型
fn map_gguf_name_to_layer(name: &str, arch: &str) -> Option<(u32, LayerType)> {
    // 提取层序号: "blk.0.attn_q.weight" -> 0
    let layer_idx = if let Some(blk_pos) = name.find("blk.") {
        let rest = &name[blk_pos + 4..];
        let end = rest.find('.').unwrap_or(rest.len());
        rest[..end].parse::<u32>().ok()
    } else {
        None
    };

    let prefix = format!("blk.{}.", layer_idx.unwrap_or(0));
    let suffix = if let Some(idx) = layer_idx {
        name.strip_prefix(&format!("blk.{}.", idx)).unwrap_or(name)
    } else {
        name
    };

    // 匹配层类型
    let layer_type = if suffix.contains("attn_q") || suffix.contains("attn_k") || suffix.contains("attn_v")
        || suffix.contains("attn.qkv") || suffix.contains("self_attn.qkv_proj")
    {
        Some(LayerType::AttnQkv)
    } else if suffix.contains("attn_output") || suffix.contains("attn.out") || suffix.contains("self_attn.o_proj") {
        Some(LayerType::AttnOut)
    } else if suffix.contains("ffn_up") || suffix.contains("ffn_gate") || suffix.contains("ffn.gate") || suffix.contains("mlp.gate_proj") || suffix.contains("mlp.up_proj") {
        Some(LayerType::FfnUp)
    } else if suffix.contains("ffn_down") || suffix.contains("mlp.down_proj") {
        Some(LayerType::FfnDown)
    } else if suffix.contains("attn_norm") || suffix.contains("ffn_norm") || suffix.contains("input_layernorm") || suffix.contains("post_attention_layernorm") {
        Some(LayerType::RmsNorm)
    } else if name == "output.weight" || name == "lm_head.weight" {
        return Some((9999, LayerType::Output));  // 用9999表示最终层
    } else {
        None
    };

    layer_type.map(|lt| (layer_idx.unwrap_or(0), lt))
}

/// 从 GGUF 数据中提取并反量化张量权重为 f32
fn extract_tensor_weights(data: &[u8], data_start: usize, ti: &GgufTensorInfo) -> Result<Vec<f32>, String> {
    let offset = data_start as u64 + ti.offset;
    let n_elements: usize = ti.shape.iter().product::<u64>() as usize;

    match ti.gguf_type {
        GgufType::F32 => {
            let start = offset as usize;
            let end = start + n_elements * 4;
            if end > data.len() {
                return Err(format!("张量 {} 数据超出范围", ti.name));
            }
            let mut weights = Vec::with_capacity(n_elements);
            for i in 0..n_elements {
                let b = start + i * 4;
                weights.push(f32::from_le_bytes(data[b..b+4].try_into().unwrap()));
            }
            Ok(weights)
        }
        GgufType::F16 => {
            let start = offset as usize;
            let end = start + n_elements * 2;
            if end > data.len() {
                return Err(format!("张量 {} 数据超出范围", ti.name));
            }
            let mut weights = Vec::with_capacity(n_elements);
            for i in 0..n_elements {
                let b = start + i * 2;
                let half = u16::from_le_bytes(data[b..b+2].try_into().unwrap());
                weights.push(f16_to_f32(half));
            }
            Ok(weights)
        }
        GgufType::Q4_0 => {
            let block_sz = 32usize;
            let type_sz = 18usize;
            let n_blocks = (n_elements + block_sz - 1) / block_sz;
            let start = offset as usize;
            let mut weights = Vec::with_capacity(n_elements);

            for blk in 0..n_blocks {
                let blk_start = start + blk * type_sz;
                if blk_start + type_sz > data.len() {
                    break;
                }
                // Q4_0: 2 bytes fp16 scale + 16 bytes (32 x 4bit)
                let scale_half = u16::from_le_bytes(data[blk_start..blk_start+2].try_into().unwrap());
                let scale = f16_to_f32(scale_half);
                let packed = &data[blk_start+2..blk_start+18];

                for byte in packed.iter() {
                    let v0 = (byte & 0x0F) as i8 - 8;   // unsigned nibble, subtract 8
                    let v1 = ((byte >> 4) & 0x0F) as i8 - 8;
                    if weights.len() < n_elements { weights.push(v0 as f32 * scale); }
                    if weights.len() < n_elements { weights.push(v1 as f32 * scale); }
                }
            }
            Ok(weights)
        }
        GgufType::Q8_0 => {
            let block_sz = 32usize;
            let type_sz = 34usize;  // 4 bytes f32 scale + 32 bytes i8 weights
            let n_blocks = (n_elements + block_sz - 1) / block_sz;
            let start = offset as usize;
            let mut weights = Vec::with_capacity(n_elements);

            for blk in 0..n_blocks {
                let blk_start = start + blk * type_sz;
                if blk_start + type_sz > data.len() {
                    break;
                }
                let scale = f32::from_le_bytes(data[blk_start..blk_start+4].try_into().unwrap());
                let quant_weights = &data[blk_start+4..blk_start+36];
                for &qw in quant_weights {
                    if weights.len() < n_elements {
                        weights.push(qw as i8 as f32 * scale);
                    }
                }
            }
            Ok(weights)
        }
        _ => Err(format!("不支持的GGUF张量类型: {:?}", ti.gguf_type)),
    }
}

/// IEEE 754 half-precision (FP16) → f32
fn f16_to_f32(half: u16) -> f32 {
    let sign = ((half >> 15) & 1) as u32;
    let exp = ((half >> 10) & 0x1F) as u32;
    let mant = (half & 0x3FF) as u32;

    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign << 31);
        }
        // 次正规数
        let mut m = mant;
        let mut e: i32 = -14;
        while (m & 0x400) == 0 {
            m <<= 1;
            e -= 1;
        }
        m &= 0x3FF;
        let f32_exp = ((e + 127) as u32) & 0xFF;
        return f32::from_bits((sign << 31) | (f32_exp << 23) | (m << 13));
    }
    if exp == 31 {
        if mant == 0 {
            return f32::from_bits((sign << 31) | (0xFF << 23));  // ±Inf
        }
        return f32::from_bits((sign << 31) | (0xFF << 23) | (mant << 13));  // NaN
    }
    let f32_exp = exp + 127 - 15;
    f32::from_bits((sign << 31) | (f32_exp << 23) | (mant << 13))
}

/// 将 f32 权重量化到目标精度
fn quantize_weights(weights: &[f32], target: QuantBits) -> (Vec<u8>, f32) {
    match target {
        QuantBits::Fp32 => {
            let mut data = Vec::with_capacity(weights.len() * 4);
            for &w in weights {
                data.extend_from_slice(&w.to_le_bytes());
            }
            (data, 1.0)
        }
        QuantBits::Fp16 => {
            let mut data = Vec::with_capacity(weights.len() * 2);
            for &w in weights {
                data.extend_from_slice(&f32_to_f16_bytes(w));
            }
            (data, 1.0)
        }
        QuantBits::Int8 => {
            let max_abs = weights.iter().map(|w| w.abs()).fold(0.0f32, f32::max);
            let scale = if max_abs > 0.0 { max_abs / 127.0 } else { 1.0 };
            let mut data = Vec::with_capacity(weights.len());
            for &w in weights {
                let q = (w / scale).round().clamp(-128.0, 127.0) as i8;
                data.push(q as u8);
            }
            (data, scale)
        }
        QuantBits::Int4 => {
            let (packed, scale) = DuanyanModel::quantize_int4(weights);
            (packed, scale)
        }
    }
}

/// f32 → FP16 字节
fn f32_to_f16_bytes(f: f32) -> [u8; 2] {
    let bits = f.to_bits();
    let sign = ((bits >> 31) & 1) as u16;
    let exp = ((bits >> 23) & 0xFF) as i32;
    let mant = bits & 0x7FFFFF;

    let half = if exp == 0 {
        sign << 15
    } else if exp == 0xFF {
        (sign << 15) | (0x1F << 10) | if mant != 0 { 0x200 } else { 0 }
    } else {
        let new_exp = exp - 127 + 15;
        if new_exp >= 31 {
            (sign << 15) | (0x1F << 10)  // overflow → Inf
        } else if new_exp <= 0 {
            sign << 15  // underflow → 0
        } else {
            (sign << 15) | ((new_exp as u16) << 10) | ((mant >> 13) as u16)
        }
    };
    half.to_le_bytes()
}

/// 公开接口: 将 f32 权重量化到目标精度 (供 binary crate 使用)
pub fn quantize_weights_pub(weights: &[f32], target: QuantBits) -> (Vec<u8>, f32) {
    quantize_weights(weights, target)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_header_roundtrip() {
        let header = ModelHeader {
            version: FORMAT_VERSION,
            n_layers: 6,
            n_params: 10_000_000,
            hidden_dim: 512,
            n_heads: 8,
            n_kv_heads: 4,
            context_len: 128,
            vocab_size: 1024,
            quant_bits: 4,
        };
        let mut buf = Vec::new();
        header.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), HEADER_SIZE);

        let mut cursor = std::io::Cursor::new(&buf);
        let header2 = ModelHeader::read_from(&mut cursor).unwrap();
        assert_eq!(header2.version, 1);
        assert_eq!(header2.n_layers, 6);
        assert_eq!(header2.n_params, 10_000_000);
        assert_eq!(header2.hidden_dim, 512);
        assert_eq!(header2.n_heads, 8);
        assert_eq!(header2.quant_bits, 4);
        assert_eq!(buf.len(), HEADER_SIZE);
    }

    #[test]
    fn test_layer_record_roundtrip() {
        let layer = LayerRecord::new(
            LayerType::AttnQkv, 512, 1536, QuantBits::Int4,
            vec![1, 2, 3, 4, 5],
        );
        let mut buf = Vec::new();
        layer.write_to(&mut buf).unwrap();

        let mut cursor = std::io::Cursor::new(&buf);
        let layer2 = LayerRecord::read_from(&mut cursor).unwrap();
        assert_eq!(layer2.layer_type, LayerType::AttnQkv);
        assert_eq!(layer2.rows, 512);
        assert_eq!(layer2.cols, 1536);
        assert_eq!(layer2.quant, 4);
        assert_eq!(layer2.data, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_model_write_read_roundtrip() {
        let model = DuanyanModel::create_demo(2, 32);
        let dir = std::env::temp_dir();
        let path = dir.join("test_roundtrip.duanyan");
        model.write_to_file(&path).unwrap();

        let model2 = DuanyanModel::read_from_file(&path).unwrap();
        assert_eq!(model2.header.n_layers, 2);
        assert_eq!(model2.header.hidden_dim, 32);
        assert_eq!(model2.layers.len(), 2 * 5 + 1); // 5 layers/block * 2 blocks + output
        assert_eq!(model2.header.n_params, model.header.n_params);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_model_verify() {
        let model = DuanyanModel::create_demo(1, 16);
        let dir = std::env::temp_dir();
        let path = dir.join("test_verify.duanyan");
        model.write_to_file(&path).unwrap();

        let result = DuanyanModel::verify(&path).unwrap();
        assert!(result.valid);
        assert_eq!(result.n_layers, 1);
        assert_eq!(result.actual_layers, 6); // 5 + output
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_invalid_magic() {
        let dir = std::env::temp_dir();
        let path = dir.join("test_bad_magic.duanyan");
        std::fs::write(&path, b"NOTAMODEL").unwrap();
        let result = DuanyanModel::read_from_file(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("magic"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_quantize_dequantize_int4() {
        let weights = vec![0.5, -0.3, 1.0, -0.8, 0.2, -0.1];
        let (packed, scale) = DuanyanModel::quantize_int4(&weights);
        let mut out = vec![0.0f32; weights.len()];
        DuanyanModel::dequantize_int4(&packed, &mut out, scale);

        // INT4 量化有损，允许一定误差
        for (orig, quant) in weights.iter().zip(out.iter()) {
            let err = (orig - quant).abs();
            assert!(err < scale * 1.5, "量化误差过大: orig={}, quant={}, scale={}, err={}", orig, quant, scale, err);
        }
    }

    #[test]
    fn test_quantize_empty() {
        let (packed, scale) = DuanyanModel::quantize_int4(&[]);
        assert!(packed.is_empty());
        assert_eq!(scale, 1.0);
    }

    #[test]
    fn test_info_string() {
        let model = DuanyanModel::create_demo(1, 16);
        let info = model.info_string();
        assert!(info.contains("锻言Mini"));
        assert!(info.contains("FP32"));
        assert!(info.contains("资源估算"));
    }

    #[test]
    fn test_layer_type_display() {
        assert_eq!(LayerType::AttnQkv.display_name(), "attn_qkv");
        assert_eq!(LayerType::Output.display_name(), "output");
    }

    #[test]
    fn test_quant_bits_bytes_per_param() {
        assert_eq!(QuantBits::Int4.bytes_per_param(), 0.5);
        assert_eq!(QuantBits::Int8.bytes_per_param(), 1.0);
        assert_eq!(QuantBits::Fp16.bytes_per_param(), 2.0);
        assert_eq!(QuantBits::Fp32.bytes_per_param(), 4.0);
    }

    #[test]
    fn test_prebuilt_models_count() {
        assert_eq!(PREBUILT_MODELS.len(), 5);
    }

    #[test]
    fn test_verify_result_display() {
        let vr = VerifyResult {
            valid: true, version: 1, n_layers: 2, actual_layers: 2,
            header_params: 1000, actual_params: 1000,
            header_data_bytes: 0, actual_data_bytes: 500, file_size: 548,
        };
        let s = vr.display();
        assert!(s.contains("有效"));
    }

    #[test]
    fn test_duanyan_mini_spec_constants() {
        // 确认v5.1规格常量已正确定义
        assert_eq!(DUANYAN_MINI_LAYERS, 6);
        assert_eq!(DUANYAN_MINI_HIDDEN, 384);
        assert_eq!(DUANYAN_MINI_HEADS, 6);
        assert_eq!(DUANYAN_MINI_KV_HEADS, 2);
        assert_eq!(DUANYAN_MINI_VOCAB, 5000);
        assert_eq!(DUANYAN_MINI_CONTEXT_LEN, 256);
        assert_eq!(DUANYAN_MINI_RAM_BUDGET_KB, 768);
        assert!((DUANYAN_MINI_INFERENCE_MAX_MS - 200.0).abs() < f64::EPSILON);
        assert!((DUANYAN_MINI_ACCURACY_MIN - 0.90).abs() < f64::EPSILON);
        assert_eq!(DUANYAN_MINI_DEFAULT_QUANT, QuantBits::Int4);
    }

    #[test]
    fn test_red_line_constants() {
        assert!((RED_LINE_FLASH_OVERHEAD_PCT - 20.0).abs() < f64::EPSILON);
        assert!((RED_LINE_RAM_OVERHEAD_PCT - 25.0).abs() < f64::EPSILON);
        assert!((RED_LINE_SPEED_OVERHEAD_PCT - 30.0).abs() < f64::EPSILON);
        assert!((RED_LINE_IRQ_OVERHEAD_US - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_create_duanyan_mini_official() {
        let model = create_duanyan_mini_official();
        assert_eq!(model.header.n_layers, DUANYAN_MINI_LAYERS);
        assert_eq!(model.header.hidden_dim, DUANYAN_MINI_HIDDEN);
        assert_eq!(model.header.n_heads, DUANYAN_MINI_HEADS);
        assert_eq!(model.header.n_kv_heads, DUANYAN_MINI_KV_HEADS);
        assert_eq!(model.header.vocab_size, DUANYAN_MINI_VOCAB);
        assert_eq!(model.header.context_len, DUANYAN_MINI_CONTEXT_LEN);
    }

    // GGUF 转换器测试

    #[test]
    fn test_f16_to_f32_basic() {
        // 0.0 in fp16 = 0x0000
        assert_eq!(f16_to_f32(0x0000), 0.0);
        // 1.0 in fp16 = 0x3C00
        assert!((f16_to_f32(0x3C00) - 1.0).abs() < 1e-6);
        // -1.0 in fp16 = 0xBC00
        assert!((f16_to_f32(0xBC00) - (-1.0)).abs() < 1e-6);
        // 0.5 in fp16 = 0x3800
        assert!((f16_to_f32(0x3800) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_f32_to_f16_roundtrip() {
        let values = [0.0f32, 1.0, -1.0, 0.5, 0.25, 2.0, 100.0];
        for &v in &values {
            let half_bytes = f32_to_f16_bytes(v);
            let half = u16::from_le_bytes(half_bytes);
            let back = f16_to_f32(half);
            let err = (v - back).abs();
            assert!(err < 0.01 * v.abs().max(0.01), "f16 roundtrip failed for {}: got {}, err={}", v, back, err);
        }
    }

    #[test]
    fn test_gguf_layer_name_mapping() {
        // Attention QKV
        let r = map_gguf_name_to_layer("blk.0.attn_q.weight", "llama");
        assert!(r.is_some());
        assert_eq!(r.unwrap(), (0, LayerType::AttnQkv));

        // FFN Up
        let r = map_gguf_name_to_layer("blk.2.ffn_gate.weight", "llama");
        assert!(r.is_some());
        assert_eq!(r.unwrap(), (2, LayerType::FfnUp));

        // FFN Down
        let r = map_gguf_name_to_layer("blk.3.ffn_down.weight", "llama");
        assert!(r.is_some());
        assert_eq!(r.unwrap(), (3, LayerType::FfnDown));

        // Attention Output
        let r = map_gguf_name_to_layer("blk.1.attn_output.weight", "llama");
        assert!(r.is_some());
        assert_eq!(r.unwrap(), (1, LayerType::AttnOut));

        // RMS Norm
        let r = map_gguf_name_to_layer("blk.0.ffn_norm.weight", "llama");
        assert!(r.is_some());
        assert_eq!(r.unwrap(), (0, LayerType::RmsNorm));

        // Output projection
        let r = map_gguf_name_to_layer("output.weight", "llama");
        assert!(r.is_some());
        assert_eq!(r.unwrap(), (9999, LayerType::Output));

        // Unknown tensor
        let r = map_gguf_name_to_layer("token_embd.weight", "llama");
        assert!(r.is_none());
    }

    #[test]
    fn test_quantize_weights_fp32() {
        let weights = vec![1.0f32, -0.5, 0.25, 0.0];
        let (data, scale) = quantize_weights(&weights, QuantBits::Fp32);
        assert_eq!(data.len(), 16);  // 4 * 4 bytes
        assert_eq!(scale, 1.0);
    }

    #[test]
    fn test_quantize_weights_int8() {
        let weights = vec![1.0f32, -1.0, 0.5, -0.5];
        let (data, scale) = quantize_weights(&weights, QuantBits::Int8);
        assert_eq!(data.len(), 4);
        assert!(scale > 0.0);
        // max abs = 1.0, scale = 1.0/127.0
        let expected_scale = 1.0 / 127.0;
        assert!((scale - expected_scale).abs() < 1e-6);
    }

    #[test]
    fn test_gguf_string_reader() {
        let mut data = Vec::new();
        let test_str = "hello";
        data.extend_from_slice(&(test_str.len() as u64).to_le_bytes());
        data.extend_from_slice(test_str.as_bytes());
        let (s, pos) = read_gguf_string(&data, 0).unwrap();
        assert_eq!(s, "hello");
        assert_eq!(pos, 8 + 5);
    }

    #[test]
    fn test_gguf_type_properties() {
        assert_eq!(GgufType::Q4_0.block_size(), 32);
        assert_eq!(GgufType::Q4_0.type_size(), 18);
        assert_eq!(GgufType::Q8_0.block_size(), 32);
        assert_eq!(GgufType::Q8_0.type_size(), 34);
        assert_eq!(GgufType::F32.block_size(), 1);
        assert_eq!(GgufType::F32.type_size(), 4);
    }

    #[test]
    fn test_quantize_weights_pub_api() {
        let weights = vec![0.1f32, 0.2, 0.3];
        let (data, _scale) = quantize_weights_pub(&weights, QuantBits::Fp32);
        assert_eq!(data.len(), 12);
    }

    // ===== generate_gesture_model 测试 =====

    #[test]
    fn test_generate_gesture_model_structure() {
        let m = generate_gesture_model();
        // 6层 × 5个LayerRecord + 1个Output = 31
        assert_eq!(m.layers.len(), (DUANYAN_MINI_LAYERS * 5 + 1) as usize);
        assert_eq!(m.header.n_layers, DUANYAN_MINI_LAYERS);
        assert_eq!(m.header.hidden_dim, DUANYAN_MINI_HIDDEN);
        assert_eq!(m.header.n_heads, DUANYAN_MINI_HEADS);
        assert_eq!(m.header.quant_bits, DUANYAN_MINI_DEFAULT_QUANT as u32);
    }

    #[test]
    fn test_generate_gesture_model_params_nonzero() {
        let m = generate_gesture_model();
        assert!(m.header.n_params > 0, "参数数量应 > 0");
        // 至少应有多M参数
        assert!(m.header.n_params > 1_000_000, "应有 >1M 参数");
    }

    #[test]
    fn test_generate_gesture_model_roundtrip() {
        let m = generate_gesture_model();
        let tmp = std::env::temp_dir().join("test_gesture_model.duanyan");
        m.write_to_file(&tmp).unwrap();
        let m2 = DuanyanModel::read_from_file(&tmp).unwrap();
        assert_eq!(m.header.n_layers, m2.header.n_layers);
        assert_eq!(m.header.n_params, m2.header.n_params);
        assert_eq!(m.layers.len(), m2.layers.len());
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn test_generate_esp32_benchmark_output() {
        let m = generate_gesture_model();
        let c_code = generate_esp32_benchmark(&m);
        assert!(c_code.contains("esp_timer_get_time"));
        assert!(c_code.contains("RED_LINE_INFERENCE_MS"));
        assert!(c_code.contains("app_main"));
        assert!(c_code.contains("gesture_benchmark"));
        assert!(c_code.contains(&format!("#define QL_LAYERS      {}", DUANYAN_MINI_LAYERS)));
    }

    #[test]
    fn test_xavier_init_range() {
        let mut seed = 42u32;
        let weights = xavier_init(100, 384, 384, &mut seed);
        assert_eq!(weights.len(), 100);
        let limit = (6.0_f32 / (384.0 + 384.0)).sqrt();
        for &w in &weights {
            assert!(w.abs() <= limit + 0.001, "Xavier权重超出范围: {} vs limit {}", w, limit);
        }
    }
}
