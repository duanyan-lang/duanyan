//! snar.lock 版本锁定
//!
//! 记录已安装包的精确版本和校验和，确保可重现安装

use std::fs;
use std::path::Path;
use super::json::JsonValue;

/// 锁定的包信息
#[derive(Debug, Clone)]
pub struct LockedPackage {
    pub name: String,
    pub version: String,
    pub checksum: String,
}

/// 锁文件 (snar.lock)
#[derive(Debug, Clone)]
pub struct Lockfile {
    pub packages: Vec<LockedPackage>,
}

impl Lockfile {
    /// 从目录加载 snar.lock
    pub fn load(dir: &Path) -> Result<Lockfile, String> {
        let path = dir.join("snar.lock");
        if !path.exists() {
            return Ok(Lockfile { packages: Vec::new() });
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("读取 snar.lock 失败: {}", e))?;
        Self::from_str(&content)
    }

    /// 从 JSON 解析
    pub fn from_str(s: &str) -> Result<Lockfile, String> {
        let v = JsonValue::parse(s)?;
        let mut packages = Vec::new();

        if let Some(pkgs) = v.get("packages") {
            if let Some(pairs) = pkgs.as_object() {
                for (name, info) in pairs {
                    let version = info.get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0.0.0")
                        .to_string();
                    let checksum = info.get("checksum")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    packages.push(LockedPackage {
                        name: name.clone(),
                        version,
                        checksum,
                    });
                }
            }
        }

        Ok(Lockfile { packages })
    }

    /// 保存到 snar.lock
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let json = self.to_json();
        let path = dir.join("snar.lock");
        fs::write(&path, json)
            .map_err(|e| format!("写入 snar.lock 失败: {}", e))
    }

    /// 序列化为 JSON
    pub fn to_json(&self) -> String {
        let pkg_entries: Vec<(String, JsonValue)> = self.packages.iter()
            .map(|p| {
                (p.name.clone(), JsonValue::Object(vec![
                    ("version".to_string(), JsonValue::Str(p.version.clone())),
                    ("checksum".to_string(), JsonValue::Str(p.checksum.clone())),
                ]))
            })
            .collect();

        let obj = JsonValue::Object(vec![
            ("packages".to_string(), JsonValue::Object(pkg_entries)),
        ]);
        obj.to_json_string()
    }

    /// 添加或更新锁定的包
    pub fn upsert(&mut self, pkg: LockedPackage) {
        if let Some(entry) = self.packages.iter_mut().find(|p| p.name == pkg.name) {
            entry.version = pkg.version;
            entry.checksum = pkg.checksum;
        } else {
            self.packages.push(pkg);
        }
    }

    /// 移除包
    pub fn remove(&mut self, name: &str) -> bool {
        let len = self.packages.len();
        self.packages.retain(|p| p.name != name);
        self.packages.len() < len
    }

    /// 查找已锁定的包
    pub fn find(&self, name: &str) -> Option<&LockedPackage> {
        self.packages.iter().find(|p| p.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lockfile_roundtrip() {
        let mut lf = Lockfile { packages: Vec::new() };
        lf.upsert(LockedPackage {
            name: "math-utils".to_string(),
            version: "1.2.3".to_string(),
            checksum: "abc123".to_string(),
        });
        let json = lf.to_json();
        let lf2 = Lockfile::from_str(&json).unwrap();
        assert_eq!(lf2.packages.len(), 1);
        assert_eq!(lf2.packages[0].version, "1.2.3");
    }

    #[test]
    fn test_upsert_and_remove() {
        let mut lf = Lockfile { packages: Vec::new() };
        lf.upsert(LockedPackage {
            name: "a".to_string(), version: "1.0.0".to_string(), checksum: "x".to_string(),
        });
        lf.upsert(LockedPackage {
            name: "a".to_string(), version: "2.0.0".to_string(), checksum: "y".to_string(),
        });
        assert_eq!(lf.packages.len(), 1);
        assert_eq!(lf.packages[0].version, "2.0.0");
        assert!(lf.remove("a"));
        assert_eq!(lf.packages.len(), 0);
    }
}
