//! Phase W4: 内存与缓存确定性
//!
//! 从"不 malloc"升级到"可证明的内存安全"：
//! - 栈溢出形式化证明 (调用图 + 每帧大小 → 最大栈使用)
//! - Cache 行为分析 (L1 hit/miss 建模, WCET cache penalty)
//! - DMA 安全模型 (缓冲区重叠检测)
//! - MPU 内存分区配置生成
//! - 多任务数据竞争检测

// =============================================================================
// 栈溢出形式化证明
// =============================================================================

/// 调用图节点
#[derive(Debug, Clone)]
pub struct CallGraphNode {
    /// 函数名
    pub name: String,
    /// 本帧栈使用 (bytes)
    pub frame_size: usize,
    /// 被调用函数列表
    pub callees: Vec<String>,
    /// 是否递归
    pub is_recursive: bool,
}

/// 调用图
#[derive(Debug, Clone)]
pub struct CallGraph {
    pub nodes: Vec<CallGraphNode>,
    pub entry: String,
}

impl CallGraph {
    pub fn new(entry: &str) -> Self {
        Self { nodes: Vec::new(), entry: entry.to_string() }
    }

    pub fn add_node(&mut self, name: &str, frame_size: usize, callees: Vec<String>) {
        self.nodes.push(CallGraphNode {
            name: name.to_string(),
            frame_size,
            callees,
            is_recursive: false,
        });
    }

    fn find_node(&self, name: &str) -> Option<&CallGraphNode> {
        self.nodes.iter().find(|n| n.name == name)
    }

    /// 计算从入口到叶子的最大栈深度
    pub fn max_stack_depth(&self) -> StackDepthResult {
        let mut visited = std::collections::HashSet::new();
        let (depth, path) = self.dfs_max_depth(&self.entry, &mut visited);
        StackDepthResult {
            max_depth_bytes: depth,
            critical_path: path,
            has_recursion: self.detect_recursion(),
        }
    }

    fn dfs_max_depth(&self, func: &str, visited: &mut std::collections::HashSet<String>) -> (usize, Vec<(String, usize)>) {
        if visited.contains(func) {
            return (0, vec![]); // 防止递归无限循环
        }
        visited.insert(func.to_string());

        let node = match self.find_node(func) {
            Some(n) => n,
            None => return (0, vec![]),
        };

        let mut max_child_depth = 0;
        let mut max_child_path: Vec<(String, usize)> = vec![];

        for callee in &node.callees {
            let (child_depth, child_path) = self.dfs_max_depth(callee, visited);
            if child_depth > max_child_depth {
                max_child_depth = child_depth;
                max_child_path = child_path;
            }
        }

        visited.remove(func);

        let mut path = vec![(node.name.clone(), node.frame_size)];
        path.extend(max_child_path);
        (node.frame_size + max_child_depth, path)
    }

    fn detect_recursion(&self) -> bool {
        // 简单检测: 是否存在 A → ... → A 的路径
        for node in &self.nodes {
            let mut visited = std::collections::HashSet::new();
            if self.has_cycle(&node.name, &node.name, &mut visited) {
                return true;
            }
        }
        false
    }

    fn has_cycle(&self, start: &str, current: &str, visited: &mut std::collections::HashSet<String>) -> bool {
        if visited.contains(current) { return false; }
        visited.insert(current.to_string());

        if let Some(node) = self.find_node(current) {
            for callee in &node.callees {
                if callee == start { return true; }
                if self.has_cycle(start, callee, visited) { return true; }
            }
        }
        false
    }
}

/// 栈深度分析结果
#[derive(Debug, Clone)]
pub struct StackDepthResult {
    /// 最大栈使用 (bytes)
    pub max_depth_bytes: usize,
    /// 关键路径 (函数名, 帧大小)
    pub critical_path: Vec<(String, usize)>,
    /// 是否有递归
    pub has_recursion: bool,
}

/// 栈安全证明
#[derive(Debug, Clone)]
pub struct StackSafetyProof {
    /// 任务名
    pub task_name: String,
    /// 最大栈使用
    pub max_usage: usize,
    /// 分配栈大小
    pub allocated: usize,
    /// 安全余量
    pub margin: usize,
    /// 余量百分比
    pub margin_percent: f64,
    /// 是否安全
    pub safe: bool,
    /// 关键路径
    pub critical_path: Vec<(String, usize)>,
    /// 形式化证明文本
    pub proof: String,
}

/// 执行栈安全证明
pub fn prove_stack_safety_formal(task_name: &str, call_graph: &CallGraph, allocated: usize) -> StackSafetyProof {
    let result = call_graph.max_stack_depth();
    let margin = if allocated > result.max_depth_bytes {
        allocated - result.max_depth_bytes
    } else {
        0
    };
    let margin_pct = if allocated > 0 {
        margin as f64 / allocated as f64 * 100.0
    } else {
        0.0
    };
    let safe = result.max_depth_bytes < allocated && !result.has_recursion;

    let path_str: Vec<String> = result.critical_path.iter()
        .map(|(name, size)| format!("{}({}B)", name, size))
        .collect();

    let proof = format!(
        "THEOREM stack_safe({})\n  max_stack = {} bytes\n  allocated = {} bytes\n  path: {}\n  recursion: {}\n  QED: {} {} {} → {}",
        task_name,
        result.max_depth_bytes,
        allocated,
        path_str.join(" → "),
        if result.has_recursion { "DETECTED (UNSAFE)" } else { "none" },
        result.max_depth_bytes,
        if safe { "<" } else { ">=" },
        allocated,
        if safe { "SAFE" } else { "OVERFLOW RISK" },
    );

    StackSafetyProof {
        task_name: task_name.to_string(),
        max_usage: result.max_depth_bytes,
        allocated,
        margin,
        margin_percent: margin_pct,
        safe,
        critical_path: result.critical_path,
        proof,
    }
}

// =============================================================================
// Cache 行为分析
// =============================================================================

/// Cache 配置
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// L1 数据缓存大小 (bytes)
    pub l1_size: usize,
    /// 缓存行大小 (bytes)
    pub line_size: usize,
    /// 组相联度
    pub associativity: usize,
    /// 是否有 L2
    pub has_l2: bool,
    /// L2 大小 (bytes)
    pub l2_size: usize,
}

impl CacheConfig {
    pub fn esp32s3() -> Self {
        Self { l1_size: 16384, line_size: 32, associativity: 4, has_l2: false, l2_size: 0 }
    }

    pub fn stm32f4() -> Self {
        // Cortex-M4: 无 L1 cache (紧密耦合内存)
        Self { l1_size: 0, line_size: 0, associativity: 0, has_l2: false, l2_size: 0 }
    }

    pub fn cortex_m7() -> Self {
        Self { l1_size: 16384, line_size: 32, associativity: 4, has_l2: false, l2_size: 0 }
    }

    pub fn raspberry_pi4() -> Self {
        Self { l1_size: 32768, line_size: 64, associativity: 2, has_l2: true, l2_size: 1048576 }
    }

    pub fn has_cache(&self) -> bool {
        self.l1_size > 0
    }

    /// 缓存集数
    pub fn num_sets(&self) -> usize {
        if self.l1_size == 0 || self.line_size == 0 || self.associativity == 0 { return 0; }
        self.l1_size / (self.line_size * self.associativity)
    }
}

/// 内存区域
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    pub name: String,
    pub base_addr: u32,
    pub size: usize,
    pub access_frequency: u32, // 每周期访问次数
}

/// Cache 分析结果
#[derive(Debug, Clone)]
pub struct CacheAnalysis {
    /// 是否可分析 (无 cache 的平台跳过)
    pub analyzable: bool,
    /// 工作集大小 (bytes)
    pub working_set: usize,
    /// L1 命中率估算
    pub l1_hit_rate: f64,
    /// Cache miss penalty (cycles per miss)
    pub miss_penalty_cycles: u32,
    /// 每迭代 cache penalty (cycles)
    pub per_iteration_penalty: u64,
    /// WCET 附加 (cycles)
    pub wcet_addition_cycles: u64,
    /// 建议
    pub recommendations: Vec<String>,
}

/// 分析循环的 cache 行为
pub fn analyze_cache_behavior(
    config: &CacheConfig,
    working_set_bytes: usize,
    loop_iterations: u64,
    miss_penalty: u32,
) -> CacheAnalysis {
    if !config.has_cache() {
        return CacheAnalysis {
            analyzable: false,
            working_set: working_set_bytes,
            l1_hit_rate: 1.0,
            miss_penalty_cycles: 0,
            per_iteration_penalty: 0,
            wcet_addition_cycles: 0,
            recommendations: vec!["No data cache on this platform (TCM/scratchpad)".into()],
        };
    }

    let hit_rate = if working_set_bytes <= config.l1_size {
        // 工作集 fits in L1
        0.99
    } else if working_set_bytes <= config.l1_size * 2 {
        0.90
    } else {
        0.75
    };

    let miss_rate = 1.0 - hit_rate;
    let accesses_per_iter = working_set_bytes / config.line_size.max(1);
    let misses_per_iter = (accesses_per_iter as f64 * miss_rate).ceil() as u64;
    let per_iter_penalty = misses_per_iter * miss_penalty as u64;
    let total_penalty = per_iter_penalty * loop_iterations;

    let mut recommendations = Vec::new();
    if working_set_bytes > config.l1_size {
        recommendations.push(format!(
            "Working set ({}B) exceeds L1 ({}B): consider loop tiling",
            working_set_bytes, config.l1_size
        ));
    }
    if hit_rate < 0.95 {
        recommendations.push("Consider data layout optimization for cache locality".into());
    }

    CacheAnalysis {
        analyzable: true,
        working_set: working_set_bytes,
        l1_hit_rate: hit_rate,
        miss_penalty_cycles: miss_penalty,
        per_iteration_penalty: per_iter_penalty,
        wcet_addition_cycles: total_penalty,
        recommendations,
    }
}

// =============================================================================
// DMA 安全模型
// =============================================================================

/// DMA 缓冲区
#[derive(Debug, Clone)]
pub struct DmaBuffer {
    pub name: String,
    pub base_addr: u32,
    pub size: usize,
    /// 是否被 CPU 同时访问
    pub cpu_accessed: bool,
}

/// DMA 安全检查结果
#[derive(Debug, Clone)]
pub struct DmaSafetyResult {
    /// 是否安全 (无重叠)
    pub safe: bool,
    /// 重叠对
    pub overlaps: Vec<(String, String)>,
    /// 建议
    pub recommendations: Vec<String>,
}

/// 检查 DMA 缓冲区安全性
pub fn check_dma_safety(buffers: &[DmaBuffer]) -> DmaSafetyResult {
    let mut overlaps = Vec::new();
    let mut recommendations = Vec::new();

    for i in 0..buffers.len() {
        for j in (i + 1)..buffers.len() {
            let a = &buffers[i];
            let b = &buffers[j];
            let a_end = a.base_addr + a.size as u32;
            let b_end = b.base_addr + b.size as u32;

            if a.base_addr < b_end && b.base_addr < a_end {
                overlaps.push((a.name.clone(), b.name.clone()));
                if a.cpu_accessed || b.cpu_accessed {
                    recommendations.push(format!(
                        "CRITICAL: DMA buffers '{}' and '{}' overlap with CPU access — data corruption risk",
                        a.name, b.name
                    ));
                }
            }
        }
    }

    if overlaps.is_empty() {
        recommendations.push("All DMA buffers are non-overlapping: SAFE".into());
    }

    DmaSafetyResult {
        safe: overlaps.is_empty(),
        overlaps,
        recommendations,
    }
}

// =============================================================================
// MPU 内存分区
// =============================================================================

/// MPU 区域
#[derive(Debug, Clone)]
pub struct MpuRegion {
    pub name: String,
    pub base_addr: u32,
    pub size: u32, // 必须是 2 的幂
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub cacheable: bool,
}

/// 生成 MPU 配置 C 代码 (Cortex-M)
pub fn generate_mpu_config(regions: &[MpuRegion]) -> String {
    let mut code = String::new();
    code.push_str("/* DUANYAN W4: MPU Memory Protection Configuration */\n");
    code.push_str("/* Auto-generated — do not edit */\n\n");
    code.push_str("#include \"stm32f4xx.h\"\n\n");
    code.push_str("void _duanyan_mpu_init(void) {\n");
    code.push_str("    MPU->CTRL = 0; /* Disable MPU */\n\n");

    for (i, region) in regions.iter().enumerate().take(8) {
        let size_attr = match region.size {
            32 => 4, 64 => 5, 128 => 6, 256 => 7,
            512 => 8, 1024 => 9, 2048 => 10, 4096 => 11,
            8192 => 12, 16384 => 13, 32768 => 14, 65536 => 15,
            131072 => 16, 262144 => 17, 524288 => 18, 1048576 => 19,
            _ => 11, // 默认 4KB
        };
        let mut attrs: u32 = (size_attr << 1) | 1; // ENABLE bit
        if !region.cacheable { attrs |= 0x02; } // Shareable
        let ap = if region.writable { 0x03 << 24 } else { 0x05 << 24 }; // Full access / Read-only
        let xn = if !region.executable { 1u32 << 28 } else { 0 };

        code.push_str(&format!("    /* Region {}: {} (0x{:08X}, {}B) */\n",
            i, region.name, region.base_addr, region.size));
        code.push_str(&format!("    MPU->RNR = {};\n", i));
        code.push_str(&format!("    MPU->RBAR = 0x{:08X};\n", region.base_addr));
        code.push_str(&format!("    MPU->RASR = 0x{:08X};\n\n", attrs | ap | xn));
    }

    code.push_str("    MPU->CTRL = 5; /* Enable MPU + background region */\n");
    code.push_str("    __DSB();\n    __ISB();\n}\n");

    code
}

// =============================================================================
// 数据竞争检测
// =============================================================================

/// 共享变量
#[derive(Debug, Clone)]
pub struct SharedVariable {
    pub name: String,
    /// 写入该变量的任务
    pub writers: Vec<String>,
    /// 读取该变量的任务
    pub readers: Vec<String>,
    /// 是否有保护 (原子/互斥/无锁)
    pub protected: bool,
}

/// 竞争检测结果
#[derive(Debug, Clone)]
pub struct RaceDetectionResult {
    /// 检测到的竞争
    pub races: Vec<DataRace>,
    /// 是否无竞争
    pub race_free: bool,
    /// 证明文本
    pub proof: String,
}

#[derive(Debug, Clone)]
pub struct DataRace {
    pub variable: String,
    pub task_a: String,
    pub task_b: String,
    pub kind: RaceKind,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RaceKind {
    /// 写-写竞争
    WriteWrite,
    /// 读-写竞争
    ReadWrite,
}

/// 检测数据竞争
pub fn detect_data_races(variables: &[SharedVariable]) -> RaceDetectionResult {
    let mut races = Vec::new();

    for var in variables {
        if var.protected { continue; }

        // 写-写: 多个写者
        if var.writers.len() > 1 {
            for i in 0..var.writers.len() {
                for j in (i + 1)..var.writers.len() {
                    races.push(DataRace {
                        variable: var.name.clone(),
                        task_a: var.writers[i].clone(),
                        task_b: var.writers[j].clone(),
                        kind: RaceKind::WriteWrite,
                    });
                }
            }
        }

        // 读-写: 有写者且有读者 (不同任务)
        for writer in &var.writers {
            for reader in &var.readers {
                if writer != reader {
                    races.push(DataRace {
                        variable: var.name.clone(),
                        task_a: writer.clone(),
                        task_b: reader.clone(),
                        kind: RaceKind::ReadWrite,
                    });
                }
            }
        }
    }

    let race_free = races.is_empty();
    let proof = if race_free {
        "PROOF: No unprotected shared variables with concurrent access → RACE-FREE".to_string()
    } else {
        format!("WARNING: {} potential data race(s) detected", races.len())
    };

    RaceDetectionResult { races, race_free, proof }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_graph_max_depth() {
        let mut cg = CallGraph::new("main");
        cg.add_node("main", 64, vec!["motorCtrl".into(), "sensorRead".into()]);
        cg.add_node("motorCtrl", 128, vec!["pidCompute".into()]);
        cg.add_node("sensorRead", 96, vec!["spiTransfer".into()]);
        cg.add_node("pidCompute", 48, vec![]);
        cg.add_node("spiTransfer", 32, vec![]);

        let result = cg.max_stack_depth();
        // main(64) → motorCtrl(128) → pidCompute(48) = 240
        assert_eq!(result.max_depth_bytes, 240);
        assert!(!result.has_recursion);
        assert_eq!(result.critical_path.len(), 3);
    }

    #[test]
    fn test_call_graph_recursion_detection() {
        let mut cg = CallGraph::new("foo");
        cg.add_node("foo", 32, vec!["bar".into()]);
        cg.add_node("bar", 32, vec!["foo".into()]); // 递归!

        let result = cg.max_stack_depth();
        assert!(result.has_recursion);
    }

    #[test]
    fn test_stack_safety_proof() {
        let mut cg = CallGraph::new("ctrl");
        cg.add_node("ctrl", 64, vec!["readADC".into()]);
        cg.add_node("readADC", 48, vec![]);

        let proof = prove_stack_safety_formal("ctrl", &cg, 2048);
        assert!(proof.safe);
        assert_eq!(proof.max_usage, 112);
        assert!(proof.margin > 1900);
        assert!(proof.proof.contains("SAFE"));
    }

    #[test]
    fn test_stack_safety_overflow() {
        let mut cg = CallGraph::new("deep");
        cg.add_node("deep", 1024, vec!["deeper".into()]);
        cg.add_node("deeper", 1024, vec!["deepest".into()]);
        cg.add_node("deepest", 512, vec![]);

        let proof = prove_stack_safety_formal("deep", &cg, 2048);
        assert!(!proof.safe);
        assert_eq!(proof.max_usage, 2560);
        assert!(proof.proof.contains("OVERFLOW RISK"));
    }

    #[test]
    fn test_cache_analysis_fits() {
        let config = CacheConfig::esp32s3();
        let result = analyze_cache_behavior(&config, 4096, 100, 10);
        assert!(result.analyzable);
        assert!(result.l1_hit_rate > 0.98);
        assert!(result.wcet_addition_cycles < 3000);
    }

    #[test]
    fn test_cache_analysis_exceeds() {
        let config = CacheConfig::esp32s3(); // 16KB L1
        let result = analyze_cache_behavior(&config, 65536, 1000, 10);
        assert!(result.l1_hit_rate < 0.80);
        assert!(result.wcet_addition_cycles > 0);
        assert!(!result.recommendations.is_empty());
    }

    #[test]
    fn test_cache_no_cache_platform() {
        let config = CacheConfig::stm32f4(); // 无 cache
        let result = analyze_cache_behavior(&config, 4096, 100, 10);
        assert!(!result.analyzable);
        assert_eq!(result.wcet_addition_cycles, 0);
    }

    #[test]
    fn test_dma_safety_no_overlap() {
        let buffers = vec![
            DmaBuffer { name: "tx_buf".into(), base_addr: 0x2000_0000, size: 256, cpu_accessed: false },
            DmaBuffer { name: "rx_buf".into(), base_addr: 0x2000_1000, size: 256, cpu_accessed: false },
        ];
        let result = check_dma_safety(&buffers);
        assert!(result.safe);
        assert!(result.overlaps.is_empty());
    }

    #[test]
    fn test_dma_safety_overlap() {
        let buffers = vec![
            DmaBuffer { name: "buf_a".into(), base_addr: 0x2000_0000, size: 512, cpu_accessed: true },
            DmaBuffer { name: "buf_b".into(), base_addr: 0x2000_0100, size: 512, cpu_accessed: false },
        ];
        let result = check_dma_safety(&buffers);
        assert!(!result.safe);
        assert_eq!(result.overlaps.len(), 1);
    }

    #[test]
    fn test_mpu_config_generation() {
        let regions = vec![
            MpuRegion { name: "FLASH".into(), base_addr: 0x0800_0000, size: 1048576, readable: true, writable: false, executable: true, cacheable: true },
            MpuRegion { name: "SRAM".into(), base_addr: 0x2000_0000, size: 131072, readable: true, writable: true, executable: false, cacheable: true },
            MpuRegion { name: "PERIPH".into(), base_addr: 0x4000_0000, size: 1048576, readable: true, writable: true, executable: false, cacheable: false },
        ];
        let code = generate_mpu_config(&regions);
        assert!(code.contains("MPU->RNR"));
        assert!(code.contains("FLASH"));
        assert!(code.contains("SRAM"));
        assert!(code.contains("__DSB()"));
    }

    #[test]
    fn test_race_detection_clean() {
        let vars = vec![
            SharedVariable { name: "speed".into(), writers: vec!["ctrl".into()], readers: vec!["display".into()], protected: false },
        ];
        // 单写多读 = SPSC, 如果只有一个写者且读者不写 → 需要看是否保护
        // 实际上单写多读在无锁下也可能有竞争 (撕裂读)
        let result = detect_data_races(&vars);
        assert!(!result.race_free); // 有读写竞争
    }

    #[test]
    fn test_race_detection_protected() {
        let vars = vec![
            SharedVariable { name: "speed".into(), writers: vec!["ctrl".into()], readers: vec!["display".into()], protected: true },
        ];
        let result = detect_data_races(&vars);
        assert!(result.race_free);
    }

    #[test]
    fn test_race_detection_write_write() {
        let vars = vec![
            SharedVariable { name: "cmd".into(), writers: vec!["taskA".into(), "taskB".into()], readers: vec![], protected: false },
        ];
        let result = detect_data_races(&vars);
        assert!(!result.race_free);
        assert_eq!(result.races[0].kind, RaceKind::WriteWrite);
    }
}
