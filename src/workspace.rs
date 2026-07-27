//! SNARjs Workspace - 多项目管理
//!
//! Phase H: `snar workspace` 命令
//! 支持 monorepo 多子项目统一管理、拓扑排序编译

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Workspace 配置
#[derive(Debug, Clone)]
pub struct WorkspaceConfig {
    pub name: String,
    pub version: String,
    pub packages: Vec<PackageRef>,
    pub shared_libs: Vec<String>,
    pub default_target: String,
    pub build_order: Vec<String>,
}

/// 子项目引用
#[derive(Debug, Clone)]
pub struct PackageRef {
    pub name: String,
    pub path: String,
    pub target: Option<String>,
    pub dependencies: Vec<String>,
}

/// Workspace 状态
#[derive(Debug, Clone)]
pub struct PackageStatus {
    pub name: String,
    pub path: String,
    pub target: String,
    pub has_config: bool,
    pub has_entry: bool,
    pub last_build: Option<String>,
}

impl WorkspaceConfig {
    /// 创建默认配置
    pub fn default_with_name(name: &str) -> Self {
        Self {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            packages: Vec::new(),
            shared_libs: Vec::new(),
            default_target: "esp32".to_string(),
            build_order: Vec::new(),
        }
    }

    /// 从 JSON 文件加载
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("snar.workspace.json");
        if !path.exists() {
            return Err("未找到 snar.workspace.json，请先运行 'snar workspace init'".to_string());
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| format!("读取配置失败: {}", e))?;
        Self::from_json(&raw)
    }

    /// 从 JSON 字符串解析
    pub fn from_json(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if !trimmed.starts_with('{') {
            return Err("无效的 JSON".to_string());
        }

        let mut ws = Self::default_with_name("workspace");

        // 简易 JSON 解析
        let name = extract_string_field(trimmed, "name");
        if let Some(v) = name { ws.name = v; }

        let ver = extract_string_field(trimmed, "version");
        if let Some(v) = ver { ws.version = v; }

        let dt = extract_string_field(trimmed, "defaultTarget");
        if let Some(v) = dt { ws.default_target = v; }

        // 解析 packages 数组
        if let Some(pkgs_str) = extract_array_field(trimmed, "packages") {
            ws.packages = parse_packages_array(&pkgs_str)?;
        }

        // 解析 sharedLibs
        if let Some(libs_str) = extract_array_field(trimmed, "sharedLibs") {
            ws.shared_libs = parse_string_array(&libs_str);
        }

        Ok(ws)
    }

    /// 序列化为 JSON
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"name\": \"{}\",\n", self.name));
        out.push_str(&format!("  \"version\": \"{}\",\n", self.version));
        out.push_str(&format!("  \"defaultTarget\": \"{}\",\n", self.default_target));

        out.push_str("  \"packages\": [\n");
        for (i, pkg) in self.packages.iter().enumerate() {
            out.push_str("    {");
            out.push_str(&format!("\"name\": \"{}\", ", pkg.name));
            out.push_str(&format!("\"path\": \"{}\"", pkg.path));
            if let Some(t) = &pkg.target {
                out.push_str(&format!(", \"target\": \"{}\"", t));
            }
            if !pkg.dependencies.is_empty() {
                let deps = pkg.dependencies.iter()
                    .map(|d| format!("\"{}\"", d))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(", \"dependencies\": [{}]", deps));
            }
            out.push('}');
            if i < self.packages.len() - 1 { out.push(','); }
            out.push('\n');
        }
        out.push_str("  ],\n");

        let libs = self.shared_libs.iter()
            .map(|l| format!("\"{}\"", l))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("  \"sharedLibs\": [{}]\n", libs));
        out.push_str("}\n");
        out
    }

    /// 保存到文件
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join("snar.workspace.json");
        fs::write(&path, self.to_json())
            .map_err(|e| format!("写入配置失败: {}", e))
    }

    /// 添加子项目
    pub fn add_package(&mut self, pkg: PackageRef) {
        self.packages.push(pkg);
    }

    /// 解析依赖图，检测循环依赖，返回拓扑排序
    pub fn resolve_build_order(&mut self) -> Result<Vec<String>, String> {
        let name_to_idx: HashMap<String, usize> = self.packages.iter()
            .enumerate()
            .map(|(i, p)| (p.name.clone(), i))
            .collect();

        let n = self.packages.len();
        let mut in_degree = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];

        for (i, pkg) in self.packages.iter().enumerate() {
            for dep in &pkg.dependencies {
                if let Some(&j) = name_to_idx.get(dep) {
                    adj[j].push(i);
                    in_degree[i] += 1;
                }
            }
        }

        // Kahn's algorithm
        let mut queue: Vec<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::new();

        while let Some(node) = queue.pop() {
            order.push(self.packages[node].name.clone());
            for &next in &adj[node] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push(next);
                }
            }
        }

        if order.len() != n {
            return Err("检测到循环依赖! 请检查 packages 之间的依赖关系".to_string());
        }

        self.build_order = order.clone();
        Ok(order)
    }

    /// 获取子项目的完整路径
    pub fn package_path(&self, root: &Path, pkg: &PackageRef) -> PathBuf {
        root.join(&pkg.path)
    }

    /// 获取子项目的编译目标
    pub fn package_target<'a>(&'a self, pkg: &'a PackageRef) -> &'a str {
        pkg.target.as_deref().unwrap_or(&self.default_target)
    }

    /// 获取所有子项目状态
    pub fn status(&self, root: &Path) -> Vec<PackageStatus> {
        self.packages.iter().map(|pkg| {
            let pkg_dir = root.join(&pkg.path);
            let has_config = pkg_dir.join("snar.json").exists()
                || pkg_dir.join("snar.config.json").exists();
            let has_entry = pkg_dir.join("main.js").exists()
                || pkg_dir.join("index.js").exists()
                || pkg_dir.join("src/main.js").exists();
            let target = pkg.target.as_deref()
                .unwrap_or(&self.default_target)
                .to_string();
            let last_build = {
                let build_dir = pkg_dir.join("build");
                if build_dir.exists() {
                    Some("已编译".to_string())
                } else {
                    None
                }
            };
            PackageStatus {
                name: pkg.name.clone(),
                path: pkg.path.clone(),
                target,
                has_config,
                has_entry,
                last_build,
            }
        }).collect()
    }
}

/// 初始化 workspace
pub fn workspace_init(dir: &Path, name: &str, monorepo: bool) -> Result<WorkspaceConfig, String> {
    let ws_file = dir.join("snar.workspace.json");
    if ws_file.exists() {
        return Err("snar.workspace.json 已存在".to_string());
    }

    let mut ws = WorkspaceConfig::default_with_name(name);

    if monorepo {
        // 创建 packages/ 目录结构
        let pkgs_dir = dir.join("packages");
        fs::create_dir_all(&pkgs_dir).map_err(|e| format!("创建目录失败: {}", e))?;

        // 创建共享库
        let shared_dir = pkgs_dir.join("shared");
        fs::create_dir_all(&shared_dir).map_err(|e| format!("创建目录失败: {}", e))?;
        if !shared_dir.join("index.js").exists() {
            fs::write(shared_dir.join("index.js"), "// 共享库入口\nexport {};\n")
                .map_err(|e| format!("写入文件失败: {}", e))?;
        }
        ws.shared_libs.push("packages/shared".to_string());
    }

    // 创建 .snar/ 目录
    let snar_dir = dir.join(".snar");
    fs::create_dir_all(&snar_dir).map_err(|e| format!("创建目录失败: {}", e))?;

    // 创建 boards.json
    let boards_json = "{\n  \"boards\": []\n}\n";
    fs::write(snar_dir.join("boards.json"), boards_json)
        .map_err(|e| format!("写入 boards.json 失败: {}", e))?;

    // 创建 lint.json
    let lint_json = "{\n  \"rules\": {},\n  \"maxRamKb\": null,\n  \"maxFlashKb\": null\n}\n";
    fs::write(snar_dir.join("lint.json"), lint_json)
        .map_err(|e| format!("写入 lint.json 失败: {}", e))?;

    // 创建 .gitignore
    let gitignore = dir.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, ".snar/\nsnar.lock\nbuild/\ntarget/\n")
            .map_err(|e| format!("写入 .gitignore 失败: {}", e))?;
    }

    ws.save(dir)?;
    Ok(ws)
}

// =========================================================================
// JSON 辅助解析函数
// =========================================================================

fn extract_string_field(json: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\"", field);
    if let Some(pos) = json.find(&pattern) {
        let rest = &json[pos + pattern.len()..];
        let rest = rest.trim_start();
        if rest.starts_with(':') {
            let rest = rest[1..].trim_start();
            if rest.starts_with('"') {
                let end = rest[1..].find('"').map(|i| i + 1)?;
                return Some(rest[1..end].to_string());
            }
        }
    }
    None
}

fn extract_array_field(json: &str, field: &str) -> Option<String> {
    let pattern = format!("\"{}\"", field);
    if let Some(pos) = json.find(&pattern) {
        let rest = &json[pos + pattern.len()..];
        let rest = rest.trim_start();
        if rest.starts_with(':') {
            let rest = rest[1..].trim_start();
            if rest.starts_with('[') {
                let mut depth = 0;
                for (i, ch) in rest.char_indices() {
                    if ch == '[' { depth += 1; }
                    else if ch == ']' {
                        depth -= 1;
                        if depth == 0 {
                            return Some(rest[..=i].to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

fn parse_packages_array(raw: &str) -> Result<Vec<PackageRef>, String> {
    let mut pkgs = Vec::new();
    let inner = raw.trim();
    if inner.len() < 2 { return Ok(pkgs); }
    let inner = &inner[1..inner.len() - 1]; // strip [ ]

    let mut depth = 0;
    let mut start = None;

    for (i, ch) in inner.char_indices() {
        if ch == '{' {
            if depth == 0 { start = Some(i); }
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                if let Some(s) = start {
                    let obj = &inner[s..=i];
                    let name = extract_string_field(obj, "name").unwrap_or_default();
                    let path = extract_string_field(obj, "path").unwrap_or_default();
                    let target = extract_string_field(obj, "target");

                    let deps = if let Some(dep_arr) = extract_array_field(obj, "dependencies") {
                        parse_string_array(&dep_arr)
                    } else {
                        Vec::new()
                    };

                    pkgs.push(PackageRef { name, path, target, dependencies: deps });
                }
            }
        }
    }
    Ok(pkgs)
}

fn parse_string_array(raw: &str) -> Vec<String> {
    let mut result = Vec::new();
    let inner = raw.trim();
    if inner.len() < 2 { return result; }
    let inner = &inner[1..inner.len() - 1]; // strip [ ]

    let mut in_str = false;
    let mut current = String::new();
    for ch in inner.chars() {
        if ch == '"' {
            if in_str {
                result.push(current.clone());
                current.clear();
            }
            in_str = !in_str;
        } else if in_str {
            current.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workspace_config_roundtrip() {
        let mut ws = WorkspaceConfig::default_with_name("test-ws");
        ws.add_package(PackageRef {
            name: "main".to_string(),
            path: "packages/main".to_string(),
            target: Some("esp32".to_string()),
            dependencies: vec!["shared".to_string()],
        });
        ws.add_package(PackageRef {
            name: "shared".to_string(),
            path: "packages/shared".to_string(),
            target: None,
            dependencies: Vec::new(),
        });

        let json = ws.to_json();
        let ws2 = WorkspaceConfig::from_json(&json).unwrap();
        assert_eq!(ws2.name, "test-ws");
        assert_eq!(ws2.packages.len(), 2);
        assert_eq!(ws2.packages[0].name, "main");
    }

    #[test]
    fn test_build_order() {
        let mut ws = WorkspaceConfig::default_with_name("test");
        ws.add_package(PackageRef {
            name: "app".to_string(),
            path: "packages/app".to_string(),
            target: None,
            dependencies: vec!["lib".to_string()],
        });
        ws.add_package(PackageRef {
            name: "lib".to_string(),
            path: "packages/lib".to_string(),
            target: None,
            dependencies: Vec::new(),
        });
        let order = ws.resolve_build_order().unwrap();
        assert_eq!(order[0], "lib");
        assert_eq!(order[1], "app");
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut ws = WorkspaceConfig::default_with_name("test");
        ws.add_package(PackageRef {
            name: "a".to_string(), path: "a".to_string(), target: None,
            dependencies: vec!["b".to_string()],
        });
        ws.add_package(PackageRef {
            name: "b".to_string(), path: "b".to_string(), target: None,
            dependencies: vec!["a".to_string()],
        });
        assert!(ws.resolve_build_order().is_err());
    }
}
