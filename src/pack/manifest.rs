//! duanyan.json 项目清单管理
//!
//! 读写项目配置文件，管理依赖声明

use std::fs;
use std::path::Path;
use super::json::JsonValue;

/// 项目清单 (duanyan.json)
#[derive(Debug, Clone)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub main: String,
    pub author: String,
    pub dependencies: Vec<(String, String)>,  // (name, version_constraint)
}

impl Manifest {
    /// 从当前目录读取 duanyan.json
    pub fn load(dir: &Path) -> Result<Manifest, String> {
        let path = dir.join("duanyan.json");
        if !path.exists() {
            return Err(format!("未找到 duanyan.json，请先运行 'duanyan pack init'"));
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("读取 duanyan.json 失败: {}", e))?;
        Self::from_str(&content)
    }

    /// 从 JSON 字符串解析
    pub fn from_str(s: &str) -> Result<Manifest, String> {
        let v = JsonValue::parse(s)?;
        let name = v.get("name").and_then(|v| v.as_str()).unwrap_or("unnamed").to_string();
        let version = v.get("version").and_then(|v| v.as_str()).unwrap_or("0.1.0").to_string();
        let description = v.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let main = v.get("main").and_then(|v| v.as_str()).unwrap_or("index.js").to_string();
        let author = v.get("author").and_then(|v| v.as_str()).unwrap_or("").to_string();

        let mut dependencies = Vec::new();
        if let Some(deps) = v.get("dependencies") {
            if let Some(pairs) = deps.as_object() {
                for (k, v) in pairs {
                    if let Some(ver) = v.as_str() {
                        dependencies.push((k.clone(), ver.to_string()));
                    }
                }
            }
        }

        Ok(Manifest { name, version, description, main, author, dependencies })
    }

    /// 保存为 duanyan.json
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let json = self.to_json();
        let path = dir.join("duanyan.json");
        fs::write(&path, json)
            .map_err(|e| format!("写入 duanyan.json 失败: {}", e))
    }

    /// 序列化为 JSON
    pub fn to_json(&self) -> String {
        let deps: Vec<(String, JsonValue)> = self.dependencies.iter()
            .map(|(k, v)| (k.clone(), JsonValue::Str(v.clone())))
            .collect();

        let obj = JsonValue::Object(vec![
            ("name".to_string(), JsonValue::Str(self.name.clone())),
            ("version".to_string(), JsonValue::Str(self.version.clone())),
            ("description".to_string(), JsonValue::Str(self.description.clone())),
            ("main".to_string(), JsonValue::Str(self.main.clone())),
            ("author".to_string(), JsonValue::Str(self.author.clone())),
            ("dependencies".to_string(), JsonValue::Object(deps)),
        ]);
        obj.to_json_string()
    }

    /// 添加依赖
    pub fn add_dependency(&mut self, name: &str, version: &str) {
        // 更新已有依赖或添加新依赖
        if let Some(entry) = self.dependencies.iter_mut().find(|(n, _)| n == name) {
            entry.1 = version.to_string();
        } else {
            self.dependencies.push((name.to_string(), version.to_string()));
        }
    }

    /// 移除依赖
    pub fn remove_dependency(&mut self, name: &str) -> bool {
        let len = self.dependencies.len();
        self.dependencies.retain(|(n, _)| n != name);
        self.dependencies.len() < len
    }
}

/// 创建默认清单
pub fn create_default_manifest(name: &str, version: &str) -> Manifest {
    Manifest {
        name: name.to_string(),
        version: version.to_string(),
        description: String::new(),
        main: "index.js".to_string(),
        author: String::new(),
        dependencies: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_roundtrip() {
        let m = Manifest {
            name: "test-project".to_string(),
            version: "1.0.0".to_string(),
            description: "A test".to_string(),
            main: "main.js".to_string(),
            author: "hunter".to_string(),
            dependencies: vec![
                ("math-utils".to_string(), "^1.0.0".to_string()),
            ],
        };
        let json = m.to_json();
        let m2 = Manifest::from_str(&json).unwrap();
        assert_eq!(m2.name, "test-project");
        assert_eq!(m2.dependencies.len(), 1);
        assert_eq!(m2.dependencies[0].0, "math-utils");
    }

    #[test]
    fn test_add_remove_dependency() {
        let mut m = create_default_manifest("test", "0.1.0");
        m.add_dependency("foo", "^1.0.0");
        assert_eq!(m.dependencies.len(), 1);
        m.add_dependency("foo", "^2.0.0");
        assert_eq!(m.dependencies.len(), 1);
        assert_eq!(m.dependencies[0].1, "^2.0.0");
        assert!(m.remove_dependency("foo"));
        assert_eq!(m.dependencies.len(), 0);
        assert!(!m.remove_dependency("nonexistent"));
    }
}
