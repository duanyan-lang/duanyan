//! HardyScript Project Configuration
//!
//! `duanyan.config.json` sits at the project root and tells HardyScript how to
//! compile, package, and flash the firmware.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Project configuration schema.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
    pub target: TargetConfig,
    pub build: BuildConfig,
    pub flash: FlashConfig,
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TargetConfig {
    pub platform: String, // "gowin", "stm32", "esp32"
    pub device: String,   // "GW1NSR-4C", "STM32F103C8", ...
    pub arch: String,     // "counpre64"
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BuildConfig {
    pub entry: String,
    pub output_dir: String,
    pub optimize: bool,
    pub emit_asm: bool,
    pub emit_rom: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FlashConfig {
    pub enabled: bool,
    pub programmer_path: Option<String>,
    pub port: Option<String>,
    pub cable: Option<String>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "untitled".to_string(),
            version: "0.1.0".to_string(),
            target: TargetConfig {
                platform: "gowin".to_string(),
                device: "GW1NSR-4C".to_string(),
                arch: "counpre64".to_string(),
            },
            build: BuildConfig {
                entry: "main.js".to_string(),
                output_dir: "build".to_string(),
                optimize: false,
                emit_asm: false,
                emit_rom: true,
            },
            flash: FlashConfig {
                enabled: true,
                programmer_path: None,
                port: None,
                cable: Some("USB2".to_string()),
            },
            extra: HashMap::new(),
        }
    }
}

impl ProjectConfig {
    /// Load `duanyan.config.json` from the given directory, returning defaults if missing.
    pub fn load(project_dir: &Path) -> Result<Self, String> {
        let path = project_dir.join("duanyan.config.json");
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {}", e))?;
        Self::from_json(&raw)
    }

    /// Parse a JSON configuration string.
    pub fn from_json(raw: &str) -> Result<Self, String> {
        // Minimal hand-rolled JSON parser for the fields we care about.
        let mut cfg = Self::default();
        let obj = parse_json_object(raw)?;

        if let Some(v) = obj.get("name") { cfg.name = v.to_string(); }
        if let Some(v) = obj.get("version") { cfg.version = v.to_string(); }

        if let Some(target) = obj.get("target") {
            let target_obj = parse_json_object(target)?;
            if let Some(v) = target_obj.get("platform") { cfg.target.platform = v.to_string(); }
            if let Some(v) = target_obj.get("device") { cfg.target.device = v.to_string(); }
            if let Some(v) = target_obj.get("arch") { cfg.target.arch = v.to_string(); }
        }

        if let Some(build) = obj.get("build") {
            let build_obj = parse_json_object(build)?;
            if let Some(v) = build_obj.get("entry") { cfg.build.entry = v.to_string(); }
            if let Some(v) = build_obj.get("outputDir") { cfg.build.output_dir = v.to_string(); }
            if let Some(v) = build_obj.get("optimize") { cfg.build.optimize = v == "true"; }
            if let Some(v) = build_obj.get("emitAsm") { cfg.build.emit_asm = v == "true"; }
            if let Some(v) = build_obj.get("emitRom") { cfg.build.emit_rom = v == "true"; }
        }

        if let Some(flash) = obj.get("flash") {
            let flash_obj = parse_json_object(flash)?;
            if let Some(v) = flash_obj.get("enabled") { cfg.flash.enabled = v == "true"; }
            if let Some(v) = flash_obj.get("programmerPath") { cfg.flash.programmer_path = Some(v.to_string()); }
            if let Some(v) = flash_obj.get("port") { cfg.flash.port = Some(v.to_string()); }
            if let Some(v) = flash_obj.get("cable") { cfg.flash.cable = Some(v.to_string()); }
        }

        Ok(cfg)
    }

    /// Generate a default JSON configuration for a new project.
    pub fn to_json(&self) -> String {
        format!(
            "{{\n\
             \"name\": \"{}\",\n\
             \"version\": \"{}\",\n\
             \"target\": {{\n\
             \"  platform\": \"{}\",\n\
             \"  device\": \"{}\",\n\
             \"  arch\": \"{}\"\n\
             }},\n\
             \"build\": {{\n\
             \"  entry\": \"{}\",\n\
             \"  outputDir\": \"{}\",\n\
             \"  optimize\": {},\n\
             \"  emitAsm\": {},\n\
             \"  emitRom\": {}\n\
             }},\n\
             \"flash\": {{\n\
             \"  enabled\": {},\n\
             \"  programmerPath\": null,\n\
             \"  port\": null,\n\
             \"  cable\": \"{}\"\n\
             }}\n\
             }}\n",
            self.name, self.version,
            self.target.platform, self.target.device, self.target.arch,
            self.build.entry, self.build.output_dir,
            self.build.optimize, self.build.emit_asm, self.build.emit_rom,
            self.flash.enabled, self.flash.cable.as_deref().unwrap_or("USB2")
        )
    }

    /// Resolve paths relative to the project directory.
    pub fn output_dir(&self, project_dir: &Path) -> PathBuf {
        project_dir.join(&self.build.output_dir)
    }

    pub fn entry_path(&self, project_dir: &Path) -> PathBuf {
        project_dir.join(&self.build.entry)
    }
}

/// Very small JSON object extractor: returns a map of top-level string values.
/// Supports only string/boolean/null and nested objects (as raw substring).
fn parse_json_object(raw: &str) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err("Invalid JSON object".to_string());
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut key = String::new();
    let mut value = String::new();
    let mut in_key = false;
    let mut in_value = false;
    let mut in_string = false;
    let mut depth = 0;
    let mut chars = inner.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '"' && depth == 0 {
            if !in_key && !in_value {
                in_key = true;
                key.clear();
            } else if in_key {
                in_key = false;
                // skip whitespace and colon
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == ':' { chars.next(); } else { break; }
                }
                in_value = true;
                value.clear();
                in_string = false;
            } else if in_value {
                if !in_string {
                    in_string = true;  // opening quote of string value
                } else {
                    // closing quote → finalize value
                    map.insert(key.trim().to_string(), value.trim().to_string());
                    in_value = false;
                    in_string = false;
                    value.clear();
                    // skip to next comma or end
                    while let Some(&c) = chars.peek() {
                        if c == ',' { chars.next(); break; } else if c.is_whitespace() { chars.next(); } else { break; }
                    }
                }
            }
        } else if ch == '{' && in_value {
            depth += 1;
            value.push(ch);
        } else if ch == '}' && in_value {
            if depth > 0 {
                depth -= 1;
                value.push(ch);
            }
        } else if ch == ',' && depth == 0 && in_value {
            in_value = false;
            map.insert(key.trim().to_string(), value.trim().to_string());
        } else {
            if in_key { key.push(ch); }
            else if in_value { value.push(ch); }
        }
    }
    if in_value {
        map.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_roundtrip() {
        let json = r#"{
            "name": "demo",
            "version": "1.0.0",
            "target": { "platform": "gowin", "device": "GW1NSR-4C" },
            "build": { "entry": "main.js", "outputDir": "build", "optimize": true }
        }"#;
        let cfg = ProjectConfig::from_json(json).unwrap();
        assert_eq!(cfg.name, "demo");
        assert_eq!(cfg.target.platform, "gowin");
        assert!(cfg.build.optimize);
    }
}
