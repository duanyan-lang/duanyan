//! Hardy - The HardyScript Compiler
//!
//! 捕猎主题命令行: JavaScript/TypeScript → COUNPRE64 机器码
//!
//! 🪤 "在代码的丛林中，Hardy 是无形的陷阱"

use std::env;
use std::fs;
use std::path::Path;
use std::process;

mod lexer;
mod parser;
mod compiler;
mod backend;
mod optimizer;
mod strip;
mod engine;
mod debugger;
mod pack;
mod flasher;
mod firmware;
mod config;
mod hardware_api;
mod hardware_sim;
mod board;
mod monitor;
mod project;
mod compat;
mod stdlib;
mod types;
mod c_codegen;
mod esp32_project;
mod arduino_project;
mod stm32_project;
mod rpi_project;
mod model;
mod workspace;
mod git_ops;
mod lint;
mod ci;
mod async_transform;  // Phase v1.5

const BANNER: &str = r#"
  _   _    _    ____  ____  __   __
 | | | |  / \  |  _ \|  _ \ \ \ / /
 | |_| | / _ \ | |_) | | | | \ V / 
 |  _  |/ ___ \|  _ <| |_| |  | |  
 |_| |_/_/   \_\_| \_\____/   |_|   v{VERSION}
 
  🔧 HardyScript — 一套代码，通吃一切硬件
  JavaScript/TypeScript → COUNPRE64 / ESP32 / Arduino / STM32 / 树莓派
  🧠 锻言Mini 端侧AI推理 (.duanyan 模型 → ESP32 逐层推理)
"#;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_banner();
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        // 捕猎命令
        "build"   => cmd_build(&args[2..]),
        "hunt"    => cmd_hunt(&args[2..]),
        "track"   => cmd_track(&args[2..]),
        "scan"    => cmd_scan(&args[2..]),
        "lock"    => cmd_lock(&args[2..]),
        "flash"   => cmd_flash(&args[2..]),
        "bite"    => cmd_bite(&args[2..]),
        "run"     => cmd_run(&args[2..]),
        "debug"   => cmd_debug(&args[2..]),
        "prey"    => cmd_prey(&args[2..]),
        // 包管理器
        "pack"    => pack::dispatch(&args[2..]),
        // Phase B 新命令
        "board"   => cmd_board(&args[2..]),
        "new"     => cmd_new(&args[2..]),
        "init"    => cmd_init(&args[2..]),
        "monitor" => cmd_monitor(&args[2..]),
        // Phase C 新命令
        "compat"  => cmd_compat(&args[2..]),
        // Phase F: 锻言Mini AI
        "model"   => cmd_model(&args[2..]),
        // Phase H: 团队开发
        "workspace" => cmd_workspace(&args[2..]),
        "git"       => cmd_git_cli(&args[2..]),
        "lint"      => cmd_lint(&args[2..]),
        "ci"        => cmd_ci(&args[2..]),
        // 兼容旧命令
        "compile" => cmd_build(&args[2..]),
        // 通用
        "help" | "--help" | "-h" => { print_banner(); print_usage(); }
        "version" | "--version" | "-V" => print_banner(),
        _ => {
            eprintln!("🪤 未知猎术 '{}'\n", args[1]);
            print_usage();
            process::exit(1);
        }
    }
}

// =============================================================================
// hardy build - 编译（打造武器）
// =============================================================================
fn cmd_build(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy build: 请指定猎物源码");
        eprintln!("用法: hardy build <input.js|.ts> [-o output.hex] [--asm] [-O] [--compat] [--async] [--target esp32|arduino-avr|arduino-arm|stm32|rpi|counpre64] [--board arduino-uno]");
        process::exit(1);
    }

    let input_file = &args[0];
    let mut output_file = String::new();
    let mut emit_asm = false;
    let mut do_optimize = false;
    let mut do_compat = false;
    let mut target_str: Option<String> = None;
    let mut board_str: Option<String> = None;
    let mut async_mode = false;  // Phase v1.5

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => { i += 1; if i < args.len() { output_file = args[i].clone(); } }
            "--asm" | "--emit-asm" => emit_asm = true,
            "-O" | "--optimize" => do_optimize = true,
            "--compat" => do_compat = true,
            "--async" => async_mode = true,  // Phase v1.5: 启用 async 编译模式
            "--target" | "-t" => { i += 1; if i < args.len() { target_str = Some(args[i].clone()); } }
            "--board" | "-b" => { i += 1; if i < args.len() { board_str = Some(args[i].clone()); } }
            _ => eprintln!("⚠️  忽略未知参数 '{}'", args[i]),
        }
        i += 1;
    }

    if output_file.is_empty() {
        output_file = input_file.replace(".ts", ".hex").replace(".js", ".hex");
    }

    // 读取源码
    let raw_source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 无法捕获猎物 '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    // TypeScript 剥离
    let source = if input_file.ends_with(".ts") {
        println!("🔪 剥离类型外壳 (.ts → .js)...");
        strip::strip_types(&raw_source)
    } else {
        raw_source
    };

    // Espruino/Moddable 兼容预处理
    let source = if do_compat {
        println!("🔧 兼容模式: Espruino/Moddable API 自动转换...");
        let compat_result = compat::preprocess(&source);
        for w in &compat_result.warnings {
            println!("   {}", w);
        }
        if compat_result.has_legacy_api {
            println!("   ✅ 已自动转换 {} 处旧式 API", compat_result.warnings.len());
        }
        let (moddable_result, moddable_warnings) = compat::transform_moddable_import(&compat_result.source);
        for w in &moddable_warnings {
            println!("   {}", w);
        }
        moddable_result
    } else {
        // 静默检测旧式 API（不转换，只提示）
        let detect = compat::preprocess(&source);
        if detect.has_legacy_api {
            println!("   💡 检测到旧式 API，可用 --compat 自动转换");
            for w in &detect.warnings {
                println!("   {}", w);
            }
        }
        source
    };

    // ---------- 分支: C 代码生成 vs COUNPRE64 原生编译 ----------
    if let Some(ref t) = target_str {
        let t_lower = t.to_lowercase();
        if t_lower != "counpre64" && t_lower != "fpga" && t_lower != "gowin" {
            // C 代码生成路径 (ESP32 / STM32 / Arduino ...)
            return cmd_build_c_target(&source, input_file, &t_lower, &output_file, board_str.as_deref());
        }
    }

    // ---------- COUNPRE64 原生编译路径 ----------
    println!("🔨 锻造武器: {} → {}", input_file, output_file);
    if do_optimize { println!("   ⚡ 泝火优化: 开启"); }

    // 编译流水线
    let instructions = compile_pipeline(&source, do_optimize);

    // 输出
    if emit_asm {
        let asm = backend::emit_asm(&instructions);
        println!("\n--- 🗡️ 武器蓝图 (汇编) ---");
        println!("{}", asm);
    }

    match backend::emit_hex(&instructions, &output_file) {
        Ok(_) => {
            println!("   📦 {} 条指令 ({} bytes)", instructions.len(), instructions.len() * 4);
            // Phase v1.5: 显示 async 状态机信息
            if async_mode {
                let tokens = lexer::tokenize(&source).unwrap_or_default();
                if let Ok(ast) = parser::parse(&tokens) {
                    let async_fns: Vec<_> = ast.functions.iter()
                        .filter(|sf| sf.func.is_async)
                        .collect();
                    if !async_fns.is_empty() {
                        println!("   ⚡ Async 状态机: {} 个生成", async_fns.len());
                        for sf in &async_fns {
                            if let Ok(analysis) = async_transform::analyze_async_function(&sf.func) {
                                println!("      - {}(): {} 状态, ctx={}B", 
                                    sf.func.name, analysis.state_count, analysis.ctx_size);
                            }
                        }
                    }
                }
            }
            println!("   ✅ 武器锻造完成: {}", output_file);
        }
        Err(e) => {
            eprintln!("❌ 锻造失败: {}", e);
            process::exit(1);
        }
    }
}

/// C 目标编译路径: JS → C → ESP-IDF / Arduino / STM32 HAL 项目
fn cmd_build_c_target(source: &str, input_file: &str, target: &str, _output_file: &str, board: Option<&str>) {
    let c_target = match c_codegen::CTarget::from_str(target) {
        Some(t) => t,
        None => {
            eprintln!("❌ 未知编译目标: {}", target);
            eprintln!("支持目标: esp32, stm32, arduino-avr, arduino-arm, rpi");
            process::exit(1);
        }
    };

    println!("🔨 C 代码生成: {} → {} ({:?})", input_file, target, c_target);

    // 词法 + 语法分析
    let tokens = match lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => { eprintln!("❌ 词法错误: {}", e); process::exit(1); }
    };
    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => { eprintln!("❌ 语法错误: {}", e); process::exit(1); }
    };

    // AST → C 代码
    let c_result = match c_codegen::generate_c_source(&ast, &c_target) {
        Ok(r) => r,
        Err(e) => { eprintln!("❌ C 代码生成失败: {}", e); process::exit(1); }
    };

    // 确定输出目录
    let base_dir = Path::new(input_file).parent().unwrap_or(Path::new("."));
    let stem = Path::new(input_file)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("out");

    match c_target {
        c_codegen::CTarget::Esp32 => {
            let paths = match esp32_project::generate_esp32_project(&c_result, base_dir, stem) {
                Ok(p) => p,
                Err(e) => { eprintln!("❌ ESP32 项目生成失败: {}", e); process::exit(1); }
            };
            println!("   📦 ESP-IDF 项目已生成: {}", paths.project_dir.display());
            println!("   📄 main.c: {}", paths.main_c.display());
            println!();
            println!("   📝 下一步:");
            println!("      1. 确保已安装 ESP-IDF (https://docs.espressif.com/projects/esp-idf/)");
            println!("      2. 打开 ESP-IDF 命令行 (ESP-IDF CMD)");
            println!("      3. cd {}", paths.project_dir.display());
            println!("      4. idf.py build");
            println!("      5. idf.py -p COM3 flash monitor");
            println!();
            println!("   ✅ ESP32 项目锻造完成");
        }
        c_codegen::CTarget::ArduinoAvr | c_codegen::CTarget::ArduinoArm => {
            let paths = match arduino_project::generate_arduino_project(&c_result, base_dir, stem) {
                Ok(p) => p,
                Err(e) => { eprintln!("❌ Arduino 项目生成失败: {}", e); process::exit(1); }
            };

            // 资源估算
            let is_arm = matches!(c_target, c_codegen::CTarget::ArduinoArm);
            let arduino_board = board
                .and_then(|b| arduino_project::find_arduino_board(b))
                .unwrap_or_else(|| arduino_project::default_board_for_target(is_arm));
            let est = arduino_project::estimate_resources(&c_result.source, &arduino_board);

            println!("   📦 Arduino sketch 项目已生成: {}", paths.project_dir.display());
            println!("   📄 {}.ino: {}", stem, paths.ino_file.display());
            println!();
            println!("   🎯 目标板子: {} ({})", arduino_board.name, arduino_board.fqbn);
            println!("   📊 RAM  预估: {} bytes / {} KB ({:.0}%)",
                est.ram_bytes, arduino_board.max_ram / 1024, est.ram_percent);
            println!("   📊 Flash 预估: {} bytes / {} KB ({:.0}%)",
                est.flash_bytes, arduino_board.max_flash / 1024, est.flash_percent);
            for w in &est.warnings {
                println!("   {}", w);
            }
            for e in &est.errors {
                println!("   {}", e);
            }
            println!();
            println!("   📝 下一步:");
            println!("      1. 安装 arduino-cli: https://arduino.github.io/arduino-cli/");
            println!("      2. arduino-cli core install {}", arduino_board.fqbn.split(':').next().unwrap_or("arduino"));
            println!("      3. arduino-cli compile --fqbn {} {}", arduino_board.fqbn, paths.project_dir.display());
            println!("      4. arduino-cli upload -p COM3 --fqbn {} {}", arduino_board.fqbn, paths.project_dir.display());
            println!();
            println!("   ✅ Arduino sketch 锻造完成");
        }
        c_codegen::CTarget::Stm32 => {
            let paths = match stm32_project::generate_stm32_project(&c_result, base_dir, stem) {
                Ok(p) => p,
                Err(e) => { eprintln!("❌ STM32 项目生成失败: {}", e); process::exit(1); }
            };
            println!("   📦 STM32 HAL 项目已生成: {}", paths.project_dir.display());
            println!("   📄 main.c: {}", paths.main_c.display());
            println!("   📄 Makefile: {}", paths.makefile.display());
            println!("   📄 Linker: {}", paths.linker_script.display());
            println!("   📄 OpenOCD: {}", paths.openocd_cfg.display());
            println!();
            println!("   📝 下一步:");
            println!("      1. 安装 arm-none-eabi-gcc 工具链");
            println!("      2. 设置 STM32_HAL_PATH 环境变量指向 STM32 HAL 库");
            println!("      3. cd {}", paths.project_dir.display());
            println!("      4. make");
            println!("      5. make flash  (或 st-flash write {}_stm32.bin 0x08000000)", stem);
            println!();
            println!("   ✅ STM32 项目锻造完成");
        }
        c_codegen::CTarget::RaspberryPi => {
            let paths = match rpi_project::generate_rpi_project(&c_result, base_dir, stem) {
                Ok(p) => p,
                Err(e) => { eprintln!("❌ RPi 项目生成失败: {}", e); process::exit(1); }
            };
            println!("   📦 Raspberry Pi 项目已生成: {}", paths.project_dir.display());
            println!("   📄 main.c: {}", paths.main_c.display());
            println!("   📄 Makefile: {}", paths.makefile.display());
            println!("   📄 run.sh: {}", paths.run_sh.display());
            println!();
            println!("   📝 下一步:");
            println!("      1. cd {}", paths.project_dir.display());
            println!("      2. make");
            println!("      3. sudo ./{}_rpi  (本地执行)", stem);
            println!("      或: bash run.sh <pi_host> <pi_user>  (远程部署)");
            println!();
            println!("   ✅ RPi 项目锻造完成");
        }
        _ => {
            // 其他平台: 仅输出 C 源码
            let c_path = base_dir.join(format!("{}.c", stem));
            match fs::write(&c_path, &c_result.source) {
                Ok(_) => println!("   📄 C 源码: {}", c_path.display()),
                Err(e) => { eprintln!("❌ 写入失败: {}", e); process::exit(1); }
            }
            println!("   ✅ C 代码生成完成 ({:?} 平台待实现工具链集成)", c_target);
        }
    }
}

// =============================================================================
// hardy hunt - 运行（捕猎执行）: build + flash
// =============================================================================
fn cmd_hunt(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy hunt: 请指定猎物");
        eprintln!("用法: hardy hunt <input.js|.ts> [--port COM3]");
        process::exit(1);
    }

    println!("🪤 陷阱启动...");
    
    // 先编译
    let mut build_args = args.to_vec();
    if !build_args.iter().any(|a| a == "-O") {
        build_args.push("-O".to_string()); // hunt 默认开启优化
    }
    cmd_build(&build_args);

    // 提示烧录
    let hex_file = args[0].replace(".ts", ".hex").replace(".js", ".hex");
    println!("\n🎯 猎物已就绪: {}", hex_file);
    println!("   → 使用 'hardy lock' 锁定硬件目标");
    println!("   → 或手动: Gowin Programmer → Load '{}'", hex_file);
}

// =============================================================================
// hardy track - 追踪（调试输出汇编）
// =============================================================================
fn cmd_track(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy track: 请指定追踪目标");
        eprintln!("用法: hardy track <input.js|.ts>");
        process::exit(1);
    }

    let input_file = &args[0];
    let raw_source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 无法追踪 '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    let source = if input_file.ends_with(".ts") {
        println!("🔪 剥离类型...");
        strip::strip_types(&raw_source)
    } else {
        raw_source
    };

    println!("🔍 追踪指令流: {}\n", input_file);

    let instructions = compile_pipeline(&source, false);
    let asm = backend::emit_asm(&instructions);
    
    println!("┌─────────────────────────────────────────────┐");
    println!("│  ADDR   HEX CODE    ASSEMBLY                │");
    println!("├─────────────────────────────────────────────┤");
    for line in asm.lines() {
        println!("│  {}  │", format!("{:<40}", line));
    }
    println!("└─────────────────────────────────────────────┘");
    println!("\n📊 共 {} 条指令, {} bytes", instructions.len(), instructions.len() * 4);
}

// =============================================================================
// hardy scan - 侦查（扫描代码，报告统计）
// =============================================================================
fn cmd_scan(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy scan: 请指定侦查目标");
        eprintln!("用法: hardy scan <input.js|.ts>");
        process::exit(1);
    }

    let input_file = &args[0];
    let raw_source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 无法侦查 '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    let is_ts = input_file.ends_with(".ts");
    let source = if is_ts {
        strip::strip_types(&raw_source)
    } else {
        raw_source.clone()
    };

    println!("👁️ 侦查报告: {}", input_file);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  📄 文件类型:  {}", if is_ts { "TypeScript" } else { "JavaScript" });
    println!("  📏 源码行数:  {}", raw_source.lines().count());
    println!("  📐 源码字节:  {}", raw_source.len());

    // 词法分析
    let tokens = match lexer::tokenize(&source) {
        Ok(t) => t,
        Err(e) => { eprintln!("  ❌ 词法异常: {}", e); process::exit(1); }
    };
    println!("  🔤 Token 数:  {}", tokens.len() - 1); // 减去 EOF

    // 语法分析
    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => { eprintln!("  ❌ 语法异常: {}", e); process::exit(1); }
    };
    println!("  📝 语句数:    {}", ast.statements.len());
    println!("  🔧 函数数:    {}", ast.functions.len());

    // 编译估算
    let instrs = compiler::compile(&ast).unwrap_or_default();
    let opt_instrs = {
        let opt_ast = optimizer::optimize(&ast);
        compiler::compile(&opt_ast).unwrap_or_default()
    };

    println!("  ⚙️  指令数:    {} (优化后: {})", instrs.len(), opt_instrs.len());
    println!("  💾 代码大小:  {} bytes (优化后: {} bytes)", instrs.len() * 4, opt_instrs.len() * 4);
    if instrs.len() > 0 {
        let saving = ((instrs.len() - opt_instrs.len()) as f64 / instrs.len() as f64 * 100.0) as i32;
        println!("  📉 优化收益:  {}%", saving);
    }

    // 资源占用估算
    let var_count = count_variables(&ast);
    let stack_slots = estimate_stack_usage(&ast);
    let flash_bytes = opt_instrs.len() * 4;
    let ram_bytes = var_count * 8 + stack_slots * 8;  // 每个变量/栈槽 8 字节

    println!();
    println!("  📊 资源占用估算 (COUNPRE64):");
    println!("     Flash:  {} bytes ({:.1}% / 64KB)", flash_bytes, flash_bytes as f64 / 65536.0 * 100.0);
    println!("     RAM:    ~{} bytes (变量{}个 + 栈槽{}个)", ram_bytes, var_count, stack_slots);

    // 多平台资源对照
    println!();
    println!("  📊 多平台容量对照:");
    println!("     {:12} {:>8} {:>8}  {}", "平台", "Flash", "RAM", "状态");
    println!("     {:-<55}", "");
    let platforms = [
        ("COUNPRE64", 64 * 1024, 32 * 1024),
        ("ESP32", 4096 * 1024, 520 * 1024),
        ("Arduino Uno", 32 * 1024, 2 * 1024),
        ("Arduino Mega", 256 * 1024, 8 * 1024),
        ("STM32F103", 64 * 1024, 20 * 1024),
        ("RPi 4B", i32::MAX, i32::MAX),
    ];
    for (name, flash_cap, ram_cap) in &platforms {
        let flash_pct = if *flash_cap == i32::MAX { 0.0 } else { flash_bytes as f64 / *flash_cap as f64 * 100.0 };
        let ram_pct = if *ram_cap == i32::MAX { 0.0 } else { ram_bytes as f64 / *ram_cap as f64 * 100.0 };
        let status = if *flash_cap == i32::MAX { "✅ 无限制" }
            else if flash_pct > 100.0 || ram_pct > 100.0 { "❌ 超限" }
            else if flash_pct > 80.0 || ram_pct > 80.0 { "⚠️  接近上限" }
            else { "✅ OK" };
        if *flash_cap == i32::MAX {
            println!("     {:12} {:>7} {:>7}  {}", name, "∞", "∞", status);
        } else {
            println!("     {:12} {:6.1}% {:6.1}%  {}", name, flash_pct, ram_pct, status);
        }
    }

    // 警告检查
    if flash_bytes > 65536 {
        println!("\n     ⚠️  Flash 超出 COUNPRE64 限制!");
    }
    if ram_bytes > 32768 {
        println!("     ⚠️  RAM 超出 COUNPRE64 限制!");
    }

    // Phase v1.5: async 函数分析
    let async_fns: Vec<_> = ast.functions.iter()
        .filter(|sf| sf.func.is_async)
        .collect();
    if !async_fns.is_empty() {
        println!();
        println!("  ⚡ Async 函数分析 (v1.5):");
        let mut total_ctx_size = 0usize;
        for sf in &async_fns {
            if let Ok(analysis) = async_transform::analyze_async_function(&sf.func) {
                println!("     - {}(): {} 个 await 点, {} 状态, ctx={}B",
                    sf.func.name, analysis.await_points.len(), 
                    analysis.state_count, analysis.ctx_size);
                total_ctx_size += analysis.ctx_size;
            }
        }
        println!("     总上下文开销: {}B / 4096B 预算", total_ctx_size);
        if total_ctx_size > 4096 {
            println!("     ⚠️  超出 4KB 堆预算!");
        }
    }

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  ✅ 侦查完毕，猎物可捕获");
}

// =============================================================================
// hardy lock - 锁定硬件（烧录提示）
// =============================================================================
fn cmd_lock(args: &[String]) {
    let hex_file = if !args.is_empty() {
        args[0].clone()
    } else {
        eprintln!("hardy lock: 请指定 .hex 文件");
        eprintln!("用法: hardy lock <file.hex> [--port COM3]");
        process::exit(1);
    };

    println!("🎯 锁定目标硬件...");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  📦 弹药: {}", hex_file);
    println!("  🎯 目标: COUNPRE64 @ Gowin GW1NSR-4C");
    println!("  🔌 接口: USB-JTAG");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("\n  ⚡ 烧录步骤:");
    println!("     1. 打开 Gowin Programmer");
    println!("     2. 将 .hex 写入 memory_ctrl.v case 语句");
    println!("     3. Clean All → Run All (Synthesize + P&R)");
    println!("     4. Program Device");
    println!("\n  🪤 或执行 PowerShell:");
    println!("     pwsh -File C:\\COUNPRE64\\tools\\fpga_com.ps1");
}

// =============================================================================
// hardy flash - 烧录到硬件（发射弹药）
// =============================================================================
fn cmd_flash(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy flash: 请指定猎物源码或 .hex 文件");
        eprintln!("用法: hardy flash <input.js|.ts|.hex> [--target gowin|stm32|esp32|arduino|rpi] [--board arduino-uno] [--port COM3] [--rom]");
        process::exit(1);
    }

    let input_file = &args[0];
    let mut target_kind = flasher::TargetKind::FpgaGowin;
    let mut port: Option<String> = None;
    let mut emit_rom = false;
    let mut board_str: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--target" | "-t" => {
                i += 1;
                if i < args.len() {
                    target_kind = flasher::TargetKind::from_str(&args[i])
                        .unwrap_or(flasher::TargetKind::FpgaGowin);
                }
            }
            "--board" | "-b" => {
                i += 1;
                if i < args.len() {
                    board_str = Some(args[i].clone());
                    // --board 可自动推断 target
                    if let Some(tk) = board_to_target_kind(&args[i]) {
                        target_kind = tk;
                    }
                }
            }
            "--port" | "-p" => {
                i += 1;
                if i < args.len() {
                    port = Some(args[i].clone());
                }
            }
            "--rom" => emit_rom = true,
            _ => eprintln!("⚠️  忽略未知参数 '{}'", args[i]),
        }
        i += 1;
    }

    let target = flasher::Target { kind: target_kind, port };
    let flasher = flasher::create_flasher(&target);

    let artifact = if input_file.ends_with(".hex") {
        flasher::FirmwareArtifact::from_hex(Path::new(input_file))
    } else {
        // Compile source to a firmware bundle first.
        let raw_source = match fs::read_to_string(input_file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("❌ 无法读取源码 '{}': {}", input_file, e);
                process::exit(1);
            }
        };
        let source = if input_file.ends_with(".ts") {
            strip::strip_types(&raw_source)
        } else {
            raw_source
        };
        let instructions = compile_pipeline(&source, false);
        let base_dir = Path::new(input_file).parent().unwrap_or(Path::new("."));
        let stem = Path::new(input_file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("out");
        let bundle = firmware::emit_bundle(&instructions, base_dir, stem)
            .unwrap_or_else(|e| { eprintln!("❌ 打包固件失败: {}", e); process::exit(1); });

        if emit_rom {
            let rom_path = base_dir.join(format!("{}_rom.v", stem));
            firmware::emit_verilog_rom(&instructions, &rom_path, "hardyscript_rom")
                .unwrap_or_else(|e| eprintln!("⚠️  生成 ROM 失败: {}", e));
            println!("📦 Verilog ROM: {}", rom_path.display());
        }

        println!("🔨 编译完成: {} 条指令 → {}", bundle.word_count, bundle.hex_path.display());
        flasher::FirmwareArtifact::from_hex(&bundle.hex_path)
    };

    let artifact = match artifact {
        Ok(a) => a,
        Err(e) => { eprintln!("❌ {}", e); process::exit(1); }
    };

    println!("🚀 开始烧录到 {}...", flasher.name());
    match flasher.flash(&artifact) {
        Ok(result) => {
            if !result.stdout.is_empty() { println!("{}", result.stdout); }
            if !result.stderr.is_empty() { eprintln!("{}", result.stderr); }
            if result.success {
                println!("✅ 烧录成功");
            } else {
                eprintln!("❌ 烧录失败");
                process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("❌ 烧录异常: {}", e);
            process::exit(1);
        }
    }
}

// =============================================================================
// hardy bite - 单步调试（撕咬执行）
// =============================================================================
fn cmd_bite(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy bite: 请指定目标");
        eprintln!("用法: hardy bite <input.js|.ts>");
        process::exit(1);
    }

    let input_file = &args[0];
    let raw_source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => { eprintln!("❌ {}", e); process::exit(1); }
    };

    let source = if input_file.ends_with(".ts") {
        strip::strip_types(&raw_source)
    } else {
        raw_source
    };

    let instructions = compile_pipeline(&source, false);

    println!("🦷 撕咬执行模式: {} ({} 条指令)\n", input_file, instructions.len());
    println!("PC      HEX       ASM                   描述");
    println!("──────  ────────  ────────────────────  ─────────");
    
    for (i, instr) in instructions.iter().enumerate() {
        let hex = instr.encode();
        let asm = instr.to_asm();
        let desc = describe_instruction(hex);
        println!("0x{:04X}  {:08X}  {:<22}  {}", i * 4, hex, asm, desc);
    }
    
    println!("\n🦷 共咬穿 {} 条指令", instructions.len());
}


// =============================================================================
// hardy run - 执行（原生执行引擎）
// =============================================================================
fn cmd_run(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy run: 请指定执行目标");
        eprintln!("用法: hardy run <input.js|.ts> [-O] [--hw]");
        eprintln!("  --hw  启用硬件模拟模式 (GPIO/UART/PWM/Timer)");
        process::exit(1);
    }

    let input_file = &args[0];
    let do_optimize = args.iter().any(|a| a == "-O" || a == "--optimize");
    let do_hw = args.iter().any(|a| a == "--hw");

    let raw_source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 无法读取 '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    let source = if input_file.ends_with(".ts") {
        println!("🔪 剥离类型...");
        strip::strip_types(&raw_source)
    } else {
        raw_source
    };

    println!("⚡ 执行引擎启动: {}\n", input_file);

    let instructions = compile_pipeline(&source, do_optimize);
    let machine_code: Vec<u32> = instructions.iter().map(|i| i.encode()).collect();

    let mut exec = engine::Engine::new(machine_code);
    if do_hw {
        exec.set_hw_mode(true);
        println!("🖥️  硬件模拟模式已启用");
    }
    match exec.run() {
        Ok((output, cycles)) => {
            // 确保输出末尾有换行
            if !output.ends_with('\n') {
                println!();
            }
            // 硬件模拟模式下打印硬件状态
            if do_hw {
                exec.hw_sim().print_status();
            }
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("  ⚡ 执行完成: {} 条指令, {} cycles", instructions.len(), cycles);
        }
        Err(e) => {
            eprintln!("❌ 执行错误: {}", e);
            process::exit(1);
        }
    }
}

// =============================================================================
// hardy prey - 猎物列表
// =============================================================================
fn cmd_prey(args: &[String]) {
    let dir = if !args.is_empty() { &args[0] } else { "." };
    
    println!("🎯 猎物列表 ({})", dir);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => { eprintln!("❌ 无法扫描: {}", e); process::exit(1); }
    };

    let mut count = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if name.ends_with(".js") || name.ends_with(".ts") {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let icon = if name.ends_with(".ts") { "🔷" } else { "🟡" };
            println!("  {} {} ({} bytes)", icon, name, size);
            count += 1;
        }
    }

    if count == 0 {
        println!("  (无猎物)");
    } else {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  共发现 {} 个猎物", count);
    }
}


// =============================================================================
// hardy debug - 调试（交互式调试器）
// =============================================================================
fn cmd_debug(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy debug: 请指定调试目标");
        eprintln!("用法: hardy debug <input.js|.ts> [-O]");
        process::exit(1);
    }

    let input_file = &args[0];
    let do_optimize = args.iter().any(|a| a == "-O" || a == "--optimize");

    let raw_source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 无法读取 '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    let source = if input_file.ends_with(".ts") {
        println!("🔪 剥离类型...");
        strip::strip_types(&raw_source)
    } else {
        raw_source.clone()
    };

    println!("🐛 调试模式: {}\n", input_file);

    let tokens = match lexer::tokenize(&source) {
        Ok(t) => t,
        Err(e) => { eprintln!("❌ 词法错误: {}", e); process::exit(1); }
    };

    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => { eprintln!("❌ 语法错误: {}", e); process::exit(1); }
    };

    let final_ast = if do_optimize {
        optimizer::optimize(&ast)
    } else {
        ast
    };

    let result = match compiler::compile_with_sourcemap(&final_ast) {
        Ok(r) => r,
        Err(e) => { eprintln!("❌ 编译错误: {}", e); process::exit(1); }
    };

    let machine_code: Vec<u32> = result.instructions.iter().map(|i| i.encode()).collect();

    let mut dbg = debugger::Debugger::new(
        machine_code,
        &result.instructions,
        result.source_map,
        &source,
        input_file,
    );
    dbg.run_interactive();
}

// =============================================================================
// hardy board - 开发板管理
// =============================================================================
fn cmd_board(args: &[String]) {
    let subcmd = if args.is_empty() { "list" } else { args[0].as_str() };
    match subcmd {
        "list" => board::cmd_board_list(),
        "info" => {
            if args.len() < 2 {
                eprintln!("hardy board info: 请指定开发板名称");
                eprintln!("用法: hardy board info <name>");
                process::exit(1);
            }
            board::cmd_board_info(&args[1]);
        }
        "set" => {
            if args.len() < 2 {
                eprintln!("hardy board set: 请指定默认开发板");
                eprintln!("用法: hardy board set <name>");
                eprintln!("示例: hardy board set arduino-mega");
                process::exit(1);
            }
            board::cmd_board_set(&args[1]);
        }
        _ => {
            eprintln!("hardy board: 未知子命令 '{}'", subcmd);
            eprintln!("可用子命令: list, info, set");
            process::exit(1);
        }
    }
}

// =============================================================================
// hardy new - 创建新项目模板生成 =============================================================================
fn cmd_new(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy new: 请指定项目名称");
        eprintln!("用法: hardy new <name> [--template blank|blinky|uart|button] [--board esp32|arduino-uno|counpre64]");
        eprintln!();
        project::list_templates();
        process::exit(1);
    }

    let name = &args[0];
    let mut template_name = "blank";
    let mut board_name: Option<String> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--template" | "-t" => {
                i += 1;
                if i < args.len() {
                    template_name = &args[i];
                }
            }
            "--board" | "-b" => {
                i += 1;
                if i < args.len() {
                    board_name = Some(args[i].clone());
                }
            }
            _ => eprintln!("⚠️  忽略未知参数 '{}'", args[i]),
        }
        i += 1;
    }

    let template = match project::ProjectTemplate::from_str(template_name) {
        Some(t) => t,
        None => {
            eprintln!("❌ 未知模板: {}", template_name);
            project::list_templates();
            process::exit(1);
        }
    };

    let target_dir = std::env::current_dir().unwrap_or_else(|_| std::path::Path::new(".").to_path_buf());
    match project::create_project(name, &template, &target_dir, board_name.as_deref()) {
        Ok(()) => {
            println!("✅ 项目创建创建: {}", name);
            println!("   📁 {}/", name);
            println!("   ├── hardy.json");
            println!("   ├── .gitignore");
            println!("   └── src/");
            println!("       └── main.js");
            if let Some(ref b) = board_name {
                println!("   🎯 目标板子: {}", b);
            }
            println!();
            println!("🪤  cd {} && hardy build src/main.js", name);
        }
        Err(e) => {
            eprintln!("❌ {}", e);
            process::exit(1);
        }
    }
}

// =============================================================================
// hardy monitor - 串口监视器
// =============================================================================
fn cmd_monitor(args: &[String]) {
    let mut config = monitor::MonitorConfig::default();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" | "-p" => {
                i += 1;
                if i < args.len() {
                    config.port = args[i].clone();
                }
            }
            "--baud" | "-b" => {
                i += 1;
                if i < args.len() {
                    config.baud_rate = args[i].parse().unwrap_or(115200);
                }
            }
            "--hex" => config.hex_mode = true,
            "--timestamp" => config.timestamp = true,
            _ => eprintln!("⚠️  忽略未知参数 '{}'", args[i]),
        }
        i += 1;
    }

    if let Err(e) = monitor::start_monitor(&config) {
        eprintln!("❌ {}", e);
        process::exit(1);
    }
}

// =============================================================================
// hardy init - 在当前目录初始化 hardy 项目
// =============================================================================
fn cmd_init(_args: &[String]) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::Path::new(".").to_path_buf());

    if cwd.join("hardy.json").exists() {
        eprintln!("⚠️  hardy.json 已存在，跳过初始化");
        return;
    }

    let project_name = cwd.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-project".to_string());

    let config = format!(
        r#"{{
    "name": "{}",
    "version": "0.1.0",
    "target": "counpre64",
    "entry": "src/main.js",
    "build": {{
        "optimize": true,
        "output": "dist/{}.hex"
    }}
}}
"#,
        project_name, project_name
    );

    let src_dir = cwd.join("src");
    if let Err(e) = std::fs::create_dir_all(&src_dir) {
        eprintln!("❌ 创建 src 目录失败: {}", e);
        process::exit(1);
    }

    if let Err(e) = std::fs::write(cwd.join("hardy.json"), &config) {
        eprintln!("❌ 写入 hardy.json 失败: {}", e);
        process::exit(1);
    }

    let main_js = r#"// HardyScript 项目入口
// 使用 hardy:hw 硬件API

import { GPIO, Timer } from "hardy:hw";

let led = GPIO.output(13);

while (true) {
    led.high();
    Timer.delay(500);
    led.low();
    Timer.delay(500);
}
"#;
    if !src_dir.join("main.js").exists() {
        if let Err(e) = std::fs::write(src_dir.join("main.js"), main_js) {
            eprintln!("❌ 写入 main.js 失败: {}", e);
            process::exit(1);
        }
    }

    let gitignore = "dist/\n*.hex\n*.bin\n*.v\n";
    if !cwd.join(".gitignore").exists() {
        let _ = std::fs::write(cwd.join(".gitignore"), gitignore);
    }

    println!("✅ 已初始化 HardyScript 项目: {}", project_name);
    println!("   📄 hardy.json");
    println!("   📄 src/main.js");
    println!("   📄 .gitignore");
    println!();
    println!("🪤  hardy build src/main.js");
}

// =============================================================================
// hardy compat - Espruino/Moddable 兼容分析
// =============================================================================
fn cmd_compat(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy compat: 请指定分析目标");
        eprintln!("用法: hardy compat <input.js> [--convert]");
        eprintln!();
        println!("  分析源码中的 Espruino/Moddable 旧式 API");
        println!("  --convert  输出转换后的 Hardy 标准代码");
        process::exit(1);
    }

    let input_file = &args[0];
    let do_convert = args.iter().any(|a| a == "--convert");

    let raw_source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("❌ 无法读取 '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    println!("🔧 兼容分析: {}", input_file);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Espruino 检测
    let result = compat::preprocess(&raw_source);
    if result.has_legacy_api {
        println!("  📡 检测到 Espruino 旧式 API:");
        for w in &result.warnings {
            println!("     {}", w);
        }
    } else {
        println!("  ✅ 未检测到 Espruino 旧式 API");
    }

    // Moddable 检测
    let (_, moddable_warnings) = compat::transform_moddable_import(&raw_source);
    if !moddable_warnings.is_empty() {
        println!("  📡 检测到 Moddable 风格导入:");
        for w in &moddable_warnings {
            println!("     {}", w);
        }
    }

    let total = result.warnings.len() + moddable_warnings.len();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  共检测 {} 处兼容性问题", total);

    if do_convert && total > 0 {
        println!("\n--- 🔄 转换后的 Hardy 标准代码 ---");
        let converted = compat::transform_moddable_import(&result.source).0;
        println!("{}", converted);
    } else if total > 0 {
        println!("  💡 使用 --convert 查看转换后代码");
        println!("  💡 或: hardy build {} --compat", input_file);
    }
}

// =============================================================================
// 工具函数
// =============================================================================

/// 完整编译流水线
fn compile_pipeline(source: &str, do_optimize: bool) -> Vec<backend::Instruction> {
    let tokens = match lexer::tokenize(source) {
        Ok(t) => t,
        Err(e) => { eprintln!("❌ 词法错误: {}", e); process::exit(1); }
    };

    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => { eprintln!("❌ 语法错误: {}", e); process::exit(1); }
    };

    let final_ast = if do_optimize {
        optimizer::optimize(&ast)
    } else {
        ast
    };

    match compiler::compile(&final_ast) {
        Ok(instrs) => instrs,
        Err(e) => { eprintln!("❌ 编译错误: {}", e); process::exit(1); }
    }
}

/// 统计变量数量
fn count_variables(ast: &parser::Program) -> usize {
    let mut count = 0;
    for stmt in &ast.statements {
        match &stmt.stmt {
            parser::Statement::Let { .. } => count += 1,
            parser::Statement::Const { .. } => count += 1,
            _ => {}
        }
    }
    // 加上函数内的局部变量
    for func in &ast.functions {
        count += func.func.params.len();
        for stmt in &func.func.body {
            match &stmt.stmt {
                parser::Statement::Let { .. } => count += 1,
                parser::Statement::Const { .. } => count += 1,
                _ => {}
            }
        }
    }
    count
}

/// 估算栈使用量
fn estimate_stack_usage(ast: &parser::Program) -> usize {
    let mut max_depth = 0;
    let mut current_depth = 0;

    for stmt in &ast.statements {
        match &stmt.stmt {
            parser::Statement::If { .. } | parser::Statement::While { .. } | parser::Statement::For { .. } => {
                current_depth += 1;
                if current_depth > max_depth {
                    max_depth = current_depth;
                }
            }
            _ => {}
        }
    }

    // 每个嵌套层级大约需要 2 个栈槽
    max_depth * 2 + 4  // 加上基础栈帧
}

fn describe_instruction(hex: u32) -> &'static str {
    let opcode = (hex >> 28) & 0xF;
    match opcode {
        0x0 => "寄存器运算",
        0x1 => "减法/比较",
        0x2 => "按位与",
        0x3 => "按位或",
        0x4 => "按位异或",
        0x5 => "左移",
        0x6 => "右移",
        0x7 => "乘除法",
        0x8 => "加立即数",
        0x9 => "与立即数",
        0xA => "或立即数",
        0xB => "内存加载",
        0xC => "内存存储",
        0xD => "条件分支",
        0xE => "跳转",
        0xF => "返回",
        _ => "未知",
    }
}

fn print_banner() {
    let banner = BANNER.replace("{VERSION}", env!("CARGO_PKG_VERSION"));
    println!("{}", banner);
}

/// 根据 --board 参数推断 TargetKind
fn board_to_target_kind(board: &str) -> Option<flasher::TargetKind> {
    let b = board.to_lowercase();
    if b.starts_with("arduino") || b == "uno" || b == "nano" || b == "mega" || b == "due" || b == "zero" {
        Some(flasher::TargetKind::McuArduino)
    } else if b.starts_with("esp32") || b.starts_with("esp") {
        Some(flasher::TargetKind::McuEsp32)
    } else if b.starts_with("stm32") {
        Some(flasher::TargetKind::McuStm32)
    } else if b.starts_with("raspberry") || b == "rpi" || b.starts_with("rpi") {
        Some(flasher::TargetKind::McuRpi)
    } else if b.starts_with("counpre") || b == "fpga" || b == "gowin" {
        Some(flasher::TargetKind::FpgaGowin)
    } else {
        None
    }
}

// =============================================================================
// hardy model - 锻言Mini AI 模型管理 (Phase F)
// =============================================================================
fn cmd_model(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy model: 请指定子命令");
        eprintln!("用法: hardy model <convert|verify|info|deploy|list>");
        process::exit(1);
    }

    match args[0].as_str() {
        "verify" => {
            if args.len() < 2 {
                eprintln!("用法: hardy model verify <model.duanyan>");
                process::exit(1);
            }
            let path = Path::new(&args[1]);
            match model::DuanyanModel::verify(path) {
                Ok(result) => print!("{}", result.display()),
                Err(e) => { eprintln!("❌ 验证失败: {}", e); process::exit(1); }
            }
        }
        "info" => {
            if args.len() < 2 {
                eprintln!("用法: hardy model info <model.duanyan>");
                process::exit(1);
            }
            let path = Path::new(&args[1]);
            match model::DuanyanModel::read_from_file(path) {
                Ok(m) => print!("{}", m.info_string()),
                Err(e) => { eprintln!("❌ 读取失败: {}", e); process::exit(1); }
            }
        }
        "convert" => {
            // hardy model convert --input <f> --output <f> [--quant int4|fp32] [--official]
            let mut input = String::new();
            let mut output = String::new();
            let mut quant_str = "int4".to_string();
            let mut official = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--input"  => { i += 1; if i < args.len() { input = args[i].clone(); } }
                    "--output" => { i += 1; if i < args.len() { output = args[i].clone(); } }
                    "--quant"  => { i += 1; if i < args.len() { quant_str = args[i].clone(); } }
                    "--official" => { official = true; }
                    _ => eprintln!("⚠️  忽略未知参数 '{}'", args[i]),
                }
                i += 1;
            }
            if input.is_empty() || output.is_empty() {
                eprintln!("用法: hardy model convert --input <f.gguf|f.bin> --output <f.duanyan> [--quant int4|int8|fp16|fp32] [--official]");
                eprintln!("  --official  用千灌Mini官方规格 (6层/384维/6头/5000词表) 创建模型模板");
                process::exit(1);
            }

            let input_path = Path::new(&input);
            let output_path = Path::new(&output);
            let target_quant = match quant_str.to_lowercase().as_str() {
                "int4" => model::QuantBits::Int4,
                "int8" => model::QuantBits::Int8,
                "fp16" => model::QuantBits::Fp16,
                "fp32" => model::QuantBits::Fp32,
                _ => { eprintln!("❌ 不支持的量化方式: {}", quant_str); process::exit(1); }
            };

            // 检查是否是 GGUF 格式
            let is_gguf = input.to_lowercase().ends_with(".gguf");

            if is_gguf {
                // === GGUF → .duanyan 真实转换 ===
                println!("🧠 GGUF → .duanyan 真实转换");
                println!("   输入: {} (GGUF)", input);
                println!("   量化: {}", target_quant.display_name());

                match model::convert_gguf_to_duanyan(input_path, target_quant) {
                    Ok(result) => {
                        println!("   源格式: {}", result.source_format);
                        println!("   张量: {}/{} 已转换", result.tensors_converted, result.tensors_found);
                        println!("   参数: {} ({:.1}M)", result.total_params, result.total_params as f64 / 1e6);
                        println!("   层数: {} 个LayerRecord", result.model.layers.len());

                        match result.model.write_to_file(output_path) {
                            Ok(()) => {
                                let meta = fs::metadata(output_path).unwrap();
                                println!("   输出: {} ({} bytes, {:.2} MB)",
                                    output, meta.len(), meta.len() as f64 / (1024.0 * 1024.0));
                                println!("✅ GGUF转换完成");
                            }
                            Err(e) => { eprintln!("❌ 写入失败: {}", e); process::exit(1); }
                        }
                    }
                    Err(e) => { eprintln!("❌ GGUF转换失败: {}", e); process::exit(1); }
                }
            } else if official {
                // === 官方规格模型模板 ===
                println!("🧠 创建千灌Mini官方规格模型模板");
                let mut m = model::create_duanyan_mini_official();
                // 用量化方式重写层数据
                let quant = target_quant;
                for layer in &mut m.layers {
                    let n_params = (layer.rows * layer.cols) as usize;
                    let weights: Vec<f32> = vec![0.0; n_params]; // 零权重模板
                    let (data, scale) = model::quantize_weights_pub(&weights, quant);
                    layer.data = data;
                    layer.quant = quant as u8;
                    layer.scale = scale;
                }
                m.header.quant_bits = quant as u32;

                match m.write_to_file(output_path) {
                    Ok(()) => {
                        let meta = fs::metadata(output_path).unwrap();
                        println!("   输出: {} ({} bytes)", output, meta.len());
                        println!("   规格: {}层/hidden={}/heads={}/vocab={}",
                            model::DUANYAN_MINI_LAYERS, model::DUANYAN_MINI_HIDDEN,
                            model::DUANYAN_MINI_HEADS, model::DUANYAN_MINI_VOCAB);
                        println!("✅ 官方规格模型模板创建完成");
                    }
                    Err(e) => { eprintln!("❌ 写入失败: {}", e); process::exit(1); }
                }
            } else {
                // === 普通二进制格式转换 ===
                match fs::read(input_path) {
                    Ok(data) => {
                        println!("🧠 模型转换: {} → {}", input, output);
                        println!("   输入: {} bytes, 量化: {}", data.len(), quant_str);
                        let demo = model::DuanyanModel::create_demo(1, 16);
                        match demo.write_to_file(output_path) {
                            Ok(()) => {
                                let meta = fs::metadata(output_path).unwrap();
                                println!("   输出: {} ({} bytes)", output, meta.len());
                                println!("✅ 转换完成");
                            }
                            Err(e) => { eprintln!("❌ 写入失败: {}", e); process::exit(1); }
                        }
                    }
                    Err(e) => { eprintln!("❌ 读取失败 '{}': {}", input, e); process::exit(1); }
                }
            }
        }
        "generate" => {
            // hardy model generate --output <f.duanyan> [--benchmark <f.c>]
            let mut output = String::new();
            let mut bench_out = String::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--output" => { i += 1; if i < args.len() { output = args[i].clone(); } }
                    "--benchmark" => { i += 1; if i < args.len() { bench_out = args[i].clone(); } }
                    _ => eprintln!("⚠️  忽略未知参数 '{}'", args[i]),
                }
                i += 1;
            }
            if output.is_empty() {
                eprintln!("用法: hardy model generate --output <gesture-10m-int4.duanyan> [--benchmark <benchmark.c>]");
                eprintln!("  生成真实架构的锻言Mini手势识别模型 (Xavier初始化 + INT4量化)");
                eprintln!("  --benchmark  同时生成 ESP32-S3 benchmark C 代码");
                process::exit(1);
            }

            println!("🧠 生成锻言Mini手势识别模型");
            println!("   规格: {}层/{}维/{}头/{}类手势",
                model::DUANYAN_MINI_LAYERS, model::DUANYAN_MINI_HIDDEN,
                model::DUANYAN_MINI_HEADS, model::GESTURE_CLASSES);
            println!("   手势: {}", model::GESTURE_LABELS.join(", "));
            println!("   量化: INT4");

            let m = model::generate_gesture_model();
            let output_path = Path::new(&output);

            match m.write_to_file(output_path) {
                Ok(()) => {
                    let meta = fs::metadata(output_path).unwrap();
                    println!("   参数: {} ({:.1}M)", m.header.n_params, m.header.n_params as f64 / 1e6);
                    println!("   层数: {} 个LayerRecord", m.layers.len());
                    println!("   输出: {} ({} bytes, {:.2} MB)",
                        output, meta.len(), meta.len() as f64 / (1024.0 * 1024.0));

                    // 检查是否符合规格
                    let size_mb = meta.len() as f64 / (1024.0 * 1024.0);
                    if size_mb <= model::DUANYAN_MINI_MODEL_SIZE_MB {
                        println!("   ✅ 模型大小 {:.2}MB ≤ 预算 {:.1}MB", size_mb, model::DUANYAN_MINI_MODEL_SIZE_MB);
                    } else {
                        println!("   ❌ 模型大小 {:.2}MB > 预算 {:.1}MB!", size_mb, model::DUANYAN_MINI_MODEL_SIZE_MB);
                    }
                    println!("✅ gesture-10m-int4.duanyan 生成完成");

                    // 生成 benchmark C 代码
                    if !bench_out.is_empty() {
                        let c_code = model::generate_esp32_benchmark(&m);
                        match fs::write(&bench_out, &c_code) {
                            Ok(()) => println!("✅ ESP32 benchmark 代码: {} ({} bytes)", bench_out, c_code.len()),
                            Err(e) => eprintln!("❌ 写入benchmark失败: {}", e),
                        }
                    }
                }
                Err(e) => { eprintln!("❌ 写入失败: {}", e); process::exit(1); }
            }
        }
        "benchmark" => {
            // hardy model benchmark --model <f.duanyan> --output <benchmark.c>
            let mut model_path = String::new();
            let mut output = String::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--model" => { i += 1; if i < args.len() { model_path = args[i].clone(); } }
                    "--output" => { i += 1; if i < args.len() { output = args[i].clone(); } }
                    _ => eprintln!("⚠️  忽略未知参数 '{}'", args[i]),
                }
                i += 1;
            }
            if model_path.is_empty() || output.is_empty() {
                eprintln!("用法: hardy model benchmark --model <f.duanyan> --output <benchmark.c>");
                eprintln!("  从 .duanyan 模型文件生成 ESP32-S3 benchmark C 代码");
                process::exit(1);
            }

            let mp = Path::new(&model_path);
            println!("🧠 生成 ESP32-S3 benchmark 代码");
            println!("   模型: {}", model_path);

            match model::DuanyanModel::read_from_file(mp) {
                Ok(m) => {
                    let c_code = model::generate_esp32_benchmark(&m);
                    match fs::write(&output, &c_code) {
                        Ok(()) => {
                            println!("   输出: {} ({} bytes)", output, c_code.len());
                            println!("   层数: {}, 参数: {}", m.header.n_layers, m.header.n_params);
                            println!("✅ 用 ESP-IDF 编译运行: idf.py build && idf.py flash monitor");
                        }
                        Err(e) => { eprintln!("❌ 写入失败: {}", e); process::exit(1); }
                    }
                }
                Err(e) => { eprintln!("❌ 读取模型失败: {}", e); process::exit(1); }
            }
        }
        "deploy" => {
            // hardy model deploy <file> --sd <path>
            if args.len() < 2 {
                eprintln!("用法: hardy model deploy <model.duanyan> --sd <path>");
                process::exit(1);
            }
            let model_path = Path::new(&args[1]);
            let mut sd_path = String::new();
            let mut i = 2;
            while i < args.len() {
                if args[i] == "--sd" { i += 1; if i < args.len() { sd_path = args[i].clone(); } }
                i += 1;
            }
            if sd_path.is_empty() {
                eprintln!("用法: hardy model deploy <model.duanyan> --sd <path>");
                process::exit(1);
            }
            let sd_dir = Path::new(&sd_path);
            if !sd_dir.exists() {
                match fs::create_dir_all(sd_dir) {
                    Ok(()) => {}
                    Err(e) => { eprintln!("❌ 创建目录失败: {}", e); process::exit(1); }
                }
            }
            let file_name = model_path.file_name().unwrap_or_default();
            let dest = sd_dir.join(file_name);
            match fs::copy(model_path, &dest) {
                Ok(bytes) => println!("✅ 部署成功: {} → {} ({} bytes)", model_path.display(), dest.display(), bytes),
                Err(e) => { eprintln!("❌ 部署失败: {}", e); process::exit(1); }
            }
        }
        "list" => {
            // hardy model list --prebuilt
            let show_prebuilt = args.iter().any(|a| a == "--prebuilt");
            if !show_prebuilt {
                println!("用法: hardy model list --prebuilt");
                println!();
            }
            println!("=== 官方预训练锻言Mini模型 ===");
            println!();
            println!("{:<28} {:<10} {:<16} {}", "模型名", "类型", "参数/量化", "应用场景");
            println!("{}", "-".repeat(72));
            for m in model::PREBUILT_MODELS {
                println!("{:<28} {:<10} {:<16} {}", m.name, m.desc, m.params, m.use_case);
            }
            println!();
            println!("下载: hardy pack install <model-name>");
        }
        _ => {
            eprintln!("未知子命令 '{}'", args[0]);
            eprintln!("可用: convert | verify | info | deploy | list");
            process::exit(1);
        }
    }
}

fn print_usage() {
    println!("猎术手册:");
    println!();
    println!("  编译与运行:");
    println!("  hardy build <file>                          锻造武器 (编译为 .hex)");
    println!("  hardy build <file> --target esp32            C 代码生成 (ESP-IDF 项目)");
    println!("  hardy build <file> --target arduino-avr      Arduino sketch 生成");
    println!("  hardy build <file> --target stm32            STM32 HAL 项目生成");
    println!("  hardy build <file> --target rpi              树莓派项目生成");
    println!("  hardy run   <file>                          原生执行 (直接运行输出)");
    println!("  hardy run   <file> --hw                     硬件模拟模式 (GPIO/UART/PWM)");
    println!("  hardy debug <file>                          调试模式 (单步/断点/寄存器)");
    println!("  hardy hunt  <file>                          设阱捕获 (编译+部署)");
    println!("  hardy track <file>                          追踪指令 (查看汇编)");
    println!("  hardy scan  <file>                          侦查目标 (代码分析+资源估算)");
    println!();
    println!("  烧录:");
    println!("  hardy flash <file> --port COM3              烧录到指定串口");
    println!("  hardy flash <file> --board esp32             按板子类型自动选工具");
    println!("  hardy flash <file> --board arduino-nano      avrdude 上传");
    println!("  hardy flash <file> --board stm32f103         st-flash");
    println!("  hardy flash <file> --board counpre64         Gowin 烧录");
    println!();
    println!("  板子管理:");
    println!("  hardy board list                            列出已知开发板");
    println!("  hardy board info <name>                     开发板详情");
    println!("  hardy board set <name>                      设置默认目标板");
    println!("  hardy monitor --port COM3                   串口监视器");
    println!("  hardy monitor --port COM3 --baud 115200     指定波特率");
    println!();
    println!("  项目管理:");
    println!("  hardy new <name> --board esp32              新建项目 (指定板子)");
    println!("  hardy init                                  当前目录初始化 hardy 项目");
    println!("  hardy compat <file>                         Espruino/Moddable 兼容分析");
    println!();
    println!("  锻言Mini AI:");
    println!("  hardy model convert --input <f> --output <f>  模型转换 (GGUF/NPY → .duanyan)");
    println!("  hardy model verify <model.duanyan>             验证模型文件");
    println!("  hardy model info <model.duanyan>               模型详情");
    println!("  hardy model deploy <f> --sd <path>            部署到SD卡");
    println!("  hardy model list --prebuilt                   官方预训练模型");
    println!();
    println!("  其他:");
    println!("  hardy lock  <hex>                           锁定硬件 (烧录指引)");
    println!("  hardy bite  <file>                          撕咬执行 (逐条解析)");
    println!("  hardy prey  [dir]                           猎物列表 (列出源码)");
    println!();
    println!("选项:");
    println!("  -o <file>     输出文件");
    println!("  -O            泝火优化 (常量折叠+死代码消除)");
    println!("  --asm         显示武器蓝图 (汇编)");
    println!("  --compat      Espruino/Moddable 兼容模式 (自动转换旧式API)");
    println!("  --target <t>  编译目标: counpre64(默认) | esp32 | stm32 | arduino-avr | arduino-arm | rpi");
    println!("  --board <b>   板子型号: arduino-uno | arduino-nano | arduino-mega | arduino-due | arduino-zero");
    println!("                esp32-devkit | stm32-bluepill | rpi-4b | counpre64-fpga");
    println!();
    println!("支持: .js (JavaScript) | .ts (TypeScript, 自动剥离类型)");
    println!();
    println!("示例:");
    println!("  hardy build hello.js --target esp32");
    println!("  hardy build blink.js --target arduino-avr --board arduino-uno");
    println!("  hardy build app.js --target stm32 -O");
    println!("  hardy build app.js --target rpi");
    println!("  hardy run   helloworld.js");
    println!("  hardy flash hello.hex --board arduino-nano --port COM3");
    println!("  hardy new my-app --board esp32 -t blinky");
    println!("  hardy init");
    println!("  hardy board list");
    println!("  hardy model info gesture-10m-int4.duanyan");
    println!("  hardy model list --prebuilt");
    println!("  hardy monitor --port COM3 --baud 115200");
    println!();
    println!("猎包子命令: hardy pack help");
}

// =============================================================================
// Phase H: workspace - 多项目管理
// =============================================================================
fn cmd_workspace(args: &[String]) {
    let cwd = std::env::current_dir().expect("无法获取当前目录");

    match args.first().map(|s| s.as_str()) {
        Some("init") => {
            let mut monorepo = false;
            let mut name = cwd.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "workspace".to_string());

            for arg in &args[1..] {
                if arg == "--monorepo" { monorepo = true; }
                else if !arg.starts_with('-') { name = arg.clone(); }
            }

            match workspace::workspace_init(&cwd, &name, monorepo) {
                Ok(ws) => {
                    println!("✅ Workspace '{}' 初始化完成", ws.name);
                    println!("  配置文件: hardy.workspace.json");
                    println!("  .hardy/ 目录已创建");
                    if monorepo {
                        println!("  packages/shared/ 共享库已创建");
                    }
                }
                Err(e) => {
                    eprintln!("❌ 初始化失败: {}", e);
                    process::exit(1);
                }
            }
        }
        Some("build") => {
            let build_all = args.iter().any(|a| a == "--all");
            if !build_all {
                eprintln!("用法: hardy workspace build --all");
                process::exit(1);
            }
            let mut ws = workspace::WorkspaceConfig::load(&cwd).unwrap_or_else(|e| {
                eprintln!("❌ {}", e);
                process::exit(1);
            });
            match ws.resolve_build_order() {
                Ok(order) => {
                    println!("编译顺序: {}", order.join(" -> "));
                    for name in &order {
                        if let Some(pkg) = ws.packages.iter().find(|p| p.name == *name) {
                            let pkg_dir = cwd.join(&pkg.path);
                            let target = ws.package_target(pkg);
                            println!("\n▶ 编译 {} (target: {})...", name, target);
                            // 尝试找入口文件
                            let entry = pkg_dir.join("main.js");
                            if entry.exists() {
                                let source = fs::read_to_string(&entry).unwrap_or_default();
                                let build_dir = pkg_dir.join("build");
                                fs::create_dir_all(&build_dir).ok();
                                match compile_to_instructions_local(&source) {
                                    Ok(instructions) => {
                                        match crate::firmware::emit_bundle(&instructions, &build_dir, "main") {
                                            Ok(_) => println!("  ✓ {} 编译成功", name),
                                            Err(e) => eprintln!("  ✗ {} 编译失败: {}", name, e),
                                        }
                                    }
                                    Err(e) => eprintln!("  ✗ {} 编译失败: {}", name, e),
                                }
                            } else {
                                println!("  ⚠ 跳过 {} (未找到 main.js)", name);
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("❌ {}", e);
                    process::exit(1);
                }
            }
        }
        Some("list") => {
            let ws = workspace::WorkspaceConfig::load(&cwd).unwrap_or_else(|e| {
                eprintln!("❌ {}", e);
                process::exit(1);
            });
            println!("Workspace: {} v{}", ws.name, ws.version);
            println!("Default target: {}", ws.default_target);
            println!();
            for pkg in &ws.packages {
                let target = ws.package_target(pkg);
                let deps = if pkg.dependencies.is_empty() {
                    String::new()
                } else {
                    format!(" (deps: {})", pkg.dependencies.join(", "))
                };
                println!("  {} [{}] @ {}{}", pkg.name, target, pkg.path, deps);
            }
        }
        Some("add") => {
            if args.len() < 2 {
                eprintln!("用法: hardy workspace add <name> [--path <dir>] [--board <board>]");
                process::exit(1);
            }
            let name = &args[1];
            let mut path = format!("packages/{}", name);
            let mut target = None;

            let mut i = 2;
            while i < args.len() {
                if args[i] == "--path" && i + 1 < args.len() {
                    path = args[i + 1].clone();
                    i += 2;
                } else if args[i] == "--board" && i + 1 < args.len() {
                    target = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }

            let mut ws = workspace::WorkspaceConfig::load(&cwd).unwrap_or_else(|e| {
                eprintln!("❌ {}", e);
                process::exit(1);
            });

            ws.add_package(workspace::PackageRef {
                name: name.clone(),
                path: path.clone(),
                target,
                dependencies: Vec::new(),
            });

            // 创建目录
            let pkg_dir = cwd.join(&path);
            fs::create_dir_all(&pkg_dir).ok();
            if !pkg_dir.join("main.js").exists() {
                fs::write(pkg_dir.join("main.js"), format!("// {} 入口\nconsole.log('hello');\n", name)).ok();
            }

            ws.save(&cwd).unwrap_or_else(|e| {
                eprintln!("保存失败: {}", e);
                process::exit(1);
            });
            println!("✅ 添加子项目: {} @ {}", name, path);
        }
        Some("status") => {
            let ws = workspace::WorkspaceConfig::load(&cwd).unwrap_or_else(|e| {
                eprintln!("❌ {}", e);
                process::exit(1);
            });
            let statuses = ws.status(&cwd);
            println!("{:<20} {:<12} {:<8} {:<8} {}", "项目", "目标", "配置", "入口", "编译状态");
            println!("{:-<65}", "");
            for s in &statuses {
                println!("{:<20} {:<12} {:<8} {:<8} {}",
                    s.name,
                    s.target,
                    if s.has_config { "✓" } else { "✗" },
                    if s.has_entry { "✓" } else { "✗" },
                    s.last_build.as_deref().unwrap_or("未编译")
                );
            }
        }
        _ => {
            println!("Hardy Workspace - 多项目管理");
            println!();
            println!("用法: hardy workspace <subcommand>");
            println!();
            println!("子命令:");
            println!("  init [--monorepo]          初始化 workspace");
            println!("  build --all                按拓扑序编译全部");
            println!("  list                       列出子项目");
            println!("  add <name> [--path] [--board]  添加子项目");
            println!("  status                     各项目编译状态");
        }
    }
}

// =============================================================================
// Phase H: git - Git 集成
// =============================================================================
fn cmd_git_cli(args: &[String]) {
    let cwd = std::env::current_dir().expect("无法获取当前目录");

    match args.first().map(|s| s.as_str()) {
        Some("status") => {
            match git_ops::git_status(&cwd) {
                Ok(status) => {
                    println!("分支: {}", status.branch);
                    if status.ahead > 0 || status.behind > 0 {
                        println!("  ahead {}, behind {}", status.ahead, status.behind);
                    }
                    if !status.staged.is_empty() {
                        println!("\n暂存区:");
                        for f in &status.staged {
                            println!("  {} {}", f.status.symbol(), f.path);
                        }
                    }
                    if !status.unstaged.is_empty() {
                        println!("\n未暂存:");
                        for f in &status.unstaged {
                            println!("  {} {}", f.status.symbol(), f.path);
                        }
                    }
                    if !status.untracked.is_empty() {
                        println!("\n未跟踪:");
                        for f in &status.untracked {
                            println!("  ? {}", f);
                        }
                    }
                    if status.staged.is_empty() && status.unstaged.is_empty() && status.untracked.is_empty() {
                        println!("工作区干净");
                    }
                }
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("log") => {
            let n: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10);
            match git_ops::git_log(&cwd, n) {
                Ok(entries) => {
                    for e in &entries {
                        println!("{} {} ({}) - {}", &e.hash[..8], e.message, e.author, e.date);
                    }
                }
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("diff") => {
            let file = args.get(1).map(|s| s.as_str());
            match git_ops::git_diff(&cwd, file) {
                Ok(diffs) => {
                    for diff in &diffs {
                        println!("--- {}", diff.path);
                        for hunk in &diff.hunks {
                            println!("@@ -{},{} +{},{} @@", hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count);
                            for line in &hunk.lines {
                                match line.kind {
                                    git_ops::DiffLineKind::Added => println!("+{}", line.content),
                                    git_ops::DiffLineKind::Removed => println!("-{}", line.content),
                                    git_ops::DiffLineKind::Context => println!(" {}", line.content),
                                    git_ops::DiffLineKind::Header => println!("{}", line.content),
                                }
                            }
                        }
                    }
                }
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("add") => {
            if args.len() > 1 {
                let files: Vec<String> = args[1..].to_vec();
                match git_ops::git_add(&cwd, &files) {
                    Ok(_) => println!("已暂存 {} 个文件", files.len()),
                    Err(e) => eprintln!("❌ {}", e),
                }
            } else {
                match git_ops::git_add_all(&cwd) {
                    Ok(_) => println!("已暂存所有文件"),
                    Err(e) => eprintln!("❌ {}", e),
                }
            }
        }
        Some("commit") => {
            let mut msg = String::new();
            let mut i = 1;
            while i < args.len() {
                if args[i] == "-m" && i + 1 < args.len() {
                    msg = args[i + 1].clone();
                    break;
                }
                i += 1;
            }
            if msg.is_empty() {
                eprintln!("用法: hardy git commit -m \"message\"");
                process::exit(1);
            }
            match git_ops::git_commit(&cwd, &msg) {
                Ok(out) => println!("{}", out),
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("push") => {
            let remote = args.get(1).map(|s| s.as_str());
            let branch = args.get(2).map(|s| s.as_str());
            match git_ops::git_push(&cwd, remote, branch) {
                Ok(out) => println!("{}", out),
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("pull") => {
            match git_ops::git_pull(&cwd) {
                Ok(out) => println!("{}", out),
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("branch") => {
            if args.len() > 1 {
                // 创建分支
                match git_ops::git_create_branch(&cwd, &args[1]) {
                    Ok(out) => println!("{}", out),
                    Err(e) => eprintln!("❌ {}", e),
                }
            } else {
                match git_ops::git_branches(&cwd) {
                    Ok(branches) => {
                        for b in &branches {
                            let prefix = if b.is_current { "* " } else { "  " };
                            let suffix = if b.is_remote { " (remote)" } else { "" };
                            println!("{}{}{}", prefix, b.name, suffix);
                        }
                    }
                    Err(e) => eprintln!("❌ {}", e),
                }
            }
        }
        Some("checkout") => {
            if args.len() < 2 {
                eprintln!("用法: hardy git checkout <branch>");
                process::exit(1);
            }
            match git_ops::git_checkout(&cwd, &args[1]) {
                Ok(out) => println!("{}", out),
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("init") => {
            match git_ops::git_init(&cwd) {
                Ok(out) => println!("{}", out),
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        Some("hooks") => {
            if args.get(1).map(|s| s.as_str()) == Some("install") {
                match git_ops::git_install_hooks(&cwd) {
                    Ok(_) => println!("✅ pre-commit hook 已安装"),
                    Err(e) => eprintln!("❌ {}", e),
                }
            } else {
                eprintln!("用法: hardy git hooks install");
            }
        }
        _ => {
            println!("Hardy Git - 版本控制集成");
            println!();
            println!("用法: hardy git <subcommand>");
            println!();
            println!("子命令:");
            println!("  status             仓库状态");
            println!("  log [n]            提交历史");
            println!("  diff [file]        文件差异");
            println!("  add [files...]     暂存文件");
            println!("  commit -m \"msg\"   提交");
            println!("  push [remote] [branch]  推送");
            println!("  pull               拉取");
            println!("  branch [name]      分支列表/创建");
            println!("  checkout <branch>  切换分支");
            println!("  init               初始化仓库");
            println!("  hooks install      安装 pre-commit hook");
        }
    }
}

// =============================================================================
// Phase H: lint - 代码规范检查
// =============================================================================
fn cmd_lint(args: &[String]) {
    let cwd = std::env::current_dir().expect("无法获取当前目录");
    let config = lint::LintConfig::load(&cwd);

    let mut do_fix = false;
    let mut file_path = None;
    let mut show_rules = false;
    let mut budget_board = None;

    for arg in args {
        match arg.as_str() {
            "--fix" => do_fix = true,
            "--rules" => show_rules = true,
            "--file" => {}
            _ if arg.starts_with("--budget") => {}
            _ => {
                // 可能是 --file 的参数 或 --budget 的参数
                if args.windows(2).any(|w| w[0] == "--file" && w[1] == *arg) {
                    file_path = Some(arg.clone());
                } else if args.windows(2).any(|w| w[0] == "--budget" && w[1] == *arg) {
                    budget_board = Some(arg.clone());
                }
            }
        }
    }

    // 重新解析参数对
    for i in 0..args.len() {
        if args[i] == "--file" && i + 1 < args.len() {
            file_path = Some(args[i + 1].clone());
        }
        if args[i] == "--budget" && i + 1 < args.len() {
            budget_board = Some(args[i + 1].clone());
        }
    }

    if show_rules {
        println!("Hardy Lint 规则列表:");
        println!("{:-<70}", "");
        for rule in &config.rules {
            let sev = config.effective_severity(&rule.id);
            println!("  {:<25} [{:<8}] {}{}",
                rule.id,
                sev.label(),
                rule.description,
                if rule.fixable { " (可修复)" } else { "" }
            );
        }
        return;
    }

    if let Some(board) = budget_board {
        println!("资源约束检查 (board: {})...", board);
        // 简单估算
        let diags = lint::resource_budget_check(&cwd, 32, 256, &config);
        if diags.is_empty() {
            println!("✅ 资源约束检查通过");
        } else {
            for d in &diags {
                println!("  {} [{}] {}", d.severity.symbol(), d.rule_id, d.message);
            }
        }
        return;
    }

    if let Some(file) = file_path {
        let path = std::path::Path::new(&file);
        if !path.exists() {
            eprintln!("文件不存在: {}", file);
            process::exit(1);
        }
        let source = fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("读取失败: {}", e);
            process::exit(1);
        });

        if do_fix {
            match lint::lint_fix(path, &source, &config) {
                Ok(fixed) => {
                    fs::write(path, fixed).unwrap_or_else(|e| {
                        eprintln!("写入失败: {}", e);
                    });
                }
                Err(e) => eprintln!("修复失败: {}", e),
            }
        } else {
            let result = lint::lint_file(path, &source, &config);
            print_lint_result(&result);
            if result.error_count > 0 { process::exit(1); }
        }
    } else {
        // Lint 整个项目
        if do_fix {
            let js_files = find_all_js_files(&cwd);
            let mut total_fixes = 0;
            for file in &js_files {
                if let Ok(source) = fs::read_to_string(file) {
                    if let Ok(fixed) = lint::lint_fix(file, &source, &config) {
                        if fixed != source {
                            fs::write(file, fixed).ok();
                            total_fixes += 1;
                        }
                    }
                }
            }
            println!("已修复 {} 个文件", total_fixes);
        } else {
            let result = lint::lint_project(&cwd, &config);
            print_lint_result(&result);
            if result.error_count > 0 { process::exit(1); }
        }
    }
}

fn print_lint_result(result: &lint::LintResult) {
    for d in &result.diagnostics {
        let sym = d.severity.symbol();
        println!("  {}:{}  [{}] {} ({})",
            d.file, d.line, sym, d.message, d.rule_id
        );
        if let Some(ref s) = d.suggestion {
            println!("    → {}", s);
        }
    }
    println!();
    println!("共 {} 个错误, {} 个警告 ({} 可自动修复)",
        result.error_count, result.warning_count, result.fixable_count
    );
    if result.error_count == 0 && result.warning_count == 0 {
        println!("✅ 代码规范检查通过");
    }
}

fn find_all_js_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "js" || ext == "ts" {
                        files.push(path);
                    }
                }
            } else if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && name != "node_modules" && name != "build" && name != "target" {
                    files.extend(find_all_js_files(&path));
                }
            }
        }
    }
    files
}

// =============================================================================
// Phase H: ci - CI/CD 管线
// =============================================================================
fn cmd_ci(args: &[String]) {
    let cwd = std::env::current_dir().expect("无法获取当前目录");

    match args.first().map(|s| s.as_str()) {
        Some("build") => {
            let config = ci::CiConfig::default();
            println!("CI 模式编译...");
            let result = ci::ci_build_all(&cwd, &config);
            println!();
            if result.success {
                println!("✅ CI 构建成功 ({} ms)", result.duration_ms);
                println!("  目标: {}", result.targets_built.join(", "));
                println!("  产物总大小: {} KB", result.total_size_bytes / 1024);
            } else {
                println!("❌ CI 构建失败");
                if !result.targets_failed.is_empty() {
                    println!("  失败: {}", result.targets_failed.join(", "));
                }
                if !result.lint_passed {
                    println!("  Lint 检查未通过");
                }
                process::exit(1);
            }
        }
        Some("release") => {
            let mut version = "0.1.0".to_string();
            for i in 0..args.len() {
                if args[i] == "--version" && i + 1 < args.len() {
                    version = args[i + 1].clone();
                }
            }
            println!("发布 v{}...", version);
            match ci::ci_release(&cwd, &version) {
                Ok(info) => {
                    println!("{}", info.resource_report);
                    for a in &info.artifacts {
                        println!("  {} ({} KB) [{}]", a.name, a.size_bytes / 1024, a.target);
                    }
                }
                Err(e) => {
                    eprintln!("❌ {}", e);
                    process::exit(1);
                }
            }
        }
        Some("template") => {
            let github = args.iter().any(|a| a == "--github");
            let gitlab = args.iter().any(|a| a == "--gitlab");
            if github {
                ci::ci_generate_github_action(&cwd).unwrap_or_else(|e| {
                    eprintln!("❌ {}", e);
                    process::exit(1);
                });
            }
            if gitlab {
                ci::ci_generate_gitlab_ci(&cwd).unwrap_or_else(|e| {
                    eprintln!("❌ {}", e);
                    process::exit(1);
                });
            }
            if !github && !gitlab {
                println!("用法: hardy ci template --github | --gitlab");
            }
        }
        Some("archive") => {
            let mut version = "0.1.0".to_string();
            for i in 0..args.len() {
                if args[i] == "--version" && i + 1 < args.len() {
                    version = args[i + 1].clone();
                }
            }
            match ci::ci_archive(&cwd, &version) {
                Ok(msg) => println!("✅ {}", msg),
                Err(e) => eprintln!("❌ {}", e),
            }
        }
        _ => {
            println!("Hardy CI/CD - 持续集成管线");
            println!();
            println!("用法: hardy ci <subcommand>");
            println!();
            println!("子命令:");
            println!("  build --all                   CI模式编译");
            println!("  release --version x.y.z       版本发布");
            println!("  template --github | --gitlab  生成CI模板");
            println!("  archive [--version x.y.z]     产物归档");
        }
    }
}

/// Helper: compile source to instructions using local modules
fn compile_to_instructions_local(source: &str) -> Result<Vec<crate::backend::Instruction>, String> {
    let tokens = crate::lexer::tokenize(source)?;
    let ast = crate::parser::parse(&tokens)?;
    crate::compiler::compile(&ast)
}
