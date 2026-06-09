//! SNARjs - JavaScript to COUNPRE64 Cross-Compiler
//!
//! 将 JavaScript 源码编译为 COUNPRE64 自主指令集机器码
//! Phase 4: 高级特性 (console.log, 数组, 优化)

use std::env;
use std::fs;
use std::process;

mod lexer;
mod parser;
mod compiler;
mod backend;
mod optimizer;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        process::exit(1);
    }

    match args[1].as_str() {
        "compile" => cmd_compile(&args[2..]),
        "help" | "--help" | "-h" => print_usage(),
        "version" | "--version" | "-V" => println!("snarjs {}", env!("CARGO_PKG_VERSION")),
        _ => {
            eprintln!("错误: 未知命令 '{}'", args[1]);
            print_usage();
            process::exit(1);
        }
    }
}

fn cmd_compile(args: &[String]) {
    if args.is_empty() {
        eprintln!("错误: 请指定输入文件");
        eprintln!("用法: snarjs compile <input.js> [-o output.hex] [--emit-asm] [--optimize]");
        process::exit(1);
    }

    let input_file = &args[0];
    let mut output_file = String::new();
    let mut emit_asm = false;
    let mut do_optimize = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output_file = args[i].clone();
                }
            }
            "--emit-asm" => emit_asm = true,
            "--optimize" | "-O" => do_optimize = true,
            _ => {
                eprintln!("警告: 忽略未知参数 '{}'", args[i]);
            }
        }
        i += 1;
    }

    if output_file.is_empty() {
        output_file = input_file.replace(".js", ".hex");
    }

    let source = match fs::read_to_string(input_file) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("错误: 无法读取文件 '{}': {}", input_file, e);
            process::exit(1);
        }
    };

    println!("SNARjs v{}", env!("CARGO_PKG_VERSION"));
    println!("编译: {} → {}", input_file, output_file);
    if do_optimize {
        println!("  优化: 开启 (常量折叠 + 死代码消除)");
    }

    // 1. 词法分析
    let tokens = match lexer::tokenize(&source) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("词法错误: {}", e);
            process::exit(1);
        }
    };
    println!("  词法分析: {} 个 token", tokens.len());

    // 2. 语法分析
    let ast = match parser::parse(&tokens) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("语法错误: {}", e);
            process::exit(1);
        }
    };
    println!("  语法分析: {} 个语句, {} 个函数", ast.statements.len(), ast.functions.len());

    // 3. 优化 (Phase 4)
    let final_ast = if do_optimize {
        let optimized = optimizer::optimize(&ast);
        println!("  优化后: {} 个语句", optimized.statements.len());
        optimized
    } else {
        ast
    };

    // 4. 代码生成
    let instructions = match compiler::compile(&final_ast) {
        Ok(instrs) => instrs,
        Err(e) => {
            eprintln!("编译错误: {}", e);
            process::exit(1);
        }
    };
    println!("  代码生成: {} 条指令 ({} bytes)", instructions.len(), instructions.len() * 4);

    // 5. 输出
    if emit_asm {
        let asm = backend::emit_asm(&instructions);
        println!("\n--- 汇编输出 ---");
        println!("{}", asm);
    }

    match backend::emit_hex(&instructions, &output_file) {
        Ok(_) => println!("  输出文件: {}", output_file),
        Err(e) => {
            eprintln!("输出错误: {}", e);
            process::exit(1);
        }
    }

    println!("\n编译成功!");
}

fn print_usage() {
    println!("SNARjs - JavaScript to COUNPRE64 Cross-Compiler");
    println!("Phase 4: console.log, 数组, 优化");
    println!();
    println!("用法:");
    println!("  snarjs compile <input.js> [-o output.hex] [--emit-asm] [--optimize]");
    println!("  snarjs help");
    println!("  snarjs version");
    println!();
    println!("选项:");
    println!("  -o, --output <file>   指定输出文件 (默认: input.hex)");
    println!("  --emit-asm            同时输出汇编代码");
    println!("  -O, --optimize        启用优化 (常量折叠+死代码消除)");
    println!();
    println!("示例:");
    println!("  snarjs compile hello.js");
    println!("  snarjs compile hello.js -o hello.hex --emit-asm --optimize");
    println!("  snarjs compile uart_test.js --emit-asm  # console.log → UART");
}
