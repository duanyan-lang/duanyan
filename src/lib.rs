//! HardyScript 编译器库
//!
//! JavaScript → COUNPRE64 机器码 / ESP32 C 代码 / Arduino sketch / STM32 HAL / RPi 的完整编译流水线
//! Phase E: STM32 + 树莓派后端 (AST → C → HAL/sysfs 项目)

pub mod lexer;
pub mod parser;
pub mod compiler;
pub mod backend;
pub mod optimizer;
pub mod engine;
pub mod flasher;
pub mod firmware;
pub mod config;
pub mod hardware_api;
pub mod hardware_sim;
pub mod bus_zx361;        // v1.6 P0: ZX361 总线舵机 ASCII 协议 (移植自 DUANYAN)
pub mod joint_api;        // v1.6 P0: 关节抽象 (Backend/JointSpec/路由, 镜像 DUANYAN)
pub mod robot_safety;     // v1.6 P1: 执行器安全护栏 (负载/温度/堵转, 镜像 DUANYAN safety_policy)
pub mod robot_motion;     // v1.6 P2: 机器人纯运动学/轨迹数学 (无门禁单一真源, robotics 下沉复用)
pub mod modeb_proto;      // v1.6 P3: Mode-B 线协议编解码 (镜像 DUANYAN mode_b_proto, 黄金帧字节一致)
pub mod board;
pub mod compat;
pub mod stdlib;
pub mod c_codegen;
pub mod esp32_project;
pub mod arduino_project;
pub mod stm32_project;
pub mod rpi_project;
pub mod model;
pub mod workspace;
pub mod git_ops;
pub mod lint;
pub mod ci;
pub mod async_transform;  // Phase v1.5: async/await 状态机变换
pub mod scheduler;        // Phase 1.3: 多任务调度器 (setInterval → FreeRTOS)
pub mod escape_analysis;  // Phase 2.1: 逃逸分析引擎
pub mod memory_pool;      // Phase 2.2: 静态内存池生成器
pub mod realtime_check;   // Phase 3.2: @realtime 编译器强制检查
pub mod cfg;              // Phase 4.1: 控制流图构建
pub mod wcet;             // Phase 4.2+4.3: WCET 分析引擎
pub mod wcet_advanced;    // Phase W1: WCET 精度提升 (外设延迟+中断干扰+循环推断)
pub mod rt_scheduler;     // Phase W2: 实时调度内核 (RMS/EDF+可调度性+优先级反转)
pub mod can_deterministic; // Phase W3: 汽车通信确定性 (CAN调度+E2E延迟+看门狗)
pub mod memory_determinism; // Phase W4: 内存与缓存确定性 (栈证明+Cache+DMA+MPU)
pub mod certification;    // Phase W5: 认证级输出 (MISRA C+ASIL+MC/DC+追踪矩阵)
// 行业垂直域模块: 演示广度模块, 默认不参与编译, 需要 --features domains
#[cfg(feature = "domains")]
pub mod industrial_control; // Phase IC: 工业控制 (IEC61131/EtherCAT/SIL3/运动控制)
#[cfg(feature = "domains")]
pub mod automotive;         // Phase AD: 自动驾驶 (CAN FD/混合关键性/Fail-Op/AUTOSAR)
#[cfg(feature = "domains")]
pub mod hardy_rtos;         // Phase RT: 嵌入式RTOS (微内核/IPC/BSP/形式化验证)
#[cfg(feature = "domains")]
pub mod avionics;           // Phase AV: 航空电子 (DO-178C/ARINC653/浮点确定/TMR)
#[cfg(feature = "domains")]
pub mod medical;            // Phase MD: 医疗器械 (IEC 62304/ISO 14971/故障安全)
#[cfg(feature = "domains")]
pub mod railway;            // Phase RW: 轨道交通 (EN 50128 SIL-4/联锁/安全通信)
#[cfg(feature = "domains")]
pub mod semiconductor;      // Phase SE: 半导体设备 (纳米定位/振动抑制/对准)
#[cfg(feature = "domains")]
pub mod education;          // Phase EDU: 教育平台 (课程/编译可视化/实验)
#[cfg(feature = "domains")]
pub mod robotics;           // Phase RB: 具身机器人 (运动学/力控/传感融合/安全协作)
pub mod language_intelligence; // DUANYAN 语言智能引擎 (诊断/补全/悬停/行业规则)

use std::path::Path;

/// 编译 JavaScript 源码为 COUNPRE64 机器码
pub fn compile_source(source: &str) -> Result<Vec<u32>, String> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let instructions = compiler::compile(&ast)?;
    let machine_code: Vec<u32> = instructions.iter().map(|i| i.encode()).collect();
    Ok(machine_code)
}

/// 编译 JavaScript 源码为 COUNPRE64 机器码（带优化）
pub fn compile_source_optimized(source: &str) -> Result<Vec<u32>, String> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let optimized_ast = optimizer::optimize(&ast);
    let instructions = compiler::compile(&optimized_ast)?;
    let machine_code: Vec<u32> = instructions.iter().map(|i| i.encode()).collect();
    Ok(machine_code)
}

/// Compile source into the internal instruction list.
pub fn compile_to_instructions(source: &str) -> Result<Vec<backend::Instruction>, String> {
    let tokens = lexer::tokenize(source)?;
    let ast = parser::parse(&tokens)?;
    let instructions = compiler::compile(&ast)?;
    Ok(instructions)
}

/// Build firmware bundle (hex + bin) from source and write it to `base_dir/{stem}.*`.
pub fn build_firmware(
    source: &str,
    base_dir: &Path,
    stem: &str,
) -> Result<firmware::FirmwareBundle, String> {
    let instructions = compile_to_instructions(source)?;
    firmware::emit_bundle(&instructions, base_dir, stem)
}

/// Compile source, package firmware, and flash to the specified target.
/// Returns the flasher's output message.
pub fn flash_source(
    source: &str,
    base_dir: &Path,
    stem: &str,
    target: &str,
) -> Result<String, String> {
    let bundle = build_firmware(source, base_dir, stem)?;
    let artifact = flasher::FirmwareArtifact {
        hex_path: bundle.hex_path,
        bin_path: Some(bundle.bin_path),
        verilog_path: None,
        entry_address: bundle.entry_address,
    };
    let target_kind = flasher::TargetKind::from_str(target)
        .ok_or_else(|| format!("Unknown target: {}", target))?;
    let target = flasher::Target { kind: target_kind, port: None };
    let flasher = flasher::create_flasher(&target);
    let result = flasher.flash(&artifact)?;
    if result.success {
        Ok(result.stdout)
    } else {
        Err(format!("{}", result.stderr))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_compile() {
        let source = "let a = 42;";
        let result = compile_source(source);
        assert!(result.is_ok());
        let code = result.unwrap();
        assert!(!code.is_empty());
    }

    #[test]
    fn test_optimized_compile() {
        let source = "let a = 3 + 4;";
        let result = compile_source_optimized(source);
        assert!(result.is_ok());
        let code = result.unwrap();
        // 优化后只需 ADDI x9, x0, 7 + halt = 2 条指令
        assert_eq!(code.len(), 2, "常量折叠后应只有 2 条指令");
    }

    #[test]
    fn test_console_log_compile() {
        let source = "console.log(100);";
        let result = compile_source(source);
        assert!(result.is_ok());
    }
}
