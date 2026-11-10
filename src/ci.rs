//! DUANYAN CI/CD - 持续集成/持续部署
//!
//! Phase H: `duanyan ci` 命令
//! CI模式编译、版本管理、CI模板生成、产物归档

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// CI 配置
#[derive(Debug, Clone)]
pub struct CiConfig {
    pub targets: Vec<String>,
    pub lint: bool,
    pub test: bool,
    pub archive: bool,
}

/// 发布信息
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub artifacts: Vec<Artifact>,
    pub resource_report: String,
    pub timestamp: String,
}

/// 构建产物
#[derive(Debug, Clone)]
pub struct Artifact {
    pub name: String,
    pub path: String,
    pub size_bytes: u64,
    pub target: String,
    pub hash: String,
}

/// CI 构建结果
#[derive(Debug, Clone)]
pub struct CiBuildResult {
    pub success: bool,
    pub targets_built: Vec<String>,
    pub targets_failed: Vec<String>,
    pub lint_passed: bool,
    pub total_size_bytes: u64,
    pub duration_ms: u64,
    pub red_line_violations: Vec<RedLineViolation>,
}

/// 性能红线违规记录
#[derive(Debug, Clone)]
pub struct RedLineViolation {
    pub metric: String,
    pub threshold: String,
    pub actual: String,
    pub severity: String,
}

/// 性能红线阈值 (v5.1规格)
pub struct PerformanceRedLine {
    /// Flash增长上限百分比 (相对于手写C)
    pub flash_overhead_max_pct: f64,
    /// RAM增长上限百分比
    pub ram_overhead_max_pct: f64,
    /// 推理速度上限毫秒 (单次推理)
    pub inference_max_ms: f64,
    /// IRQ额外延迟上限微秒
    pub irq_overhead_max_us: f64,
}

impl Default for PerformanceRedLine {
    fn default() -> Self {
        Self {
            flash_overhead_max_pct: 20.0,  // Flash ≤ +20%
            ram_overhead_max_pct: 25.0,    // RAM ≤ +25%
            inference_max_ms: 200.0,       // 单次推理 < 200ms
            irq_overhead_max_us: 2.0,      // IRQ延迟 ≤ 2μs
        }
    }
}

impl PerformanceRedLine {
    /// 检查构建结果是否违反红线
    pub fn check(&self, result: &CiBuildResult) -> Vec<RedLineViolation> {
        let mut violations = Vec::new();

        // 检查编译时间红线 (每个目标不超过30s)
        if result.duration_ms > 30_000 * result.targets_built.len().max(1) as u64 {
            violations.push(RedLineViolation {
                metric: "build_time".into(),
                threshold: format!("{}ms per target", 30_000),
                actual: format!("{}ms total", result.duration_ms),
                severity: "warning".into(),
            });
        }

        violations
    }
}

/// 检查固件大小是否超过板子限制
pub fn check_firmware_red_line(
    flash_bytes: u64,
    ram_bytes: u64,
    board_flash_kb: u32,
    board_ram_kb: u32,
) -> Vec<RedLineViolation> {
    let mut violations = Vec::new();
    let flash_kb = (flash_bytes + 1023) / 1024;
    let ram_kb_val = (ram_bytes + 1023) / 1024;

    if flash_kb > board_flash_kb as u64 {
        violations.push(RedLineViolation {
            metric: "flash_size".into(),
            threshold: format!("{}KB (板子上限)", board_flash_kb),
            actual: format!("{}KB", flash_kb),
            severity: "error".into(),
        });
    }

    if ram_kb_val > board_ram_kb as u64 {
        violations.push(RedLineViolation {
            metric: "ram_size".into(),
            threshold: format!("{}KB (板子上限)", board_ram_kb),
            actual: format!("{}KB", ram_kb_val),
            severity: "error".into(),
        });
    }

    violations
}

impl Default for CiConfig {
    fn default() -> Self {
        Self {
            targets: vec!["esp32".into(), "arduino-avr".into(), "counpre64".into()],
            lint: true,
            test: false,
            archive: true,
        }
    }
}

/// CI 模式编译所有目标
pub fn ci_build_all(dir: &Path, config: &CiConfig) -> CiBuildResult {
    let start = std::time::Instant::now();
    let mut targets_built = Vec::new();
    let mut targets_failed = Vec::new();
    let mut total_size = 0u64;
    let lint_passed = if config.lint {
        let lint_config = crate::lint::LintConfig::load(dir);
        let result = crate::lint::lint_project(dir, &lint_config);
        result.error_count == 0
    } else {
        true
    };

    // 查找入口文件
    let entry = find_entry_file(dir);

    for target in &config.targets {
        if let Some(ref entry_path) = entry {
            let entry_name = entry_path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let build_dir = dir.join("build").join(target);
            fs::create_dir_all(&build_dir).ok();

            // 调用 DUANYAN 编译 (跨平台，不依赖 sh)
            let output = Command::new("duanyan")
                .args([
                    "build",
                    &entry_path.to_string_lossy(),
                    "-o",
                    &build_dir.join(format!("{}.hex", entry_name)).to_string_lossy(),
                    "--target",
                    target,
                ])
                .current_dir(dir)
                .output();

            match output {
                Ok(o) if o.status.success() => {
                    // 计算产物大小
                    let hex_path = build_dir.join(format!("{}.hex", entry_name));
                    if let Ok(meta) = fs::metadata(&hex_path) {
                        total_size += meta.len();
                    }
                    targets_built.push(target.clone());
                }
                _ => {
                    targets_failed.push(target.clone());
                }
            }
        } else {
            targets_failed.push(target.clone());
        }
    }

    let mut result = CiBuildResult {
        success: targets_failed.is_empty() && lint_passed,
        targets_built,
        targets_failed,
        lint_passed,
        total_size_bytes: total_size,
        duration_ms: start.elapsed().as_millis() as u64,
        red_line_violations: Vec::new(),
    };

    // 性能红线检查
    let red_line = PerformanceRedLine::default();
    let violations = red_line.check(&result);
    if !violations.is_empty() {
        for v in &violations {
            println!("❌ 红线违规: {} = {} (阈值: {}) [{}]",
                v.metric, v.actual, v.threshold, v.severity);
        }
        // 有error级别违规时标记构建失败
        if violations.iter().any(|v| v.severity == "error") {
            result.success = false;
        }
    }
    result.red_line_violations = violations;

    result
}

/// 版本发布
pub fn ci_release(dir: &Path, version: &str) -> Result<ReleaseInfo, String> {
    let build_dir = dir.join("build");
    if !build_dir.exists() {
        return Err("build/ 目录不存在，请先运行 'duanyan ci build --all'".to_string());
    }

    let release_dir = dir.join("release").join(version);
    fs::create_dir_all(&release_dir)
        .map_err(|e| format!("创建 release 目录失败: {}", e))?;

    let mut artifacts = Vec::new();
    let mut total_size = 0u64;

    // 收集构建产物
    if let Ok(entries) = fs::read_dir(&build_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let target = path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if let Ok(files) = fs::read_dir(&path) {
                    for file in files.flatten() {
                        let file_path = file.path();
                        if let Some(ext) = file_path.extension() {
                            let ext = ext.to_string_lossy();
                            if ext == "hex" || ext == "bin" || ext == "rom" || ext == "map" {
                                let name = file_path.file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();

                                let dest = release_dir.join(&name);
                                fs::copy(&file_path, &dest).ok();

                                let size = file.metadata().map(|m| m.len()).unwrap_or(0);
                                total_size += size;

                                let hash = simple_hash(&file_path);

                                artifacts.push(Artifact {
                                    name,
                                    path: dest.to_string_lossy().to_string(),
                                    size_bytes: size,
                                    target: target.clone(),
                                    hash,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    let resource_report = format!(
        "Release v{}\nArtifacts: {}\nTotal size: {} KB\n",
        version,
        artifacts.len(),
        total_size / 1024
    );

    // 写入 release 信息
    let info_path = release_dir.join("release-info.txt");
    fs::write(&info_path, &resource_report)
        .map_err(|e| format!("写入 release 信息失败: {}", e))?;

    let timestamp = chrono_now();

    Ok(ReleaseInfo {
        version: version.to_string(),
        artifacts,
        resource_report,
        timestamp,
    })
}

/// 生成 GitHub Actions CI 模板
pub fn ci_generate_github_action(dir: &Path) -> Result<(), String> {
    let workflows_dir = dir.join(".github").join("workflows");
    fs::create_dir_all(&workflows_dir)
        .map_err(|e| format!("创建 workflows 目录失败: {}", e))?;

    let content = r#"name: DUANYAN CI

on:
  push:
    branches: [ main, develop ]
  pull_request:
    branches: [ main ]

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install DUANYAN
        run: |
          cargo install --path .
          # 或从 release 下载:
          # curl -L https://github.com/openhardy/hardyscript/releases/latest/download/hardy-linux -o /usr/local/bin/duanyan
          # chmod +x /usr/local/bin/duanyan

      - name: Lint
        run: duanyan lint

      - name: Build All Targets
        run: duanyan ci build --all

      - name: Upload Artifacts
        uses: actions/upload-artifact@v4
        with:
          name: firmware
          path: build/

  release:
    needs: build
    runs-on: ubuntu-latest
    if: startsWith(github.ref, 'refs/tags/v')
    steps:
      - uses: actions/checkout@v4

      - name: Install DUANYAN
        run: cargo install --path .

      - name: Release
        run: duanyan ci release --version ${{ github.ref_name }}

      - name: Upload Release
        uses: softprops/action-gh-release@v2
        with:
          files: release/${{ github.ref_name }}/*
"#;

    fs::write(workflows_dir.join("duanyan-ci.yml"), content)
        .map_err(|e| format!("写入 CI 模板失败: {}", e))?;

    println!("GitHub Actions CI 模板已生成: .github/workflows/duanyan-ci.yml");
    Ok(())
}

/// 生成 GitLab CI 模板
pub fn ci_generate_gitlab_ci(dir: &Path) -> Result<(), String> {
    let content = r#"# DUANYAN GitLab CI 配置
# 自动生成 by duanyan ci template --gitlab

stages:
  - lint
  - build
  - release

variables:
  DUANYAN_VERSION: "latest"

lint:
  stage: lint
  script:
    - duanyan lint
  rules:
    - if: '$CI_PIPELINE_SOURCE == "merge_request_event"'
    - if: '$CI_COMMIT_BRANCH == "main"'

build:
  stage: build
  script:
    - duanyan ci build --all
  artifacts:
    paths:
      - build/
    expire_in: 1 week

release:
  stage: release
  script:
    - duanyan ci release --version $CI_COMMIT_TAG
  artifacts:
    paths:
      - release/
  rules:
    - if: '$CI_COMMIT_TAG'
"#;

    fs::write(dir.join(".gitlab-ci.yml"), content)
        .map_err(|e| format!("写入 GitLab CI 模板失败: {}", e))?;

    println!("GitLab CI 模板已生成: .gitlab-ci.yml");
    Ok(())
}

/// 归档构建产物 (纯Rust实现，跨平台)
pub fn ci_archive(dir: &Path, version: &str) -> Result<String, String> {
    let release_dir = dir.join("release").join(version);
    if !release_dir.exists() {
        return Err("release 目录不存在，请先运行 'duanyan ci release'".to_string());
    }

    let _archive_name = format!("duanyan-release-{}.tar.gz", version);

    use std::io::Write;
    let mut total_files = 0u32;
    let mut total_bytes = 0u64;

    // 收集所有需要归档的文件
    let mut files_to_archive: Vec<(String, Vec<u8>)> = Vec::new();
    collect_archive_files(&release_dir, &release_dir, &mut files_to_archive);

    // 简易tar格式写入
    let mut tar_data: Vec<u8> = Vec::new();
    for (rel_path, data) in &files_to_archive {
        let mut header = [0u8; 512];
        let name_bytes = rel_path.as_bytes();
        let copy_len = name_bytes.len().min(100);
        header[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        // mode: 0644
        header[100..107].copy_from_slice(b"0000644");
        header[107] = 0;
        // size in octal
        let size_str = format!("{:011o}", data.len());
        header[124..135].copy_from_slice(size_str.as_bytes());
        header[135] = 0;
        // type: regular file
        header[156] = b'0';
        // magic
        header[257..262].copy_from_slice(b"ustar");
        header[263] = 0;
        header[264] = 0;
        // checksum placeholder
        header[148..156].copy_from_slice(b"        ");
        // compute checksum
        let mut cksum: u32 = 0;
        for &b in &header { cksum += b as u32; }
        let cksum_str = format!("{:06o}", cksum);
        header[148..154].copy_from_slice(cksum_str.as_bytes());
        header[154] = 0;
        header[155] = b' ';

        tar_data.extend_from_slice(&header);
        tar_data.extend_from_slice(data);
        // pad to 512 boundary
        let pad = (512 - (data.len() % 512)) % 512;
        tar_data.extend(std::iter::repeat(0u8).take(pad));
        total_files += 1;
        total_bytes += data.len() as u64;
    }
    // End-of-archive: two 512-byte blocks of zeros
    tar_data.extend(std::iter::repeat(0u8).take(1024));

    // gzip压缩 (简易: 直接写tar，因为flate2可能不在deps中)
    // 这里用 .tar 替代 .tar.gz，避免外部依赖
    let archive_path = dir.join("release").join(format!("duanyan-release-{}.tar", version));
    let mut file = fs::File::create(&archive_path)
        .map_err(|e| format!("创建归档文件失败: {}", e))?;
    file.write_all(&tar_data)
        .map_err(|e| format!("写入归档数据失败: {}", e))?;

    let size = fs::metadata(&archive_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(format!("归档完成: {} ({} 文件, {} KB)", archive_path.display(), total_files, size / 1024))
}

/// 递归收集归档文件
fn collect_archive_files(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Ok(data) = fs::read(&path) {
                    let rel = path.strip_prefix(base).unwrap_or(&path);
                    out.push((rel.to_string_lossy().to_string(), data));
                }
            } else if path.is_dir() {
                collect_archive_files(base, &path, out);
            }
        }
    }
}

// =========================================================================
// 辅助函数
// =========================================================================

fn find_entry_file(dir: &Path) -> Option<PathBuf> {
    let candidates = [
        "main.js", "index.js", "src/main.js", "src/index.js",
        "main.ts", "index.ts", "src/main.ts", "src/index.ts",
    ];
    for name in &candidates {
        let path = dir.join(name);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn simple_hash(path: &Path) -> String {
    if let Ok(data) = fs::read(path) {
        // Simple DJB2 hash (not cryptographic, but sufficient for identification)
        let mut hash: u64 = 5381;
        for &byte in &data {
            hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
        }
        format!("{:016x}", hash)
    } else {
        "unknown".to_string()
    }
}

fn chrono_now() -> String {
    // 纯Rust实现，不依赖外部 date 命令
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 简易UTC时间戳转换
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // 从1970-01-01计算年月日
    let mut y = 1970i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap_year(y) { 366 } else { 365 };
        if remaining_days < days_in_year { break; }
        remaining_days -= days_in_year;
        y += 1;
    }
    let month_days = if is_leap_year(y) {
        [31,29,31,30,31,30,31,31,30,31,30,31]
    } else {
        [31,28,31,30,31,30,31,31,30,31,30,31]
    };
    let mut m = 1u32;
    for &md in &month_days {
        if remaining_days < md { break; }
        remaining_days -= md;
        m += 1;
    }
    let d = remaining_days + 1;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", y, m, d, hours, minutes, seconds)
}

fn is_leap_year(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ci_config_default() {
        let config = CiConfig::default();
        assert!(config.lint);
        assert!(config.archive);
        assert_eq!(config.targets.len(), 3);
    }

    #[test]
    fn test_github_action_template() {
        let dir = std::env::temp_dir().join("duanyan_ci_test_gh");
        fs::create_dir_all(&dir).ok();
        let result = ci_generate_github_action(&dir);
        assert!(result.is_ok());
        assert!(dir.join(".github/workflows/duanyan-ci.yml").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_gitlab_ci_template() {
        let dir = std::env::temp_dir().join("duanyan_ci_test_gl");
        fs::create_dir_all(&dir).ok();
        let result = ci_generate_gitlab_ci(&dir);
        assert!(result.is_ok());
        assert!(dir.join(".gitlab-ci.yml").exists());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_simple_hash() {
        // Create a temp file and hash it
        let dir = std::env::temp_dir().join("duanyan_hash_test");
        fs::create_dir_all(&dir).ok();
        let path = dir.join("test.txt");
        fs::write(&path, "hello world").ok();
        let hash = simple_hash(&path);
        assert_ne!(hash, "unknown");
        assert_eq!(hash.len(), 16);
        fs::remove_dir_all(&dir).ok();
    }
}
