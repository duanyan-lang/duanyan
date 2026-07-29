//! .hardyrc 配置文件解析
//!
//! 支持项目级别的 Registry 配置:
//! ```ini
//! registry=https://packages.hardy.dev
//! token=hardy_xxxxxxxxxxxx
//! scope=@my-team
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// .hardyrc 配置
#[derive(Debug, Clone)]
pub struct SnarRc {
    pub registry: String,
    pub token: Option<String>,
    pub scope: Option<String>,
    pub extra: HashMap<String, String>,
}

impl Default for SnarRc {
    fn default() -> Self {
        Self {
            registry: "http://localhost:7890".to_string(),
            token: None,
            scope: None,
            extra: HashMap::new(),
        }
    }
}

impl SnarRc {
    /// 从目录加载 .hardyrc (查找当前目录 -> 父目录 -> 用户目录)
    pub fn load(dir: &Path) -> Self {
        // 查找顺序: 当前目录 -> 父目录 -> HOME
        let candidates = vec![
            dir.join(".hardyrc"),
            dir.join("_hardyrc"),
        ];

        // 向上查找
        let mut current = dir.to_path_buf();
        loop {
            let rc_path = current.join(".hardyrc");
            if rc_path.exists() {
                if let Ok(content) = fs::read_to_string(&rc_path) {
                    return Self::parse(&content);
                }
            }
            if !current.pop() { break; }
        }

        // 检查 HOME 目录
        if let Some(home) = std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok()) {
            let home_rc = Path::new(&home).join(".hardyrc");
            if home_rc.exists() {
                if let Ok(content) = fs::read_to_string(&home_rc) {
                    return Self::parse(&content);
                }
            }
        }

        Self::default()
    }

    /// 解析 .hardyrc 内容 (INI 格式)
    pub fn parse(content: &str) -> Self {
        let mut rc = Self::default();
        let mut extra = HashMap::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }

            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim().to_string();
                let value = line[eq_pos + 1..].trim().to_string();

                match key.as_str() {
                    "registry" => rc.registry = value,
                    "token" => rc.token = Some(value),
                    "scope" => rc.scope = Some(value),
                    _ => { extra.insert(key, value); }
                }
            }
        }

        rc.extra = extra;
        rc
    }

    /// 序列化为 .hardyrc 格式
    pub fn to_string_repr(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("registry={}\n", self.registry));
        if let Some(ref token) = self.token {
            out.push_str(&format!("token={}\n", token));
        }
        if let Some(ref scope) = self.scope {
            out.push_str(&format!("scope={}\n", scope));
        }
        for (k, v) in &self.extra {
            out.push_str(&format!("{}={}\n", k, v));
        }
        out
    }

    /// 保存到目录
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(".hardyrc");
        fs::write(&path, self.to_string_repr())
            .map_err(|e| format!("写入 .hardyrc 失败: {}", e))
    }
}

/// 获取有效的 registry URL (合并 .hardyrc 和环境变量)
pub fn get_effective_registry_url(dir: &Path) -> String {
    // 优先级: 环境变量 > .hardyrc > 默认值
    if let Ok(url) = std::env::var("HARDY_REGISTRY") {
        return url;
    }
    let rc = SnarRc::load(dir);
    rc.registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hardyrc() {
        let content = r#"
# Hardy Registry 配置
registry=https://packages.hardy.dev
token=hardy_abc123
scope=@my-team
"#;
        let rc = SnarRc::parse(content);
        assert_eq!(rc.registry, "https://packages.hardy.dev");
        assert_eq!(rc.token, Some("hardy_abc123".to_string()));
        assert_eq!(rc.scope, Some("@my-team".to_string()));
    }

    #[test]
    fn test_default_hardyrc() {
        let rc = SnarRc::default();
        assert_eq!(rc.registry, "http://localhost:7890");
        assert!(rc.token.is_none());
    }

    #[test]
    fn test_roundtrip() {
        let mut rc = SnarRc::default();
        rc.registry = "https://my-registry.dev".to_string();
        rc.token = Some("tok123".to_string());
        let serialized = rc.to_string_repr();
        let rc2 = SnarRc::parse(&serialized);
        assert_eq!(rc2.registry, "https://my-registry.dev");
        assert_eq!(rc2.token, Some("tok123".to_string()));
    }
}
