//! Hardy PACK - 包管理器
//!
//! 子命令:
//!   init    - 初始化项目
//!   install - 安装包 (下载依赖)
//!   publish - 发布包
//!   update  - 更新包
//!   remove  - 移除包
//!   list    - 查看已安装的包
//!   hunt    - 一键安装 + 运行

pub mod json;
pub mod http;
pub mod manifest;
pub mod lockfile;
pub mod archive;
pub mod registry;
pub mod resolver;
pub mod hardyrc;

use std::path::Path;
use std::fs;
use std::io::{self, Write};
use manifest::Manifest;
use lockfile::{Lockfile, LockedPackage};
use registry::RegistryClient;

/// 子命令分发
pub fn dispatch(args: &[String]) {
    match args.first().map(|s| s.as_str()) {
        Some("init") => cmd_init(),
        Some("install") | Some("i") => cmd_install(&args[1..]),
        Some("publish") => cmd_publish(),
        Some("update") => cmd_update(&args[1..]),
        Some("remove") => cmd_remove(&args[1..]),
        Some("list") | Some("ls") => cmd_list(),
        Some("search") => cmd_search(&args[1..]),
        Some("hunt") => cmd_hunt(&args[1..]),
        Some("lock") => cmd_lock(),
        Some("verify") => cmd_verify(),
        Some(other) => {
            eprintln!("未知 pack 指令: '{}'", other);
            print_pack_usage();
            std::process::exit(1);
        }
        None => print_pack_usage(),
    }
}

/// 打印 pack 子命令帮助
pub fn print_pack_usage() {
    println!("Hardy PACK - 包管理器");
    println!();
    println!("用法: hardy pack <指令>");
    println!();
    println!("pack 指令:");
    println!("  init            初始化项目 (创建 hardy.json)");
    println!("  install [name]  安装包 (下载依赖), 简写: i");
    println!("  publish         发布包");
    println!("  update [name]   更新包");
    println!("  remove <name>   移除包");
    println!("  list            查看已安装的包, 简写: ls");
    println!("  search <query>  搜索包 (Registry)");
    println!("  hunt [file]     安装依赖并运行 (install + run)");
    println!("  lock            生成依赖锁定文件 (hardy.lock)");
    println!("  verify          验证依赖完整性");
    println!();
    println!("示例:");
    println!("  hardy pack init");
    println!("  hardy pack i math-utils");
    println!("  hardy pack install");
    println!("  hardy pack publish");
    println!("  hardy pack hunt index.js");
}

// =============================================================================
// hardy pack init - 初始化项目
// =============================================================================
fn cmd_init() {
    let cwd = std::env::current_dir().expect("无法获取当前目录");

    // 检查是否已有 hardy.json
    if cwd.join("hardy.json").exists() {
        eprintln!("hardy.json 已存在，跳过初始化");
        return;
    }

    // 从目录名推断项目名
    let default_name = cwd.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-project".to_string());

    println!("初始化项目");
    println!();

    // 交互式输入
    let name = prompt_input(&format!("包名称 ({})", default_name), &default_name);
    let version = prompt_input("版本号 (0.1.0)", "0.1.0");
    let desc = prompt_input("描述", "");
    let main = prompt_input("入口文件 (index.js)", "index.js");
    let author = prompt_input("作者", "");

    let mut m = manifest::create_default_manifest(&name, &version);
    m.description = desc;
    m.main = main;
    m.author = author;

    // 创建 hardy.json
    m.save(&cwd).unwrap_or_else(|e| {
        eprintln!("写入 hardy.json 失败: {}", e);
        std::process::exit(1);
    });

    // 创建 .hardy 目录
    let hardy_dir = cwd.join(".hardy").join("packages");
    fs::create_dir_all(&hardy_dir).ok();

    // 创建 .gitignore (如果不存在)
    let gitignore = cwd.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, ".hardy/\nhardy.lock\n").ok();
    }

    println!();
    println!("hardy.json 已创建");
    println!(".hardy/ 目录已创建");
    println!();
    println!("开始锻造吧!");
}

fn prompt_input(prompt: &str, default: &str) -> String {
    print!("{}: ", prompt);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let input = input.trim();
    if input.is_empty() {
        default.to_string()
    } else {
        input.to_string()
    }
}

// =============================================================================
// hardy pack install - 安装包
// =============================================================================
fn cmd_install(args: &[String]) {
    let cwd = std::env::current_dir().expect("无法获取当前目录");

    if args.is_empty() {
        // 安装所有依赖 (从 hardy.json)
        install_all(&cwd);
    } else {
        // 安装指定包
        let pkg_name = &args[0];
        let version_constraint = if args.len() > 1 { Some(args[1].as_str()) } else { None };
        install_one(&cwd, pkg_name, version_constraint);
    }
}

fn install_all(dir: &Path) {
    let manifest = Manifest::load(dir).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    if manifest.dependencies.is_empty() {
        println!("无依赖需要安装");
        return;
    }

    println!("安装依赖...");
    let client = RegistryClient::new(&registry::get_registry_url());
    let mut lockfile = Lockfile::load(dir).unwrap_or_else(|_| Lockfile { packages: Vec::new() });

    for (name, constraint_str) in &manifest.dependencies {
        print!("  安装 {}@{}... ", name, constraint_str);
        match install_package(&client, name, constraint_str, dir, &mut lockfile) {
            Ok(version) => println!("{}@{}", name, version),
            Err(e) => println!("失败: {}", e),
        }
    }

    lockfile.save(dir).unwrap_or_else(|e| {
        eprintln!("写入 hardy.lock 失败: {}", e);
    });

    println!();
    println!("安装完成! {} 个包已安装", manifest.dependencies.len());
}

fn install_one(dir: &Path, name: &str, version_constraint: Option<&str>) {
    let constraint = version_constraint.unwrap_or("*");
    println!("安装包: {}@{}", name, constraint);

    let client = RegistryClient::new(&registry::get_registry_url());
    let mut lockfile = Lockfile::load(dir).unwrap_or_else(|_| Lockfile { packages: Vec::new() });
    let mut manifest = Manifest::load(dir).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    match install_package(&client, name, constraint, dir, &mut lockfile) {
        Ok(version) => {
            manifest.add_dependency(name, constraint);
            manifest.save(dir).ok();
            lockfile.save(dir).ok();
            println!("安装成功: {}@{}", name, version);
        }
        Err(e) => {
            eprintln!("安装失败: {}", e);
            std::process::exit(1);
        }
    }
}

fn install_package(
    client: &RegistryClient,
    name: &str,
    constraint_str: &str,
    dir: &Path,
    lockfile: &mut Lockfile,
) -> Result<String, String> {
    // 1. 查询可用版本
    let pkg_info = client.get_package(name)?;

    // 2. 解析版本约束
    let constraint = resolver::VersionConstraint::parse(constraint_str)?;
    let version = constraint.best_match(&pkg_info.versions)
        .ok_or_else(|| format!("无匹配版本 (可用: {})", pkg_info.versions.join(", ")))?;

    // 3. 下载 .hardy 包
    let data = client.download(name, &version)?;

    // 4. 校验和
    let checksum = archive::data_checksum(&data);

    // 5. 解压到 .hardy/packages/{name}/
    let target = dir.join(".hardy").join("packages").join(name);
    fs::create_dir_all(&target)
        .map_err(|e| format!("创建目录失败: {}", e))?;
    archive::unpack(&data, &target)?;

    // 6. 更新 lockfile
    lockfile.upsert(LockedPackage {
        name: name.to_string(),
        version: version.clone(),
        checksum,
    });

    Ok(version)
}

// =============================================================================
// hardy pack publish - 发布包
// =============================================================================
fn cmd_publish() {
    let cwd = std::env::current_dir().expect("无法获取当前目录");
    let manifest = Manifest::load(&cwd).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    println!("发布包: {}@{}", manifest.name, manifest.version);

    // 打包
    let data = archive::pack(&cwd).unwrap_or_else(|e| {
        eprintln!("打包失败: {}", e);
        std::process::exit(1);
    });

    println!("  包大小: {} bytes", data.len());

    // 上传 - 支持自定义 registry
    let url = hardyrc::get_effective_registry_url(&cwd);
    let client = RegistryClient::new(&url);
    let metadata = manifest.to_json();

    match client.publish(&manifest.name, &manifest.version, &metadata, &data) {
        Ok(msg) => {
            println!("发布成功: {}@{} (registry: {})", manifest.name, manifest.version, url);
            let _ = msg;
        }
        Err(e) => {
            eprintln!("发布失败: {}", e);
            std::process::exit(1);
        }
    }
}

// =============================================================================
// hardy pack update - 更新包
// =============================================================================
fn cmd_update(args: &[String]) {
    let cwd = std::env::current_dir().expect("无法获取当前目录");
    let manifest = Manifest::load(&cwd).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    let client = RegistryClient::new(&registry::get_registry_url());
    let mut lockfile = Lockfile::load(&cwd).unwrap_or_else(|_| Lockfile { packages: Vec::new() });

    if args.is_empty() {
        // 更新所有依赖
        println!("更新全部包...");
        let mut updated = 0;
        for (name, constraint_str) in &manifest.dependencies {
            match update_one(&client, name, constraint_str, &cwd, &mut lockfile) {
                Ok(old_ver) if old_ver.is_some() => {
                    let locked = lockfile.find(name).unwrap();
                    println!("  {} {} -> {}", name, old_ver.unwrap(), locked.version);
                    updated += 1;
                }
                Ok(_) => {}
                Err(e) => eprintln!("  {} 更新失败: {}", name, e),
            }
        }
        lockfile.save(&cwd).ok();
        if updated == 0 {
            println!("所有包已是最新");
        } else {
            println!("更新了 {} 个包", updated);
        }
    } else {
        let name = &args[0];
        let constraint_str = manifest.dependencies.iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .unwrap_or("*");

        match update_one(&client, name, constraint_str, &cwd, &mut lockfile) {
            Ok(Some(old_ver)) => {
                let locked = lockfile.find(name).unwrap();
                println!("{} {} -> {}", name, old_ver, locked.version);
                lockfile.save(&cwd).ok();
            }
            Ok(None) => println!("{} 已是最新", name),
            Err(e) => {
                eprintln!("更新失败: {}", e);
                std::process::exit(1);
            }
        }
    }
}

fn update_one(
    client: &RegistryClient,
    name: &str,
    constraint_str: &str,
    dir: &Path,
    lockfile: &mut Lockfile,
) -> Result<Option<String>, String> {
    let old_version = lockfile.find(name).map(|p| p.version.clone());

    // 查询最新版本
    let pkg_info = client.get_package(name)?;
    let constraint = resolver::VersionConstraint::parse(constraint_str)?;
    let best = constraint.best_match(&pkg_info.versions)
        .ok_or_else(|| "无匹配版本".to_string())?;

    // 如果版本相同，跳过
    if old_version.as_deref() == Some(&best) {
        return Ok(None);
    }

    // 重新安装
    install_package(client, name, constraint_str, dir, lockfile)?;
    Ok(old_version)
}

// =============================================================================
// hardy pack remove - 移除包
// =============================================================================
fn cmd_remove(args: &[String]) {
    if args.is_empty() {
        eprintln!("请指定要移除的包名称");
        eprintln!("用法: hardy pack remove <name>");
        std::process::exit(1);
    }

    let cwd = std::env::current_dir().expect("无法获取当前目录");
    let name = &args[0];

    let mut manifest = Manifest::load(&cwd).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });
    let mut lockfile = Lockfile::load(&cwd).unwrap_or_else(|_| Lockfile { packages: Vec::new() });

    // 从 manifest 移除
    if manifest.remove_dependency(name) {
        manifest.save(&cwd).ok();
    }

    // 从 lockfile 移除
    lockfile.remove(name);
    lockfile.save(&cwd).ok();

    // 删除文件
    let pkg_dir = cwd.join(".hardy").join("packages").join(name);
    if pkg_dir.exists() {
        fs::remove_dir_all(&pkg_dir).ok();
    }

    println!("已移除包: {}", name);
}

// =============================================================================
// hardy pack list - 已安装包列表
// =============================================================================
fn cmd_list() {
    let cwd = std::env::current_dir().expect("无法获取当前目录");
    let lockfile = Lockfile::load(&cwd).unwrap_or_else(|_| Lockfile { packages: Vec::new() });

    if lockfile.packages.is_empty() {
        println!("已安装包列表为空");
        println!("运行 'hardy pack install' 安装包");
        return;
    }

    println!("已安装包列表:");
    println!("{:-<50}", "");
    println!("{:<25} {:<15} {}", "包", "版本", "校验和");
    println!("{:-<50}", "");
    for pkg in &lockfile.packages {
        let checksum_short = if pkg.checksum.len() > 12 {
            format!("{}...", &pkg.checksum[..12])
        } else {
            pkg.checksum.clone()
        };
        println!("{:<25} {:<15} {}", pkg.name, pkg.version, checksum_short);
    }
    println!("{:-<50}", "");
    println!("共 {} 个包", lockfile.packages.len());
}

// =============================================================================
// hardy pack search - 搜索包
// =============================================================================
fn cmd_search(args: &[String]) {
    if args.is_empty() {
        eprintln!("hardy pack search: 请指定搜索关键词");
        eprintln!("用法: hardy pack search <query>");
        eprintln!("示例: hardy pack search motor");
        std::process::exit(1);
    }

    let query = args.join(" ");
    let url = registry::get_registry_url();
    let client = RegistryClient::new(&url);

    println!("搜索包: \"{}\"", query);
    println!("{:-<50}", "");

    match client.search(&query) {
        Ok(results) => {
            if results.is_empty() {
                println!("未找到匹配的包");
            } else {
                println!("{:<25} {:<12} {}", "包", "最新版本", "描述");
                println!("{:-<50}", "");
                for pkg in &results {
                    let desc = if pkg.description.len() > 30 {
                        format!("{}...", &pkg.description[..27])
                    } else {
                        pkg.description.clone()
                    };
                    println!("{:<25} {:<12} {}", pkg.name, pkg.latest, desc);
                }
                println!("{:-<50}", "");
                println!("共找到 {} 个包", results.len());
            }
        }
        Err(e) => {
            eprintln!("[FAIL] {}", e);
            eprintln!("提示: 确保 Registry 服务正在运行: {}", url);
        }
    }
}

// =============================================================================
// hardy pack hunt - 安装依赖并运行 (install + run)
// =============================================================================
fn cmd_hunt(args: &[String]) {
    let cwd = std::env::current_dir().expect("无法获取当前目录");

    // Step 1: Install dependencies
    let manifest = Manifest::load(&cwd).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    if !manifest.dependencies.is_empty() {
        println!("Phase 1: 安装依赖...");
        install_all(&cwd);
        println!();
    }

    // Step 2: Run the main file (or specified file)
    let target_file = if args.is_empty() {
        cwd.join(&manifest.main)
    } else {
        cwd.join(&args[0])
    };

    if !target_file.exists() {
        eprintln!("目标文件不存在: {}", target_file.display());
        std::process::exit(1);
    }

    println!("Phase 2: 运行 -> {}", target_file.display());
    println!("{:=<50}", "");

    // 调用编译器执行
    let source = fs::read_to_string(&target_file).unwrap_or_else(|e| {
        eprintln!("读取文件失败: {}", e);
        std::process::exit(1);
    });

    // 编译 + 运行
    let tokens = crate::lexer::tokenize(&source).unwrap_or_else(|e| {
        eprintln!("词法分析失败: {}", e);
        std::process::exit(1);
    });
    let ast = crate::parser::parse(&tokens).unwrap_or_else(|e| {
        eprintln!("语法分析失败: {}", e);
        std::process::exit(1);
    });
    let optimized = crate::optimizer::optimize(&ast);
    let instructions = crate::compiler::compile(&optimized).unwrap_or_else(|e| {
        eprintln!("编译失败: {}", e);
        std::process::exit(1);
    });
    let machine_code: Vec<u32> = instructions.iter().map(|i| i.encode()).collect();

    let mut engine = crate::engine::Engine::new(machine_code);
    let _ = engine.run();
}

// =============================================================================
// hardy pack lock - 生成锁定文件
// =============================================================================
fn cmd_lock() {
    let cwd = std::env::current_dir().expect("无法获取当前目录");
    let manifest = Manifest::load(&cwd).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });

    println!("生成依赖锁定文件...");

    let client = RegistryClient::new(&hardyrc::get_effective_registry_url(&cwd));
    let mut lockfile = Lockfile { packages: Vec::new() };

    for (name, constraint_str) in &manifest.dependencies {
        match client.get_package(name) {
            Ok(pkg_info) => {
                let constraint = resolver::VersionConstraint::parse(constraint_str)
                    .unwrap_or_else(|_| resolver::VersionConstraint::parse("*").unwrap());
                if let Some(version) = constraint.best_match(&pkg_info.versions) {
                    lockfile.upsert(LockedPackage {
                        name: name.clone(),
                        version,
                        checksum: "pending".to_string(),
                    });
                }
            }
            Err(e) => {
                eprintln!("  {} 解析失败: {}", name, e);
            }
        }
    }

    lockfile.save(&cwd).unwrap_or_else(|e| {
        eprintln!("写入 hardy.lock 失败: {}", e);
        std::process::exit(1);
    });

    println!("hardy.lock 已生成 ({} 个依赖)", lockfile.packages.len());
}

// =============================================================================
// hardy pack verify - 验证依赖完整性
// =============================================================================
fn cmd_verify() {
    let cwd = std::env::current_dir().expect("无法获取当前目录");
    let manifest = Manifest::load(&cwd).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1);
    });
    let lockfile = Lockfile::load(&cwd).unwrap_or_else(|_| Lockfile { packages: Vec::new() });

    println!("验证依赖完整性...");
    let mut ok = true;

    for (name, _constraint) in &manifest.dependencies {
        if let Some(locked) = lockfile.find(name) {
            let pkg_dir = cwd.join(".hardy").join("packages").join(name);
            if pkg_dir.exists() {
                println!("  {} {} ... OK", name, locked.version);
            } else {
                println!("  {} {} ... 缺失 (需要安装)", name, locked.version);
                ok = false;
            }
        } else {
            println!("  {} ... 未锁定", name);
            ok = false;
        }
    }

    if ok {
        println!("\n所有依赖验证通过");
    } else {
        println!("\n部分依赖缺失，运行 'hardy pack install' 修复");
    }
}
