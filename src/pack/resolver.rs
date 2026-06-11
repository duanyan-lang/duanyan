//! 依赖解析 + 版本约束
//!
//! 支持语义化版本约束:
//! - ^1.2.3  = >=1.2.3, <2.0.0 (兼容更新)
//! - ~1.2.3  = >=1.2.3, <1.3.0 (补丁更新)
//! - 1.2.3   = 精确匹配
//! - *       = 任意版本

/// 语义化版本
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVer {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SemVer {
    pub fn parse(s: &str) -> Result<SemVer, String> {
        let s = s.trim().trim_start_matches('v');
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("无效版本号: '{}'", s));
        }
        let major = parts[0].parse::<u32>()
            .map_err(|_| format!("无效主版本号: '{}'", parts[0]))?;
        let minor = parts[1].parse::<u32>()
            .map_err(|_| format!("无效次版本号: '{}'", parts[1]))?;
        let patch = parts[2].parse::<u32>()
            .map_err(|_| format!("无效补丁版本号: '{}'", parts[2]))?;
        Ok(SemVer { major, minor, patch })
    }

    pub fn to_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }

    /// 比较版本
    pub fn cmp(&self, other: &SemVer) -> std::cmp::Ordering {
        self.major.cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }

    pub fn gte(&self, other: &SemVer) -> bool {
        matches!(self.cmp(other), std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    }

    pub fn lt(&self, other: &SemVer) -> bool {
        matches!(self.cmp(other), std::cmp::Ordering::Less)
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cmp(other)
    }
}

/// 版本约束
#[derive(Debug, Clone)]
pub enum VersionConstraint {
    /// 任意版本 (*)
    Any,
    /// 精确版本 (1.2.3)
    Exact(SemVer),
    /// 兼容更新 (^1.2.3 = >=1.2.3, <2.0.0)
    Caret(SemVer),
    /// 补丁更新 (~1.2.3 = >=1.2.3, <1.3.0)
    Tilde(SemVer),
}

impl VersionConstraint {
    /// 解析版本约束字符串
    pub fn parse(s: &str) -> Result<VersionConstraint, String> {
        let s = s.trim();
        if s == "*" || s.is_empty() {
            return Ok(VersionConstraint::Any);
        }
        if let Some(rest) = s.strip_prefix('^') {
            return Ok(VersionConstraint::Caret(SemVer::parse(rest)?));
        }
        if let Some(rest) = s.strip_prefix('~') {
            return Ok(VersionConstraint::Tilde(SemVer::parse(rest)?));
        }
        Ok(VersionConstraint::Exact(SemVer::parse(s)?))
    }

    /// 检查版本是否满足约束
    pub fn satisfies(&self, version: &SemVer) -> bool {
        match self {
            VersionConstraint::Any => true,
            VersionConstraint::Exact(v) => version == v,
            VersionConstraint::Caret(min) => {
                // >=min, <next_major
                if !version.gte(min) { return false; }
                if min.major == 0 {
                    // ^0.x.y: 只在 0.x 范围内兼容
                    version.major == 0 && version.minor == min.minor
                } else {
                    version.major == min.major
                }
            }
            VersionConstraint::Tilde(min) => {
                // >=min, <next_minor
                version.gte(min) && version.major == min.major && version.minor == min.minor
            }
        }
    }

    /// 从可用版本列表中选择最佳匹配
    pub fn best_match(&self, versions: &[String]) -> Option<String> {
        let mut candidates: Vec<SemVer> = versions.iter()
            .filter_map(|v| SemVer::parse(v).ok())
            .filter(|v| self.satisfies(v))
            .collect();
        candidates.sort();
        candidates.last().map(|v| v.to_string())
    }
}

/// 解析后的依赖 (包含精确版本)
#[derive(Debug, Clone)]
pub struct ResolvedDep {
    pub name: String,
    pub version: String,
}

/// 解析依赖列表
/// deps: [(name, constraint_str)]
/// available: 查询函数 fn(name) -> Vec<version_string>
pub fn resolve_deps(
    deps: &[(String, String)],
    available: &dyn Fn(&str) -> Result<Vec<String>, String>,
) -> Result<Vec<ResolvedDep>, String> {
    let mut resolved = Vec::new();

    for (name, constraint_str) in deps {
        let constraint = VersionConstraint::parse(constraint_str)?;
        let versions = available(name)?;
        match constraint.best_match(&versions) {
            Some(version) => {
                resolved.push(ResolvedDep { name: name.clone(), version });
            }
            None => {
                return Err(format!(
                    "无法解析依赖 '{}': 约束 '{}' 无匹配版本 (可用: {})",
                    name,
                    constraint_str,
                    if versions.is_empty() { "无".to_string() } else { versions.join(", ") }
                ));
            }
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semver_parse() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
    }

    #[test]
    fn test_semver_compare() {
        let a = SemVer::parse("1.2.3").unwrap();
        let b = SemVer::parse("1.3.0").unwrap();
        assert!(a < b);
    }

    #[test]
    fn test_caret_constraint() {
        let c = VersionConstraint::parse("^1.2.0").unwrap();
        assert!(c.satisfies(&SemVer::parse("1.2.0").unwrap()));
        assert!(c.satisfies(&SemVer::parse("1.9.9").unwrap()));
        assert!(!c.satisfies(&SemVer::parse("2.0.0").unwrap()));
        assert!(!c.satisfies(&SemVer::parse("1.1.0").unwrap()));
    }

    #[test]
    fn test_tilde_constraint() {
        let c = VersionConstraint::parse("~1.2.0").unwrap();
        assert!(c.satisfies(&SemVer::parse("1.2.0").unwrap()));
        assert!(c.satisfies(&SemVer::parse("1.2.9").unwrap()));
        assert!(!c.satisfies(&SemVer::parse("1.3.0").unwrap()));
    }

    #[test]
    fn test_exact_constraint() {
        let c = VersionConstraint::parse("1.0.0").unwrap();
        assert!(c.satisfies(&SemVer::parse("1.0.0").unwrap()));
        assert!(!c.satisfies(&SemVer::parse("1.0.1").unwrap()));
    }

    #[test]
    fn test_any_constraint() {
        let c = VersionConstraint::parse("*").unwrap();
        assert!(c.satisfies(&SemVer::parse("99.99.99").unwrap()));
    }

    #[test]
    fn test_best_match() {
        let c = VersionConstraint::parse("^1.0.0").unwrap();
        let versions = vec!["1.0.0".to_string(), "1.5.0".to_string(), "2.0.0".to_string()];
        assert_eq!(c.best_match(&versions), Some("1.5.0".to_string()));
    }

    #[test]
    fn test_resolve_deps() {
        let deps = vec![
            ("math".to_string(), "^1.0.0".to_string()),
            ("utils".to_string(), "~2.1.0".to_string()),
        ];
        let available = |name: &str| -> Result<Vec<String>, String> {
            match name {
                "math" => Ok(vec!["1.0.0".to_string(), "1.5.0".to_string(), "2.0.0".to_string()]),
                "utils" => Ok(vec!["2.1.0".to_string(), "2.1.5".to_string(), "2.2.0".to_string()]),
                _ => Err("unknown".to_string()),
            }
        };
        let resolved = resolve_deps(&deps, &available).unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].version, "1.5.0");
        assert_eq!(resolved[1].version, "2.1.5");
    }
}
