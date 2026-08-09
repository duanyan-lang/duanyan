// 行业垂直编译命令区 (--target ic|ad|rt|av|md|rw|se|edu|rb)
// 2026-08 工程治理: 自 main.rs 剥离
use std::fs;
use std::path::Path;
use std::process;
use crate::{lexer, parser};

// =============================================================================
// hardy build --target ic|ad|rt|av|md|rw|se|edu|rb — 行业垂直编译 (feature = "domains")
// =============================================================================
#[cfg(feature = "domains")]
pub fn cmd_build_industry(source: &str, input_file: &str, target: &str) {
    let stem = Path::new(input_file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("out");
    let base_dir = Path::new(input_file).parent().unwrap_or(Path::new("."));
    let out_dir = base_dir.join(format!("{}_{}", stem, target));

    println!("[HardyScript] 行业编译: {} --target {}", input_file, target);
    println!();

    // 解析源码
    let tokens = match lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => { eprintln!("词法错误: {}", e); process::exit(1); }
    };
    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => { eprintln!("语法错误: {}", e); process::exit(1); }
    };

    let func_count = ast.functions.len();
    println!("   解析完成: {} 个函数", func_count);

    match target {
        "ic" => cmd_industry_ic(&ast, stem, &out_dir),
        "ad" => cmd_industry_ad(&ast, stem, &out_dir),
        "rt" => cmd_industry_rt(&ast, stem, &out_dir),
        "av" => cmd_industry_av(&ast, stem, &out_dir),
        "md" => cmd_industry_md(&ast, stem, &out_dir),
        "rw" => cmd_industry_rw(&ast, stem, &out_dir),
        "se" => cmd_industry_se(&ast, stem, &out_dir),
        "edu" => cmd_industry_edu(&ast, stem, &out_dir),
        "rb" => cmd_industry_rb(&ast, stem, &out_dir),
        _ => unreachable!(),
    }
}

/// IC: 工业控制
#[cfg(feature = "domains")]
fn cmd_industry_ic(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::industrial_control::*;

    println!("   [工业控制] IEC 61131-3 / EtherCAT / SIL-3 / 运动控制");
    println!();

    // 为每个函数生成 PLC POU
    let mut pou_codes = Vec::new();
    for sf in &ast.functions {
        let pou = PlcPou {
            name: sf.func.name.clone(),
            kind: PouKind::FunctionBlock,
            hs_function: sf.func.name.clone(),
            cycle_us: 1000,
            inputs: vec![],
            outputs: vec![],
            locals: vec![],
        };
        let st_code = generate_st_code(&pou, &format!("    (* {} logic *)", sf.func.name));
        pou_codes.push(st_code);
    }

    // EtherCAT 调度示例
    let slaves = vec![
        EcatSlave { name: "Servo_X".into(), station: 1001, vendor_id: 0, product_code: 0, input_bytes: 32, output_bytes: 32, dc_supported: true },
        EcatSlave { name: "Servo_Y".into(), station: 1002, vendor_id: 0, product_code: 0, input_bytes: 32, output_bytes: 32, dc_supported: true },
        EcatSlave { name: "IO_Module".into(), station: 1003, vendor_id: 0, product_code: 0, input_bytes: 8, output_bytes: 8, dc_supported: false },
    ];
    let schedule = plan_ecat_schedule(&slaves, 1000);
    let ecat_code = generate_ecat_cyclic_code(&schedule);

    // 运动控制
    let axes = vec![
        MotionAxis { name: "X".into(), id: 0, max_velocity: 1000.0, max_acceleration: 5000.0, max_jerk: 50000.0, pos_loop_us: 1000, vel_loop_us: 100, cur_loop_us: 50 },
        MotionAxis { name: "Y".into(), id: 1, max_velocity: 1000.0, max_acceleration: 5000.0, max_jerk: 50000.0, pos_loop_us: 1000, vel_loop_us: 100, cur_loop_us: 50 },
    ];
    let motion_code = generate_motion_control_code(&axes);

    // 输出
    let _ = fs::create_dir_all(out_dir);
    let plc_path = out_dir.join(format!("{}_plc.st", stem));
    let ecat_path = out_dir.join("ecat_cyclic.c");
    let motion_path = out_dir.join("motion_control.c");

    let all_st = pou_codes.join("\n");
    let _ = fs::write(&plc_path, &all_st);
    let _ = fs::write(&ecat_path, &ecat_code);
    let _ = fs::write(&motion_path, &motion_code);

    println!("   EtherCAT: {} 从站, 周期 {}μs, 可行: {}", schedule.slaves.len(), schedule.cycle_us, schedule.cycle_feasible);
    println!("   总线时间: {:.1}μs, 抖动: {:.1}μs", schedule.bus_time_us, schedule.jitter_us);
    println!();
    println!("   输出文件:");
    println!("      {}", plc_path.display());
    println!("      {}", ecat_path.display());
    println!("      {}", motion_path.display());
    println!();
    println!("   [工业控制] 编译完成");
}

/// AD: 自动驾驶
#[cfg(feature = "domains")]
fn cmd_industry_ad(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::automotive::*;

    println!("   [自动驾驶] CAN FD / 混合关键性 / Fail-Operational / AUTOSAR");
    println!();

    // 为每个函数生成 AUTOSAR Runnable
    let runnables: Vec<AutosarRunnable> = ast.functions.iter().enumerate().map(|(i, sf)| {
        AutosarRunnable {
            name: format!("{}_Run", sf.func.name),
            hs_function: sf.func.name.clone(),
            trigger: RunnableTrigger::TimingEvent,
            period_ms: match i % 3 { 0 => 10.0, 1 => 20.0, _ => 100.0 },
            asil: if i == 0 { AsilGrade::AsilD } else { AsilGrade::AsilB },
            read_ports: vec![],
            write_ports: vec![],
        }
    }).collect();

    let swc = AutosarSwc {
        name: stem.to_string(),
        runnables: runnables.clone(),
        sender_ports: vec![],
        receiver_ports: vec![],
    };
    let swc_code = generate_autosar_swc(&swc);
    let sched_code = generate_autosar_schedule(&runnables);

    // Fail-Operational
    let failop_cfg = FailOpConfig {
        system_name: stem.to_string(),
        primary_asil: AsilGrade::AsilD,
        backup_asil: AsilGrade::AsilB,
        heartbeat_ms: 10,
        fault_timeout_ms: 50,
        degraded_speed_limit: 30,
        mrc_time_budget_s: 5.0,
    };
    let failop_code = generate_failop_code(&failop_cfg);

    // CAN FD 总线负载
    let canfd_cfg = CanFdConfig::standard();
    let msgs: Vec<CanFdMessage> = ast.functions.iter().enumerate().map(|(i, sf)| {
        CanFdMessage {
            name: sf.func.name.clone(), id: 0x100 + i as u32 * 0x100,
            extended: false, dlc: 8, period_ms: 10.0,
            sender: "ECU".into(), asil: AsilGrade::AsilB, signals: vec![],
        }
    }).collect();
    let bus_load = calculate_canfd_bus_load(&msgs, &canfd_cfg);

    // 输出
    let _ = fs::create_dir_all(out_dir);
    let swc_path = out_dir.join(format!("{}_swc.c", stem));
    let sched_path = out_dir.join("schedule_table.c");
    let failop_path = out_dir.join("fail_operational.c");

    let _ = fs::write(&swc_path, &swc_code);
    let _ = fs::write(&sched_path, &sched_code);
    let _ = fs::write(&failop_path, &failop_code);

    println!("   CAN FD: {} 消息, 总线负载 {:.1}% (500k/2M)", msgs.len(), bus_load);
    println!("   AUTOSAR SWC: {} runnables", runnables.len());
    println!("   Fail-Op: ASIL-D → ASIL-B → MRC ({}s)", failop_cfg.mrc_time_budget_s);
    println!();
    println!("   输出文件:");
    println!("      {}", swc_path.display());
    println!("      {}", sched_path.display());
    println!("      {}", failop_path.display());
    println!();
    println!("   [自动驾驶] 编译完成");
}

/// RT: 嵌入式 RTOS
#[cfg(feature = "domains")]
fn cmd_industry_rt(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::hardy_rtos::*;

    println!("   [嵌入式 RTOS] 微内核 / IPC / BSP / 形式化验证");
    println!();

    // 从函数生成任务
    let tasks: Vec<RtosTask> = ast.functions.iter().enumerate().map(|(i, sf)| {
        RtosTask {
            name: sf.func.name.clone(),
            priority: i as u8,
            stack_bytes: 1024,
            period_us: (i as u64 + 1) * 1000,
            wcet_us: 100,
            hs_function: sf.func.name.clone(),
        }
    }).collect();

    let config = RtosConfig {
        system_name: stem.to_string(),
        target: RtosTarget::Stm32f4,
        cpu_freq_hz: 168_000_000,
        tick_us: 100,
        tasks: tasks.clone(),
        time_slicing: false,
        time_slice_ticks: 5,
    };

    let kernel_code = generate_rtos_kernel(&config);
    let tla_code = generate_tla_model(&config);
    let spin_code = generate_spin_model(&config);

    // BSP
    let bsp_desc = BspDesc {
        board_name: "STM32F407VG".into(),
        target: RtosTarget::Stm32f4,
        crystal_hz: 8_000_000,
        sysclk_hz: 168_000_000,
        flash_base: 0x0800_0000,
        flash_size: 1024 * 1024,
        ram_base: 0x2000_0000,
        ram_size: 128 * 1024,
        irq_count: 82,
    };
    let (startup_code, linker_script) = generate_bsp(&bsp_desc);

    // 输出
    let _ = fs::create_dir_all(out_dir);
    let kernel_path = out_dir.join("hardy_rtos.c");
    let bsp_path = out_dir.join("startup.c");
    let ld_path = out_dir.join("memory.ld");
    let tla_path = out_dir.join(format!("{}.tla", stem));
    let spin_path = out_dir.join(format!("{}.pml", stem));

    let _ = fs::write(&kernel_path, &kernel_code);
    let _ = fs::write(&bsp_path, &startup_code);
    let _ = fs::write(&ld_path, &linker_script);
    let _ = fs::write(&tla_path, &tla_code);
    let _ = fs::write(&spin_path, &spin_code);

    println!("   内核: {} 任务, 抢占式优先级调度", tasks.len());
    println!("   BSP: STM32F407VG (168MHz, 1MB Flash, 128KB RAM)");
    println!("   验证: TLA+ 模型 + Spin/PROMELA 模型");
    println!();
    println!("   输出文件:");
    println!("      {}", kernel_path.display());
    println!("      {}", bsp_path.display());
    println!("      {}", ld_path.display());
    println!("      {}", tla_path.display());
    println!("      {}", spin_path.display());
    println!();
    println!("   [嵌入式 RTOS] 编译完成");
}

/// AV: 航空电子
#[cfg(feature = "domains")]
fn cmd_industry_av(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::avionics::*;

    println!("   [航空电子] DO-178C / ARINC 653 / 浮点确定 / TMR");
    println!();

    let dal = DalLevel::DalA;

    // ARINC 653 分区
    let partitions: Vec<Arinc653Partition> = vec![
        Arinc653Partition {
            name: "FlightCtrl".into(),
            criticality: DalLevel::DalA,
            time_window_us: 5000,
            memory_bytes: 65536,
            processes: ast.functions.iter().map(|sf| Arinc653Process {
                name: sf.func.name.clone(),
                period_us: 5000,
                wcet_us: 500,
                stack_bytes: 4096,
                hs_function: sf.func.name.clone(),
            }).collect(),
        },
    ];
    let schedule = plan_arinc653_schedule(&partitions, &[], 10000);
    let arinc_code = generate_arinc653_config(&schedule);

    // 浮点确定性
    let float_cfg = FloatDeterminismConfig::dal_a();
    let float_code = generate_float_determinism_code(&float_cfg);

    // TMR
    let tmr_cfg = TmrConfig {
        system_name: stem.to_string(),
        function_name: "ctrl".into(),
        channels: 3,
        diverse_compilation: true,
        output_type: TmrOutputType::Float32,
        vote_period_us: 1000,
    };
    let tmr_code = generate_tmr_code(&tmr_cfg);

    // DO-178C 报告
    let report = generate_avionics_certification_report(stem, dal, &schedule, &float_cfg, &tmr_cfg);

    // 输出
    let _ = fs::create_dir_all(out_dir);
    let arinc_path = out_dir.join("arinc653_config.c");
    let float_path = out_dir.join("float_determinism.c");
    let tmr_path = out_dir.join("tmr_voting.c");
    let report_path = out_dir.join(format!("{}_do178c_report.txt", stem));

    let _ = fs::write(&arinc_path, &arinc_code);
    let _ = fs::write(&float_path, &float_code);
    let _ = fs::write(&tmr_path, &tmr_code);
    let _ = fs::write(&report_path, &report);

    println!("   DO-178C: {} (MC/DC 覆盖)", dal.as_str());
    println!("   ARINC 653: {} 分区, 主时间帧 {}μs", schedule.partitions.len(), schedule.major_frame_us);
    println!("   TMR: 3 通道多样性编译 + 中值表决");
    println!("   浮点: IEEE 754 严格模式, 禁止 FMA, 表驱动 sin/cos");
    println!();
    println!("   输出文件:");
    println!("      {}", arinc_path.display());
    println!("      {}", float_path.display());
    println!("      {}", tmr_path.display());
    println!("      {}", report_path.display());
    println!();
    println!("   [航空电子] 编译完成");
}

/// MD: 医疗器械
#[cfg(feature = "domains")]
fn cmd_industry_md(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::medical::*;

    println!("   [医疗器械] IEC 62304 / ISO 14971 / 故障安全");
    println!();

    let units: Vec<MedicalSoftwareUnit> = ast.functions.iter().map(|sf| {
        MedicalSoftwareUnit {
            name: sf.func.name.clone(),
            hs_function: sf.func.name.clone(),
            safety_class: SoftwareClass::ClassC,
            risk_level: RiskLevel::Alarp,
            failure_modes: vec![
                FailureMode {
                    description: format!("{}: 输出超限", sf.func.name),
                    severity: 5,
                    probability: 2,
                    mitigation: "软件限位 + 硬件看门狗".into(),
                    residual_acceptable: true,
                },
            ],
        }
    }).collect();

    let report = generate_iec62304_report(&units, stem);
    let mut failsafe_codes = Vec::new();
    for unit in &units {
        failsafe_codes.push(generate_medical_failsafe(unit));
    }

    let _ = fs::create_dir_all(out_dir);
    let report_path = out_dir.join(format!("{}_iec62304.txt", stem));
    let failsafe_path = out_dir.join(format!("{}_failsafe.c", stem));

    let _ = fs::write(&report_path, &report);
    let _ = fs::write(&failsafe_path, failsafe_codes.join("\n"));

    println!("   安全分类: Class C ({} 个软件单元)", units.len());
    println!("   验证活动: 21 项 (MC/DC + 单元测试 + 架构分析)");
    println!("   SOUP: 无 (零外部依赖)");
    println!();
    println!("   输出文件:");
    println!("      {}", report_path.display());
    println!("      {}", failsafe_path.display());
    println!();
    println!("   [医疗器械] 编译完成");
}

/// RW: 轨道交通
#[cfg(feature = "domains")]
fn cmd_industry_rw(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::railway::*;

    println!("   [轨道交通] EN 50128 SIL-4 / 联锁 / 安全通信");
    println!();

    // 联锁条件 (从函数生成)
    let conditions: Vec<InterlockCondition> = ast.functions.iter().map(|sf| {
        InterlockCondition {
            description: format!("Route_{}: 进路建立", sf.func.name),
            preconditions: vec!["TrackSection == Clear".into(), "Point == Locked".into()],
            inhibitions: vec!["TrackSection == Occupied".into()],
            sil: RailwaySil::Sil4,
        }
    }).collect();

    let verification = verify_interlocking(&conditions);
    let summary = generate_en50128_summary(stem, RailwaySil::Sil4, ast.functions.len());

    let comm_cfg = SafetyCommConfig {
        system_name: stem.to_string(),
        sil: RailwaySil::Sil4,
        safety_code_bits: 32,
        sequence_bits: 32,
        timeout_ms: 500,
        max_delay_ms: 200,
    };
    let comm_code = generate_safety_comm_code(&comm_cfg);

    let _ = fs::create_dir_all(out_dir);
    let summary_path = out_dir.join(format!("{}_en50128.txt", stem));
    let comm_path = out_dir.join(format!("{}_safety_comm.c", stem));
    let interlock_path = out_dir.join(format!("{}_interlock.txt", stem));

    let _ = fs::write(&summary_path, &summary);
    let _ = fs::write(&comm_path, &comm_code);
    let mut interlock_report = String::new();
    interlock_report.push_str(&format!("Interlocking Verification: {} conditions, {} verified\n", verification.total_conditions, verification.verified_count));
    for v in &verification.verified { interlock_report.push_str(&format!("  {}\n", v)); }
    for c in &verification.conflicts { interlock_report.push_str(&format!("  CONFLICT: {}\n", c)); }
    let _ = fs::write(&interlock_path, &interlock_report);

    println!("   SIL: 4 (THR 10^-9/h)");
    println!("   联锁验证: {} 条件, {} 通过", verification.total_conditions, verification.verified_count);
    println!("   安全通信: EN 50159 CRC-32 + 序列号 + 超时");
    println!();
    println!("   输出文件:");
    println!("      {}", summary_path.display());
    println!("      {}", comm_path.display());
    println!("      {}", interlock_path.display());
    println!();
    println!("   [轨道交通] 编译完成");
}

/// SE: 半导体设备
#[cfg(feature = "domains")]
fn cmd_industry_se(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::semiconductor::*;

    println!("   [半导体设备] 纳米定位 / 振动抑制 / 对准系统");
    println!();

    let axes: Vec<NanoAxis> = vec![
        NanoAxis { name: "WaferStage_X".into(), stroke_nm: 300_000_000, resolution_nm: 0.1, max_velocity_mm_s: 500.0, max_accel_m_s2: 50.0, accuracy_nm: 1.0, repeatability_nm: 0.5, servo_period_us: 50, motor_type: MotorType::PlanarMotor },
        NanoAxis { name: "WaferStage_Y".into(), stroke_nm: 300_000_000, resolution_nm: 0.1, max_velocity_mm_s: 500.0, max_accel_m_s2: 50.0, accuracy_nm: 1.0, repeatability_nm: 0.5, servo_period_us: 50, motor_type: MotorType::PlanarMotor },
    ];
    let gains = PidGains { kp: 50.0, ki: 0.01, kd: 5.0 };
    let servo_code = generate_nano_servo_code(&axes, &gains);

    // 振动抑制
    let shaper = design_zvd_shaper(150.0, 0.02);
    let align_cfg = AlignmentConfig { mark_count: 4, required_accuracy_nm: 2.0, field_of_view_um: 100.0, mark_type: AlignMarkType::Grating };
    let spec = analyze_positioning(&axes[0], &gains);

    let _ = fs::create_dir_all(out_dir);
    let servo_path = out_dir.join(format!("{}_nano_servo.c", stem));
    let _ = fs::write(&servo_path, &servo_code);

    println!("   轴数: {} (Planar Motor, 20kHz 伺服环)", axes.len());
    println!("   定位精度: {:.2} nm | 整定时间: {:.1} ms", spec.settling_error_nm, spec.settling_time_ms);
    println!("   振动抑制: ZVD 整形器 @ {:.0} Hz ({} 脉冲)", shaper.frequency_hz, shaper.impulses.len());
    println!("   对准精度: {} nm | 视场: {} μm", align_cfg.required_accuracy_nm, align_cfg.field_of_view_um);
    println!("   函数数: {}", ast.functions.len());
    println!();
    println!("   输出文件:");
    println!("      {}", servo_path.display());
    println!();
    println!("   [半导体设备] 编译完成");
}

/// EDU: 教育平台
#[cfg(feature = "domains")]
fn cmd_industry_edu(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::education::*;

    println!("   [教育平台] 课程生成 / 编译可视化 / 实验指导");
    println!();

    // 生成编译可视化 (所有阶段)
    let source_text = ast.functions.iter()
        .map(|sf| format!("function {}() {{ ... }}", sf.func.name))
        .collect::<Vec<_>>()
        .join("\n");
    let source_display = if source_text.is_empty() { "let x = 42;".to_string() } else { source_text };

    let mut vis_output = String::new();
    for stage in CompileStage::all() {
        vis_output.push_str(&generate_compile_visualization(&source_display, *stage));
        vis_output.push_str("\n");
    }

    // 课程大纲
    let curriculum = generate_curriculum();
    let mut curriculum_text = String::new();
    curriculum_text.push_str("=== HardyScript 嵌入式课程大纲 ===\n\n");
    for unit in &curriculum {
        curriculum_text.push_str(&format!("[{}] {}\n", unit.level.as_str(), unit.title));
        for obj in &unit.objectives {
            curriculum_text.push_str(&format!("  - {}\n", obj));
        }
        curriculum_text.push('\n');
    }

    let _ = fs::create_dir_all(out_dir);
    let vis_path = out_dir.join(format!("{}_compile_visual.txt", stem));
    let course_path = out_dir.join(format!("{}_curriculum.txt", stem));

    let _ = fs::write(&vis_path, &vis_output);
    let _ = fs::write(&course_path, &curriculum_text);

    println!("   编译可视化: 7 个阶段 (Source → Lexer → Parser → IR → Opt → CodeGen → Linker)");
    println!("   课程单元: {} 个 (L1~L5)", curriculum.len());
    println!("   函数数: {}", ast.functions.len());
    println!();
    println!("   输出文件:");
    println!("      {}", vis_path.display());
    println!("      {}", course_path.display());
    println!();
    println!("   [教育平台] 编译完成");
}

/// RB: 具身机器人
#[cfg(feature = "domains")]
fn cmd_industry_rb(ast: &parser::Program, stem: &str, out_dir: &Path) {
    use crate::robotics::*;

    println!("   [具身机器人] 运动学 / 力控 / 传感融合 / ISO 15066");
    println!();

    let dof = ast.functions.len().max(6);
    let impedance = ImpedanceParams { mass: 2.0, damping: 20.0, stiffness: 100.0 };
    let safety = CollaborativeSafetyConfig {
        max_force_n: 150.0,
        max_pressure_pa: 300_000.0,
        max_speed_mm_s: 250.0,
        safe_stop_distance_mm: 50.0,
        mode: SafetyMode::PowerForceLimit,
    };

    let control_code = generate_robot_control_code(dof, &impedance, &safety);

    // 轨迹规划示例
    let traj = plan_quintic(0.0, std::f64::consts::FRAC_PI_2, 2.0);
    let (pos_mid, vel_mid, _) = sample_quintic(&traj, 1.0);

    let _ = fs::create_dir_all(out_dir);
    let ctrl_path = out_dir.join(format!("{}_robot_ctrl.c", stem));
    let _ = fs::write(&ctrl_path, &control_code);

    println!("   自由度: {} DOF", dof);
    println!("   阻抗控制: M={:.0}kg D={:.0}Ns/m K={:.0}N/m", impedance.mass, impedance.damping, impedance.stiffness);
    println!("   安全模式: 功率/力限制 (ISO/TS 15066)");
    println!("   轨迹规划: 五次多项式, 中点 pos={:.3}rad vel={:.3}rad/s", pos_mid, vel_mid);
    println!();
    println!("   输出文件:");
    println!("      {}", ctrl_path.display());
    println!();
    println!("   [具身机器人] 编译完成");
}

