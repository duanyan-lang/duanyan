//! SNARjs Git Operations
//!
//! Phase H: Git 集成模块
//! 通过 CLI 封装 git 命令，提供 IDE 和 CLI 可用的 Git 操作接口

use std::path::Path;
use std::process::Command;

/// 文件变更类型
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeType {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Unknown,
}

impl ChangeType {
    pub fn from_char(c: char) -> Self {
        match c {
            'M' => Self::Modified,
            'A' => Self::Added,
            'D' => Self::Deleted,
            'R' => Self::Renamed,
            'C' => Self::Copied,
            '?' => Self::Untracked,
            _ => Self::Unknown,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Modified => "Modified",
            Self::Added => "Added",
            Self::Deleted => "Deleted",
            Self::Renamed => "Renamed",
            Self::Copied => "Copied",
            Self::Untracked => "Untracked",
            Self::Unknown => "Unknown",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Modified => "M",
            Self::Added => "A",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::Untracked => "?",
            Self::Unknown => " ",
        }
    }
}

/// 文件变更
#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub status: ChangeType,
}

/// Git 仓库状态
#[derive(Debug, Clone)]
pub struct GitStatus {
    pub branch: String,
    pub staged: Vec<FileChange>,
    pub unstaged: Vec<FileChange>,
    pub untracked: Vec<String>,
    pub ahead: u32,
    pub behind: u32,
}

/// Git 提交记录
#[derive(Debug, Clone)]
pub struct GitLogEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

/// Diff 行类型
#[derive(Debug, Clone)]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
    Header,
}

/// Diff 行
#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
}

/// Diff hunk
#[derive(Debug, Clone)]
pub struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub lines: Vec<DiffLine>,
}

/// 文件 diff
#[derive(Debug, Clone)]
pub struct FileDiff {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
}

/// 分支信息
#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
}

// =========================================================================
// 核心 Git 操作
// =========================================================================

fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("执行 git 失败: {}", e))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(stderr.trim().to_string())
    }
}

fn run_git_quiet(dir: &Path, args: &[&str]) -> Result<(), String> {
    run_git(dir, args)?;
    Ok(())
}

/// 获取仓库状态
pub fn git_status(dir: &Path) -> Result<GitStatus, String> {
    // 获取当前分支
    let branch = run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "unknown".to_string())
        .trim().to_string();

    // 获取 porcelain v1 状态
    let raw = run_git(dir, &["status", "--porcelain=v1", "-b"])?;

    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut ahead = 0u32;
    let mut behind = 0u32;

    for line in raw.lines() {
        if line.starts_with("##") {
            // branch info: ## main...origin/main [ahead 2, behind 1]
            if let Some(bracket) = line.find('[') {
                let info = &line[bracket + 1..line.len() - 1];
                for part in info.split(',') {
                    let part = part.trim();
                    if part.starts_with("ahead") {
                        ahead = part.split_whitespace().last()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                    } else if part.starts_with("behind") {
                        behind = part.split_whitespace().last()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);
                    }
                }
            }
            continue;
        }

        if line.len() < 3 { continue; }
        let chars: Vec<char> = line.chars().collect();
        let index_status = chars[0];
        let work_status = chars[1];
        let path = line[3..].to_string();

        if index_status == '?' && work_status == '?' {
            untracked.push(path);
        } else {
            if index_status != ' ' && index_status != '?' {
                staged.push(FileChange {
                    path: path.clone(),
                    status: ChangeType::from_char(index_status),
                });
            }
            if work_status != ' ' && work_status != '?' {
                unstaged.push(FileChange {
                    path,
                    status: ChangeType::from_char(work_status),
                });
            }
        }
    }

    Ok(GitStatus { branch, staged, unstaged, untracked, ahead, behind })
}

/// 获取提交历史
pub fn git_log(dir: &Path, n: usize) -> Result<Vec<GitLogEntry>, String> {
    let fmt_str = "--format=%H%n%an%n%aI%n%s";
    let n_str = format!("-{}", n);
    let raw = run_git(dir, &["log", fmt_str, n_str.as_str(), "--no-decorate"])?;

    let lines: Vec<&str> = raw.lines().collect();
    let mut entries = Vec::new();
    let mut i = 0;
    while i + 3 < lines.len() {
        entries.push(GitLogEntry {
            hash: lines[i].to_string(),
            author: lines[i + 1].to_string(),
            date: lines[i + 2].to_string(),
            message: lines[i + 3].to_string(),
        });
        i += 4;
    }
    Ok(entries)
}

/// 获取文件 diff
pub fn git_diff(dir: &Path, file: Option<&str>) -> Result<Vec<FileDiff>, String> {
    let mut args = vec!["diff", "--unified=3"];
    if let Some(f) = file { args.push("--"); args.push(f); }
    let raw = run_git(dir, &args)?;
    Ok(parse_diff_output(&raw))
}

/// 获取暂存区 diff
pub fn git_staged_diff(dir: &Path) -> Result<Vec<FileDiff>, String> {
    let raw = run_git(dir, &["diff", "--cached", "--unified=3"])?;
    Ok(parse_diff_output(&raw))
}

/// 暂存文件
pub fn git_add(dir: &Path, files: &[String]) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["add"];
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend(file_refs);
    run_git_quiet(dir, &args)
}

/// 暂存所有文件
pub fn git_add_all(dir: &Path) -> Result<(), String> {
    run_git_quiet(dir, &["add", "-A"])
}

/// 取消暂存
pub fn git_unstage(dir: &Path, files: &[String]) -> Result<(), String> {
    let mut args: Vec<&str> = vec!["reset", "HEAD", "--"];
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    args.extend(file_refs);
    run_git_quiet(dir, &args)
}

/// 提交
pub fn git_commit(dir: &Path, message: &str) -> Result<String, String> {
    run_git(dir, &["commit", "-m", message])
}

/// 推送
pub fn git_push(dir: &Path, remote: Option<&str>, branch: Option<&str>) -> Result<String, String> {
    let mut args = vec!["push"];
    if let Some(r) = remote { args.push(r); }
    if let Some(b) = branch { args.push(b); }
    run_git(dir, &args)
}

/// 拉取
pub fn git_pull(dir: &Path) -> Result<String, String> {
    run_git(dir, &["pull"])
}

/// 获取分支列表
pub fn git_branches(dir: &Path) -> Result<Vec<BranchInfo>, String> {
    let raw = run_git(dir, &["branch", "-a", "--format=%(refname:short) %(objectname:short)"])?;
    let current = run_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_default()
        .trim().to_string();

    let mut branches = Vec::new();
    for line in raw.lines() {
        let name = line.split_whitespace().next().unwrap_or("").to_string();
        if name.is_empty() { continue; }
        let is_remote = name.starts_with("origin/") || name.starts_with("remotes/");
        branches.push(BranchInfo {
            is_current: name == current,
            name,
            is_remote,
        });
    }
    Ok(branches)
}

/// 切换分支
pub fn git_checkout(dir: &Path, branch: &str) -> Result<String, String> {
    run_git(dir, &["checkout", branch])
}

/// 创建并切换分支
pub fn git_create_branch(dir: &Path, name: &str) -> Result<String, String> {
    run_git(dir, &["checkout", "-b", name])
}

/// 初始化仓库
pub fn git_init(dir: &Path) -> Result<String, String> {
    run_git(dir, &["init"])
}

/// 生成 .gitignore 内容
pub fn git_generate_gitignore(board: &str) -> String {
    let mut content = String::new();
    content.push_str("# SNARjs 项目 .gitignore\n");
    content.push_str("# 自动生成 by snar git hooks install\n\n");

    content.push_str("# 构建产物\n");
    content.push_str("build/\n*.hex\n*.bin\n*.rom\n*.asm\n*.map\n\n");

    content.push_str("# SNAR 工具链\n");
    content.push_str(".snar/\nsnar.lock\n\n");

    content.push_str("# IDE\n");
    content.push_str(".vscode/\n.idea/\n*.swp\n*.swo\n\n");

    content.push_str("# OS\n");
    content.push_str(".DS_Store\nThumbs.db\n\n");

    // 板子特定的忽略规则
    match board {
        "esp32" | "ESP32" => {
            content.push_str("# ESP32 特有\n");
            content.push_str("sdkconfig.old\n");
            content.push_str("CMakeLists.txt.user\n");
        }
        "arduino-avr" | "arduino-arm" => {
            content.push_str("# Arduino 特有\n");
            content.push_str("*.elf\n");
        }
        "stm32" => {
            content.push_str("# STM32 特有\n");
            content.push_str("*.elf\n*.d\n");
            content.push_str("Debug/\nRelease/\n");
        }
        _ => {}
    }

    content
}

/// 安装 Git Hooks (pre-commit 自动跑 snar lint)
pub fn git_install_hooks(dir: &Path) -> Result<(), String> {
    let hooks_dir = dir.join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|e| format!("创建 hooks 目录失败: {}", e))?;

    let hook_content = r#"#!/bin/sh
# SNARjs pre-commit hook
# 自动运行 snar lint 检查

echo "Running snar lint..."
if command -v snar &> /dev/null; then
    snar lint
    if [ $? -ne 0 ]; then
        echo "Lint 检查失败! 请修复错误后再提交。"
        echo "使用 'git commit --no-verify' 跳过检查。"
        exit 1
    fi
    echo "Lint 检查通过"
else
    echo "Warning: snar 命令未找到，跳过 lint 检查"
fi
"#;

    let hook_path = hooks_dir.join("pre-commit");
    std::fs::write(&hook_path, hook_content)
        .map_err(|e| format!("写入 pre-commit hook 失败: {}", e))?;

    // 设置可执行权限 (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)
            .map_err(|e| format!("设置权限失败: {}", e))?;
    }

    Ok(())
}

/// 检查目录是否是 git 仓库
pub fn is_git_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

// =========================================================================
// Diff 解析
// =========================================================================

fn parse_diff_output(raw: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut current_file: Option<String> = None;
    let mut current_hunks: Vec<DiffHunk> = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line = 0u32;
    let mut new_line = 0u32;

    for line in raw.lines() {
        if line.starts_with("diff --git") {
            // 保存上一个文件
            if let Some(ref hunk) = current_hunk {
                current_hunks.push(hunk.clone());
            }
            if let Some(path) = current_file.take() {
                files.push(FileDiff { path, hunks: current_hunks.clone() });
            }
            current_hunks.clear();
            current_hunk = None;
            continue;
        }

        if line.starts_with("---") || line.starts_with("+++") {
            if line.starts_with("+++ ") && !line.starts_with("+++ /dev/null") {
                let path = line[4..].trim_start_matches("b/").to_string();
                current_file = Some(path);
            }
            continue;
        }

        if line.starts_with("@@") {
            if let Some(ref hunk) = current_hunk {
                current_hunks.push(hunk.clone());
            }
            // Parse @@ -old_start,old_count +new_start,new_count @@
            let parts: Vec<&str> = line.split(" @@").collect();
            if parts.len() >= 2 {
                let range = parts[0].trim_start_matches("@@ ").trim();
                let (os, oc, ns, nc) = parse_hunk_range(range);
                old_line = os;
                new_line = ns;
                current_hunk = Some(DiffHunk {
                    old_start: os,
                    old_count: oc,
                    new_start: ns,
                    new_count: nc,
                    lines: Vec::new(),
                });
            }
            continue;
        }

        if let Some(ref mut hunk) = current_hunk {
            if line.starts_with('+') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    content: line[1..].to_string(),
                    old_lineno: None,
                    new_lineno: Some(new_line),
                });
                new_line += 1;
            } else if line.starts_with('-') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    content: line[1..].to_string(),
                    old_lineno: Some(old_line),
                    new_lineno: None,
                });
                old_line += 1;
            } else if line.starts_with(' ') || line.is_empty() {
                let content = if line.starts_with(' ') { line[1..].to_string() } else { String::new() };
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    content,
                    old_lineno: Some(old_line),
                    new_lineno: Some(new_line),
                });
                old_line += 1;
                new_line += 1;
            }
        }
    }

    // 保存最后一个文件
    if let Some(ref hunk) = current_hunk {
        current_hunks.push(hunk.clone());
    }
    if let Some(path) = current_file {
        files.push(FileDiff { path, hunks: current_hunks });
    }

    files
}

fn parse_hunk_range(range: &str) -> (u32, u32, u32, u32) {
    let parts: Vec<&str> = range.split_whitespace().collect();
    let mut old_start = 1u32;
    let mut old_count = 1u32;
    let mut new_start = 1u32;
    let mut new_count = 1u32;

    for part in parts {
        if part.starts_with('-') {
            let nums: Vec<&str> = part[1..].split(',').collect();
            old_start = nums.first().and_then(|s| s.parse().ok()).unwrap_or(1);
            old_count = nums.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
        } else if part.starts_with('+') {
            let nums: Vec<&str> = part[1..].split(',').collect();
            new_start = nums.first().and_then(|s| s.parse().ok()).unwrap_or(1);
            new_count = nums.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
        }
    }

    (old_start, old_count, new_start, new_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_type() {
        assert_eq!(ChangeType::from_char('M'), ChangeType::Modified);
        assert_eq!(ChangeType::from_char('A'), ChangeType::Added);
        assert_eq!(ChangeType::from_char('D'), ChangeType::Deleted);
        assert_eq!(ChangeType::from_char('?'), ChangeType::Untracked);
    }

    #[test]
    fn test_parse_hunk_range() {
        let (os, oc, ns, nc) = parse_hunk_range("-10,5 +20,8");
        assert_eq!(os, 10);
        assert_eq!(oc, 5);
        assert_eq!(ns, 20);
        assert_eq!(nc, 8);
    }

    #[test]
    fn test_generate_gitignore() {
        let gi = git_generate_gitignore("esp32");
        assert!(gi.contains("build/"));
        assert!(gi.contains(".snar/"));
        assert!(gi.contains("ESP32"));
    }
}
