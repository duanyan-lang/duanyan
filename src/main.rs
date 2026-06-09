//! SNAR - The Snare Compiler
//!
//! 捕猎主题命令行: JavaScript/TypeScript → COUNPRE64 机器码
//!
//! 🪤 "在代码的丛林中，SNAR 是无形的陷阱"

use std::env;
use std::fs;
use std::process;

mod lexer;
mod parser;
mod compiler;
mod backend;
mod optimizer;
mod strip;

const BANNER: &str = r#"
   ____  _   _    _    ____  
  / ___|| \ | |  / \  |  _ \ 
  \___ \|  \| | / _ \ | |_) |
   ___) | |\  |/ ___ \|  _ < 
  |____/|_| \_/_/   \_\_| \_\  v{VERSION}
  
  🪤 The Snare Compiler
  JavaScript/TypeScript → COUNPRE64
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
        "bite"    => cmd_bite(&args[2..]),
        "prey"    => cmd_prey(&args[2..]),
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
// snar build - 编译（打造武器）
// =============================================================================
fn cmd_build(args: &[String]) {
    if args.is_empty() {
        eprintln!("🔨 snar build: 请指定猎物源码");
        eprintln!("用法: snar build <input.js|.ts> [-o output.hex] [--asm] [-O]");
        process::exit(1);
    }

    let input_file = &args[0];
    let mut output_file = String::new();
    let mut emit_asm = false;
    let mut do_optimize = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => { i += 1; if i < args.len() { output_file = args[i].clone(); } }
            "--asm" | "--emit-asm" => emit_asm = true,
            "-O" | "--optimize" => do_optimize = true,
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

    println!("🔨 锻造武器: {} → {}", input_file, output_file);
    if do_optimize { println!("   ⚡ 淬火优化: 开启"); }

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
            println!("   ✅ 武器锻造完成: {}", output_file);
        }
        Err(e) => {
            eprintln!("❌ 锻造失败: {}", e);
            process::exit(1);
        }
    }
}

// =============================================================================
// snar hunt - 运行（捕猎执行）: build + flash
// =============================================================================
fn cmd_hunt(args: &[String]) {
    if args.is_empty() {
        eprintln!("🪤 snar hunt: 请指定猎物");
        eprintln!("用法: snar hunt <input.js|.ts> [--port COM3]");
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
    println!("   → 使用 'snar lock' 锁定硬件目标");
    println!("   → 或手动: Gowin Programmer → Load '{}'", hex_file);
}

// =============================================================================
// snar track - 追踪（调试输出汇编）
// =============================================================================
fn cmd_track(args: &[String]) {
    if args.is_empty() {
        eprintln!("🔍 snar track: 请指定追踪目标");
        eprintln!("用法: snar track <input.js|.ts>");
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
// snar scan - 侦查（扫描代码，报告统计）
// =============================================================================
fn cmd_scan(args: &[String]) {
    if args.is_empty() {
        eprintln!("👁️ snar scan: 请指定侦查目标");
        eprintln!("用法: snar scan <input.js|.ts>");
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
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  ✅ 侦查完毕，猎物可捕获");
}

// =============================================================================
// snar lock - 锁定硬件（烧录提示）
// =============================================================================
fn cmd_lock(args: &[String]) {
    let hex_file = if !args.is_empty() {
        args[0].clone()
    } else {
        eprintln!("🎯 snar lock: 请指定 .hex 文件");
        eprintln!("用法: snar lock <file.hex> [--port COM3]");
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
// snar bite - 单步调试（撕咬执行）
// =============================================================================
fn cmd_bite(args: &[String]) {
    if args.is_empty() {
        eprintln!("🦷 snar bite: 请指定目标");
        eprintln!("用法: snar bite <input.js|.ts>");
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
// snar prey - 猎物列表
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

/// 指令描述
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

fn print_usage() {
    println!("🪤 猎术手册:");
    println!();
    println!("  snar build <file>   🔨 锻造武器 (编译为 .hex)");
    println!("  snar hunt  <file>   🪤 设阱捕获 (编译+部署)");
    println!("  snar track <file>   🔍 追踪指令 (查看汇编)");
    println!("  snar scan  <file>   👁️  侦查目标 (代码分析)");
    println!("  snar lock  <hex>    🎯 锁定硬件 (烧录指引)");
    println!("  snar bite  <file>   🦷 撕咬执行 (逐条解析)");
    println!("  snar prey  [dir]    🎯 猎物列表 (列出源码)");
    println!();
    println!("选项:");
    println!("  -o <file>     输出文件");
    println!("  -O            淬火优化 (常量折叠+死代码消除)");
    println!("  --asm         显示武器蓝图 (汇编)");
    println!();
    println!("支持: .js (JavaScript) | .ts (TypeScript, 自动剥离类型)");
    println!();
    println!("示例:");
    println!("  snar build hello.ts -O --asm");
    println!("  snar hunt  fibonacci.js");
    println!("  snar scan  complex.ts");
    println!("  snar track loop.js");
}
