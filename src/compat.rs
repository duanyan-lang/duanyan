//! Espruino / Moddable 兼容预处理模块
//!
//! Phase C: 跨引擎语法兼容预处理
//! 自动将 Espruino/Moddable 风格 API 转换为 HardyScript 标准模块化写法

/// 兼容层警告
#[derive(Debug, Clone)]
pub struct CompatWarning {
    pub line: usize,
    pub original: String,
    pub suggestion: String,
    pub api_type: LegacyApiType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LegacyApiType {
    /// Espruino 全局 API
    Espruino,
    /// Moddable import 风格
    Moddable,
    /// 其他旧式 API
    Other,
}

impl std::fmt::Display for CompatWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "⚠️  行 {}: 检测到 {:?} 旧式 API '{}', 建议改用 Hardy 标准写法: {}",
            self.line, self.api_type, self.original, self.suggestion
        )
    }
}

/// 预处理结果
#[derive(Debug, Clone)]
pub struct PreprocessResult {
    /// 转换后的源码
    pub source: String,
    /// 兼容警告列表
    pub warnings: Vec<CompatWarning>,
    /// 是否检测到旧式 API
    pub has_legacy_api: bool,
    /// 需要注入的模块导入
    pub required_imports: Vec<String>,
}

/// Espruino → HardyScript 转换规则
#[derive(Debug, Clone)]
struct TransformRule {
    /// 匹配模式 (简单字符串匹配)
    pattern: &'static str,
    /// 替换为
    replacement: &'static str,
    /// 需要的导入模块
    import_module: &'static str,
    /// API 类型
    api_type: LegacyApiType,
}

fn get_transform_rules() -> Vec<TransformRule> {
    vec![
        // Espruino GPIO API
        TransformRule {
            pattern: "pinMode(",
            replacement: "__hardy_gpio__.pinMode(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "digitalWrite(",
            replacement: "__hardy_gpio__.digitalWrite(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "digitalRead(",
            replacement: "__hardy_gpio__.digitalRead(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "analogRead(",
            replacement: "__hardy_adc__.read(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "analogWrite(",
            replacement: "__hardy_pwm__.write(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        // Espruino Timer API
        TransformRule {
            pattern: "setTimeout(",
            replacement: "__hardy_timer__.setTimeout(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "setInterval(",
            replacement: "__hardy_timer__.setInterval(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "clearTimeout(",
            replacement: "__hardy_timer__.clearTimeout(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "clearInterval(",
            replacement: "__hardy_timer__.clearInterval(",
            import_module: "hardy:hw",
            api_type: LegacyApiType::Espruino,
        },
        // Espruino Serial API
        TransformRule {
            pattern: "Serial1.println(",
            replacement: "console.log(",
            import_module: "",
            api_type: LegacyApiType::Espruino,
        },
        TransformRule {
            pattern: "Serial1.print(",
            replacement: "console.log(",
            import_module: "",
            api_type: LegacyApiType::Espruino,
        },
    ]
}

/// 预处理源码，转换旧式 API
pub fn preprocess(source: &str) -> PreprocessResult {
    let mut result_source = source.to_string();
    let mut warnings = Vec::new();
    let mut required_imports = Vec::new();
    let rules = get_transform_rules();

    // 逐行处理
    let lines: Vec<&str> = source.lines().collect();
    let mut output_lines = Vec::new();

    for (line_num, line) in lines.iter().enumerate() {
        let mut current_line = line.to_string();

        for rule in &rules {
            if line.contains(rule.pattern) {
                // 记录警告
                warnings.push(CompatWarning {
                    line: line_num + 1,
                    original: rule.pattern.trim_end_matches('(').to_string(),
                    suggestion: get_suggestion(rule.pattern),
                    api_type: rule.api_type.clone(),
                });

                // 转换代码
                current_line = current_line.replace(rule.pattern, rule.replacement);

                // 记录需要的导入
                if !rule.import_module.is_empty() && !required_imports.contains(&rule.import_module.to_string()) {
                    required_imports.push(rule.import_module.to_string());
                }
            }
        }

        output_lines.push(current_line);
    }

    result_source = output_lines.join("\n");

    // 如果需要注入模块，在文件开头添加
    if !required_imports.is_empty() {
        let import_line = format!(
            "// [HardyScript 自动注入] 兼容旧式 API\nconst __hardy_gpio__ = {{ pinMode: GPIO.output, digitalWrite: (p,v) => v ? GPIO.high(p) : GPIO.low(p), digitalRead: GPIO.read }};\nconst __hardy_adc__ = {{ read: ADC.read }};\nconst __hardy_pwm__ = {{ write: PWM.duty }};\nconst __hardy_timer__ = {{ setTimeout: Timer.setTimeout, setInterval: Timer.setInterval, clearTimeout: Timer.clear, clearInterval: Timer.clear }};\n"
        );
        result_source = format!("{}{}", import_line, result_source);
    }

    PreprocessResult {
        source: result_source,
        has_legacy_api: !warnings.is_empty(),
        warnings,
        required_imports,
    }
}

/// 获取 API 建议
fn get_suggestion(pattern: &str) -> String {
    match pattern {
        "pinMode(" => "const gpio = require('hardy:hw').GPIO; gpio.output(pin)".to_string(),
        "digitalWrite(" => "gpio.write(pin, value)".to_string(),
        "digitalRead(" => "gpio.read(pin)".to_string(),
        "analogRead(" => "const adc = require('hardy:hw').ADC; adc.read(channel)".to_string(),
        "analogWrite(" => "const pwm = require('hardy:hw').PWM; pwm.duty(pin, value)".to_string(),
        "setTimeout(" => "const Timer = require('hardy:hw').Timer; Timer.delay(ms)".to_string(),
        "setInterval(" => "// 使用 while(true) + Timer.delay() 替代".to_string(),
        _ => "使用 hardy:hw 标准模块".to_string(),
    }
}

/// Moddable 风格 import 转换
pub fn transform_moddable_import(source: &str) -> (String, Vec<CompatWarning>) {
    let mut warnings = Vec::new();
    let mut result = source.to_string();

    // Moddable: import Digital from "pins/digital/outline"
    // Moddable: import Analog from "pins/analog"
    // Moddable: import Timer from "timer"

    let moddable_patterns = [
        ("import Digital from \"pins/digital", "const GPIO = require('hardy:hw').GPIO"),
        ("import Analog from \"pins/analog", "const ADC = require('hardy:hw').ADC"),
        ("import PWM from \"pwm", "const PWM = require('hardy:hw').PWM"),
        ("import Timer from \"timer", "const Timer = require('hardy:hw').Timer"),
        ("import Serial from \"serial", "// Serial → console.log"),
    ];

    let lines: Vec<&str> = source.lines().collect();
    let mut output_lines = Vec::new();

    for (line_num, line) in lines.iter().enumerate() {
        let mut current_line = line.to_string();

        for (pattern, replacement) in &moddable_patterns {
            if line.contains(pattern) {
                warnings.push(CompatWarning {
                    line: line_num + 1,
                    original: pattern.to_string(),
                    suggestion: replacement.to_string(),
                    api_type: LegacyApiType::Moddable,
                });
                // 注释掉原 import，添加 Hardy 标准 import
                current_line = format!("// [Moddable兼容] {}\n{}", line.trim(), replacement);
            }
        }

        output_lines.push(current_line);
    }

    result = output_lines.join("\n");
    (result, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_espruino_pinmode_transform() {
        let source = r#"
pinMode(2, "output");
digitalWrite(2, 1);
"#;
        let result = preprocess(source);
        assert!(result.has_legacy_api);
        assert_eq!(result.warnings.len(), 2);
        assert!(result.source.contains("__hardy_gpio__"));
    }

    #[test]
    fn test_moddable_import_transform() {
        let source = r#"import Digital from "pins/digital/outline";"#;
        let (result, warnings) = transform_moddable_import(source);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].api_type, LegacyApiType::Moddable);
        assert!(result.contains("require('hardy:hw')"));
    }

    #[test]
    fn test_no_legacy_api() {
        let source = r#"
let x = 42;
console.log(x);
"#;
        let result = preprocess(source);
        assert!(!result.has_legacy_api);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn test_timer_transform() {
        let source = "setTimeout(() => console.log('hi'), 1000);";
        let result = preprocess(source);
        assert!(result.has_legacy_api);
        assert!(result.source.contains("__hardy_timer__"));
    }
}
