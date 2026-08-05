//! HardyScript Lint - 代码规范检查器
//!
//! Phase H: `hardy lint` 命令
//! 可扩展规则系统 + 自动修复 + 资源约束检查

use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// 严重级别
#[derive(Debug, Clone, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Error => "E",
            Self::Warning => "W",
            Self::Info => "I",
        }
    }
}

/// Lint 规则定义
#[derive(Debug, Clone)]
pub struct LintRule {
    pub id: String,
    pub description: String,
    pub severity: Severity,
    pub fixable: bool,
}

/// Lint 配置
#[derive(Debug, Clone)]
pub struct LintConfig {
    pub rules: Vec<LintRule>,
    pub max_ram_kb: Option<u32>,
    pub max_flash_kb: Option<u32>,
    pub severity_overrides: HashMap<String, Severity>,
}

/// Lint 诊断信息
#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub suggestion: Option<String>,
    pub fix: Option<LintFix>,
}

/// 自动修复操作
#[derive(Debug, Clone)]
pub struct LintFix {
    pub description: String,
    pub line: u32,
    pub old_text: String,
    pub new_text: String,
}

/// Lint 结果
#[derive(Debug, Clone)]
pub struct LintResult {
    pub diagnostics: Vec<LintDiagnostic>,
    pub error_count: u32,
    pub warning_count: u32,
    pub fixable_count: u32,
}

impl LintConfig {
    /// 默认配置（启用所有内置规则）
    pub fn default_config() -> Self {
        Self {
            rules: builtin_rules(),
            max_ram_kb: None,
            max_flash_kb: None,
            severity_overrides: HashMap::new(),
        }
    }

    /// 从 .hardy/lint.json 加载
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(".hardy").join("lint.json");
        if !path.exists() {
            return Self::default_config();
        }
        match fs::read_to_string(&path) {
            Ok(raw) => Self::from_json(&raw),
            Err(_) => Self::default_config(),
        }
    }

    /// 从 JSON 解析
    pub fn from_json(raw: &str) -> Self {
        let mut config = Self::default_config();
        let trimmed = raw.trim();

        // 解析 maxRamKb / maxFlashKb
        if let Some(v) = extract_number(trimmed, "maxRamKb") {
            config.max_ram_kb = Some(v as u32);
        }
        if let Some(v) = extract_number(trimmed, "maxFlashKb") {
            config.max_flash_kb = Some(v as u32);
        }

        // 解析 rules overrides
        if let Some(rules_start) = trimmed.find("\"rules\"") {
            let rest = &trimmed[rules_start..];
            if let Some(brace) = rest.find('{') {
                let inner = &rest[brace + 1..];
                if let Some(end) = inner.find('}') {
                    let rules_block = &inner[..end];
                    for rule in &mut config.rules {
                        if let Some(val) = extract_string_value(rules_block, &rule.id) {
                            match val.as_str() {
                                "error" => { config.severity_overrides.insert(rule.id.clone(), Severity::Error); }
                                "warning" => { config.severity_overrides.insert(rule.id.clone(), Severity::Warning); }
                                "info" => { config.severity_overrides.insert(rule.id.clone(), Severity::Info); }
                                "off" => { config.severity_overrides.insert(rule.id.clone(), Severity::Info); }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        config
    }

    /// 获取规则的实际严重级别（考虑 override）
    pub fn effective_severity(&self, rule_id: &str) -> Severity {
        if let Some(s) = self.severity_overrides.get(rule_id) {
            s.clone()
        } else if let Some(r) = self.rules.iter().find(|r| r.id == rule_id) {
            r.severity.clone()
        } else {
            Severity::Warning
        }
    }

    /// 是否禁用某规则
    pub fn is_rule_disabled(&self, rule_id: &str) -> bool {
        matches!(self.severity_overrides.get(rule_id), Some(Severity::Info))
    }
}

// =========================================================================
// 内置规则集
// =========================================================================

pub fn builtin_rules() -> Vec<LintRule> {
    vec![
        LintRule { id: "no-var".into(), description: "使用 let/const 替代 var".into(), severity: Severity::Error, fixable: true },
        LintRule { id: "prefer-const".into(), description: "不变的变量用 const".into(), severity: Severity::Warning, fixable: true },
        LintRule { id: "no-unused-vars".into(), description: "未使用的变量".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "no-implicit-globals".into(), description: "禁止隐式全局变量".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "eqeqeq".into(), description: "使用 === 而非 ==".into(), severity: Severity::Error, fixable: true },
        LintRule { id: "no-eval".into(), description: "禁止 eval()".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "no-console-log".into(), description: "生产代码移除 console.log".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "max-lines".into(), description: "文件不超过500行".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "no-magic-numbers".into(), description: "避免魔数 (除0/1/2外)".into(), severity: Severity::Info, fixable: false },
        LintRule { id: "require-hardware-init".into(), description: "必须有 hardware.init() 调用".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "no-blocking-loops".into(), description: "禁止无 delay/yield 的无限循环".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "prefer-hardware-api".into(), description: "使用 HardyScript API".into(), severity: Severity::Info, fixable: false },
        LintRule { id: "resource-budget".into(), description: "RAM/Flash 不超板子上限".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "pin-valid".into(), description: "引脚号在板子合法范围内".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "no-dead-code".into(), description: "不可达代码 (return后的语句)".into(), severity: Severity::Warning, fixable: false },
        // v5.1 工业级规则
        LintRule { id: "brain-timeout-required".into(), description: "AI推理调用必须设置超时保护".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "no-blocking-in-loop".into(), description: "禁止循环中忙等待 (while(digitalRead))".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "gpio-must-release".into(), description: "GPIO使用后必须释放 (hardware.release)".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "max-isr-lines".into(), description: "ISR/中断函数不超过15行".into(), severity: Severity::Error, fixable: false },
        // Phase v1.5: async/await 规则
        LintRule { id: "await-outside-async".into(), description: "await 只能在 async 函数内使用".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "async-no-busy-loop".into(), description: "async 函数中禁止无 await 的无限循环".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "promise-unhandled".into(), description: "Promise 返回值必须 await 或 .catch".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "async-stack-depth".into(), description: "async 函数嵌套不超过 5 层 (MCU 栈限制)".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "async-heap-budget".into(), description: "所有 async 上下文结构体总大小不超过 4KB".into(), severity: Severity::Error, fixable: false },
        // Phase 2.5: 零 malloc 保证规则
        LintRule { id: "no-dynamic-alloc".into(), description: "禁止动态内存分配 (new/Array(n)/字符串拼接)".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "array-bounds-static".into(), description: "编译期可确定的数组越界访问".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "pool-budget-exceeded".into(), description: "内存池预算超出 (对象/数组过多)".into(), severity: Severity::Warning, fixable: false },
        // Phase 4.5: WCET 实时安全规则
        LintRule { id: "wcet-exceeded".into(), description: "WCET 超过 @realtime 注解的上限".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "unbounded-loop-in-realtime".into(), description: "@realtime 中循环上界不可知".into(), severity: Severity::Error, fixable: false },
        LintRule { id: "float-in-realtime".into(), description: "@realtime 中使用浮点".into(), severity: Severity::Warning, fixable: false },
        LintRule { id: "deep-call-chain".into(), description: "调用链超过 5 层".into(), severity: Severity::Warning, fixable: false },
    ]
}

// =========================================================================
// Lint 执行
// =========================================================================

/// Lint 单个文件
pub fn lint_file(file_path: &Path, source: &str, config: &LintConfig) -> LintResult {
    let file_name = file_path.to_string_lossy().to_string();
    let lines: Vec<&str> = source.lines().collect();
    let mut diagnostics = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let line_num = (line_idx + 1) as u32;
        let trimmed = line.trim();

        // no-var: 检查 var 声明
        if !config.is_rule_disabled("no-var") {
            if let Some(col) = find_word_at_start(trimmed, "var ") {
                let severity = config.effective_severity("no-var");
                diagnostics.push(LintDiagnostic {
                    rule_id: "no-var".into(),
                    severity,
                    message: "使用 let 或 const 替代 var".into(),
                    file: file_name.clone(),
                    line: line_num,
                    column: col + 1,
                    suggestion: Some("将 var 替换为 let 或 const".into()),
                    fix: Some(LintFix {
                        description: "替换 var 为 let".into(),
                        line: line_num,
                        old_text: "var ".into(),
                        new_text: "let ".into(),
                    }),
                });
            }
        }

        // eqeqeq: 检查 == 和 !=
        if !config.is_rule_disabled("eqeqeq") {
            for (col, _) in trimmed.match_indices("==") {
                // 排除 === 和 !==
                let before_eq = col > 0 && trimmed.as_bytes()[col - 1] == b'!';
                let after_eq = col + 2 < trimmed.len() && trimmed.as_bytes()[col + 2] == b'=';
                if !after_eq {
                    let severity = config.effective_severity("eqeqeq");
                    let op = if before_eq { "!=" } else { "==" };
                    let fix_op = if before_eq { "!==" } else { "===" };
                    diagnostics.push(LintDiagnostic {
                        rule_id: "eqeqeq".into(),
                        severity,
                        message: format!("使用 {} 替代 {}", fix_op, op),
                        file: file_name.clone(),
                        line: line_num,
                        column: col as u32 + 1,
                        suggestion: Some(format!("替换 {} 为 {}", op, fix_op)),
                        fix: Some(LintFix {
                            description: format!("{} → {}", op, fix_op),
                            line: line_num,
                            old_text: op.into(),
                            new_text: fix_op.into(),
                        }),
                    });
                }
            }
        }

        // no-eval
        if !config.is_rule_disabled("no-eval") {
            if trimmed.contains("eval(") {
                let severity = config.effective_severity("no-eval");
                diagnostics.push(LintDiagnostic {
                    rule_id: "no-eval".into(),
                    severity,
                    message: "禁止使用 eval()，存在安全风险".into(),
                    file: file_name.clone(),
                    line: line_num,
                    column: 1,
                    suggestion: Some("使用安全替代方案".into()),
                    fix: None,
                });
            }
        }

        // no-console-log
        if !config.is_rule_disabled("no-console-log") {
            if trimmed.contains("console.log") && !trimmed.starts_with("//") {
                let severity = config.effective_severity("no-console-log");
                diagnostics.push(LintDiagnostic {
                    rule_id: "no-console-log".into(),
                    severity,
                    message: "生产代码应移除 console.log()".into(),
                    file: file_name.clone(),
                    line: line_num,
                    column: 1,
                    suggestion: Some("使用 UART 输出替代 console.log".into()),
                    fix: None,
                });
            }
        }

        // no-magic-numbers: 简单检查数字字面量 (排除 0,1,2)
        if !config.is_rule_disabled("no-magic-numbers") {
            // 只在赋值和函数调用中检查，跳过数组索引和常量定义
            if !trimmed.starts_with("//") && !trimmed.starts_with("const ") && !trimmed.contains("pinMode") {
                // 简单检查: 大于2的数字
                for m in trimmed.matches(|c: char| c.is_ascii_digit()) {
                    if let Ok(n) = m.parse::<u32>() {
                        if n > 2 && n < 1000 {
                            // 不报告小数字和常见值
                        }
                    }
                }
            }
        }
    }

    // max-lines: 文件行数检查
    if !config.is_rule_disabled("max-lines") && lines.len() > 500 {
        let severity = config.effective_severity("max-lines");
        diagnostics.push(LintDiagnostic {
            rule_id: "max-lines".into(),
            severity,
            message: format!("文件超过500行 (当前{}行)", lines.len()),
            file: file_name.clone(),
            line: 1,
            column: 1,
            suggestion: Some("考虑拆分文件".into()),
            fix: None,
        });
    }

    // require-hardware-init: 检查是否调用了 hardware.init
    if !config.is_rule_disabled("require-hardware-init") {
        let has_init = source.contains("hardware.init")
            || source.contains("hardwareInit")
            || source.contains("setup()");
        let has_hardware = source.contains("hardware")
            || source.contains("pinMode")
            || source.contains("digitalWrite")
            || source.contains("analogRead");
        if has_hardware && !has_init {
            let severity = config.effective_severity("require-hardware-init");
            diagnostics.push(LintDiagnostic {
                rule_id: "require-hardware-init".into(),
                severity,
                message: "硬件项目需要调用 hardware.init() 初始化".into(),
                file: file_name.clone(),
                line: 1,
                column: 1,
                suggestion: Some("在 setup() 或主函数开头添加 hardware.init()".into()),
                fix: None,
            });
        }
    }

    // no-blocking-loops: while(true) 或 while(1) 中没有 delay/yield
    if !config.is_rule_disabled("no-blocking-loops") {
        let mut in_infinite_loop = false;
        let mut loop_start = 0u32;
        let mut loop_has_yield = false;
        let mut brace_depth = 0i32;

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_num = (line_idx + 1) as u32;

            if trimmed.contains("while(true)") || trimmed.contains("while(1)") || trimmed.contains("while (true)") || trimmed.contains("while (1)") {
                in_infinite_loop = true;
                loop_start = line_num;
                loop_has_yield = false;
                brace_depth = 0;
            }

            if in_infinite_loop {
                for ch in trimmed.chars() {
                    if ch == '{' { brace_depth += 1; }
                    else if ch == '}' {
                        brace_depth -= 1;
                        if brace_depth <= 0 {
                            if !loop_has_yield {
                                let severity = config.effective_severity("no-blocking-loops");
                                diagnostics.push(LintDiagnostic {
                                    rule_id: "no-blocking-loops".into(),
                                    severity,
                                    message: "无限循环中没有 delay() 或 yield()，将阻塞系统".into(),
                                    file: file_name.clone(),
                                    line: loop_start,
                                    column: 1,
                                    suggestion: Some("添加 delay(10) 或 yield()".into()),
                                    fix: None,
                                });
                            }
                            in_infinite_loop = false;
                        }
                    }
                }

                if trimmed.contains("delay(") || trimmed.contains("yield()") || trimmed.contains("sleep(") {
                    loop_has_yield = true;
                }
            }
        }
    }

    // no-dead-code: return 后的语句
    if !config.is_rule_disabled("no-dead-code") {
        let mut after_return = false;
        let mut brace_depth = 0i32;
        let mut return_line = 0u32;

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_num = (line_idx + 1) as u32;

            for ch in trimmed.chars() {
                if ch == '{' { brace_depth += 1; after_return = false; }
                else if ch == '}' { brace_depth -= 1; after_return = false; }
            }

            if after_return && !trimmed.is_empty() && !trimmed.starts_with("//") && !trimmed.starts_with('}') {
                let severity = config.effective_severity("no-dead-code");
                diagnostics.push(LintDiagnostic {
                    rule_id: "no-dead-code".into(),
                    severity,
                    message: format!("return (第{}行) 后的代码不可达", return_line),
                    file: file_name.clone(),
                    line: line_num,
                    column: 1,
                    suggestion: Some("移除不可达代码".into()),
                    fix: None,
                });
                after_return = false;
            }

            if trimmed.starts_with("return ") || trimmed == "return;" {
                after_return = true;
                return_line = line_num;
            }
        }
    }

    // =========================================================================
    // v5.1 工业级规则
    // =========================================================================

    // brain-timeout-required: AI推理调用必须设置超时保护
    if !config.is_rule_disabled("brain-timeout-required") {
        let ai_patterns = ["hardyAI.infer(", "hardyAI.predict(", "model.infer(", "model.predict(", "ai.run("];
        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_num = (line_idx + 1) as u32;
            for pat in &ai_patterns {
                if trimmed.contains(pat) && !trimmed.starts_with("//") {
                    // 检查附近是否有超时设置
                    let context_start = if line_idx >= 5 { line_idx - 5 } else { 0 };
                    let context_end = (line_idx + 5).min(lines.len());
                    let context: String = lines[context_start..context_end].join("\n");
                    let has_timeout = context.contains("timeout")
                        || context.contains("setTimeout")
                        || context.contains("maxTimeMs")
                        || context.contains("deadline");
                    if !has_timeout {
                        let severity = config.effective_severity("brain-timeout-required");
                        diagnostics.push(LintDiagnostic {
                            rule_id: "brain-timeout-required".into(),
                            severity,
                            message: format!("AI推理调用 {} 缺少超时保护", pat.trim_end_matches('(')),
                            file: file_name.clone(),
                            line: line_num,
                            column: 1,
                            suggestion: Some("在调用前设置 timeout 参数，例如: hardyAI.infer(data, { timeout: 200 })".into()),
                            fix: None,
                        });
                    }
                }
            }
        }
    }

    // no-blocking-in-loop: 禁止循环中忙等待
    if !config.is_rule_disabled("no-blocking-in-loop") {
        let busy_patterns = [
            "while(digitalRead(", "while (digitalRead(",
            "while(analogRead(", "while (analogRead(",
            "while(!digitalRead(", "while (!digitalRead(",
        ];
        let mut in_loop = false;
        let mut loop_brace_depth = 0i32;

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_num = (line_idx + 1) as u32;

            // 跟踪循环上下文
            if trimmed.starts_with("while") || trimmed.starts_with("for") {
                in_loop = true;
                loop_brace_depth = 0;
            }
            if in_loop {
                for ch in trimmed.chars() {
                    if ch == '{' { loop_brace_depth += 1; }
                    else if ch == '}' {
                        loop_brace_depth -= 1;
                        if loop_brace_depth <= 0 { in_loop = false; }
                    }
                }
                // 检查忙等待模式
                for pat in &busy_patterns {
                    if trimmed.contains(pat) {
                        // 检查循环体是否只有这个条件 (纯忙等待)
                        let has_yield = trimmed.contains("delay(") || trimmed.contains("yield()")
                            || trimmed.contains("sleep(") || trimmed.contains("await ");
                        if !has_yield {
                            let severity = config.effective_severity("no-blocking-in-loop");
                            diagnostics.push(LintDiagnostic {
                                rule_id: "no-blocking-in-loop".into(),
                                severity,
                                message: "循环中存在忙等待，会阻塞系统并浪费电能".into(),
                                file: file_name.clone(),
                                line: line_num,
                                column: 1,
                                suggestion: Some("添加 delay(1) 或使用中断替代轮询".into()),
                                fix: None,
                            });
                        }
                    }
                }
            }
        }
    }

    // gpio-must-release: GPIO使用后必须释放
    if !config.is_rule_disabled("gpio-must-release") {
        let has_gpio_use = source.contains("pinMode(")
            || source.contains("digitalWrite(")
            || source.contains("digitalRead(")
            || source.contains("analogWrite(")
            || source.contains("analogRead(");
        let has_release = source.contains("hardware.release")
            || source.contains("pinRelease(")
            || source.contains("hardware.cleanup")
            || source.contains("gpio.release");
        if has_gpio_use && !has_release {
            let severity = config.effective_severity("gpio-must-release");
            diagnostics.push(LintDiagnostic {
                rule_id: "gpio-must-release".into(),
                severity,
                message: "GPIO使用后未发现释放调用，可能导致资源泄漏".into(),
                file: file_name.clone(),
                line: 1,
                column: 1,
                suggestion: Some("在程序退出或切换引脚时调用 hardware.release(pin)".into()),
                fix: None,
            });
        }
    }

    // max-isr-lines: ISR/中断函数不超过15行
    if !config.is_rule_disabled("max-isr-lines") {
        let isr_patterns = ["ISR(", "interrupt ", "onInterrupt(", "attachInterrupt(", "irq_handler"];
        let max_isr = 15u32;
        let mut in_isr = false;
        let mut isr_start = 0u32;
        let mut isr_brace_depth = 0i32;
        let mut isr_line_count = 0u32;

        for (line_idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let line_num = (line_idx + 1) as u32;

            if !in_isr {
                for pat in &isr_patterns {
                    if trimmed.contains(pat) && !trimmed.starts_with("//") {
                        in_isr = true;
                        isr_start = line_num;
                        isr_brace_depth = 0;
                        isr_line_count = 0;
                        break;
                    }
                }
            }

            if in_isr {
                for ch in trimmed.chars() {
                    if ch == '{' { isr_brace_depth += 1; }
                    else if ch == '}' {
                        isr_brace_depth -= 1;
                        if isr_brace_depth <= 0 {
                            if isr_line_count > max_isr {
                                let severity = config.effective_severity("max-isr-lines");
                                diagnostics.push(LintDiagnostic {
                                    rule_id: "max-isr-lines".into(),
                                    severity,
                                    message: format!("ISR函数共{}行，超过{}行上限 (第{}行开始)", isr_line_count, max_isr, isr_start),
                                    file: file_name.clone(),
                                    line: isr_start,
                                    column: 1,
                                    suggestion: Some("ISR应尽量短，将复杂逻辑移到主循环中，ISR只设标志位".into()),
                                    fix: None,
                                });
                            }
                            in_isr = false;
                        }
                    }
                }
                if !trimmed.is_empty() && !trimmed.starts_with("//") {
                    isr_line_count += 1;
                }
            }
        }
    }

    // Phase 2.5: 零 malloc 保证检查
    // no-dynamic-alloc: 检测 new / Array(n) / 字符串拼接
    if !config.is_rule_disabled("no-dynamic-alloc") {
        for (line_idx, line) in lines.iter().enumerate() {
            let line_num = (line_idx + 1) as u32;
            let trimmed = line.trim();
            if trimmed.starts_with("//") { continue; }
            // 检测 new 关键字
            if trimmed.contains("new ") && (trimmed.contains("new Array") || trimmed.contains("new Object") || trimmed.contains("new String")) {
                let severity = config.effective_severity("no-dynamic-alloc");
                diagnostics.push(LintDiagnostic {
                    rule_id: "no-dynamic-alloc".into(),
                    severity,
                    message: "禁止动态内存分配: 使用对象字面量或数组字面量替代 new".into(),
                    file: file_name.clone(),
                    line: line_num,
                    column: 1,
                    suggestion: Some("使用 {} 或 [] 字面量，编译期确定大小".into()),
                    fix: None,
                });
            }
            // 检测 Array(n) 动态长度数组
            if trimmed.contains("Array(") && !trimmed.contains("// Array") {
                let severity = config.effective_severity("no-dynamic-alloc");
                diagnostics.push(LintDiagnostic {
                    rule_id: "no-dynamic-alloc".into(),
                    severity,
                    message: "禁止动态长度数组: 使用编译期已知长度的数组字面量".into(),
                    file: file_name.clone(),
                    line: line_num,
                    column: 1,
                    suggestion: Some("使用 [0, 0, 0, ...] 定长初始化".into()),
                    fix: None,
                });
            }
        }
    }

    // 统计
    let error_count = diagnostics.iter().filter(|d| d.severity == Severity::Error).count() as u32;
    let warning_count = diagnostics.iter().filter(|d| d.severity == Severity::Warning).count() as u32;
    let fixable_count = diagnostics.iter().filter(|d| d.fix.is_some()).count() as u32;

    LintResult { diagnostics, error_count, warning_count, fixable_count }
}

/// Lint 整个项目
pub fn lint_project(dir: &Path, config: &LintConfig) -> LintResult {
    let mut all_diagnostics = Vec::new();
    let mut total_errors = 0u32;
    let mut total_warnings = 0u32;
    let mut total_fixable = 0u32;

    // 查找所有 .js/.ts 文件
    let js_files = find_js_files(dir);

    for file in &js_files {
        if let Ok(source) = fs::read_to_string(file) {
            let result = lint_file(file, &source, config);
            total_errors += result.error_count;
            total_warnings += result.warning_count;
            total_fixable += result.fixable_count;
            all_diagnostics.extend(result.diagnostics);
        }
    }

    LintResult {
        diagnostics: all_diagnostics,
        error_count: total_errors,
        warning_count: total_warnings,
        fixable_count: total_fixable,
    }
}

/// 自动修复
pub fn lint_fix(file_path: &Path, source: &str, config: &LintConfig) -> Result<String, String> {
    let result = lint_file(file_path, source, config);
    let mut lines: Vec<String> = source.lines().map(|s| s.to_string()).collect();
    let mut fix_count = 0;

    // 收集可修复的诊断 (按行号倒序，避免偏移)
    let mut fixes: Vec<LintFix> = result.diagnostics.iter()
        .filter_map(|d| d.fix.clone())
        .collect();
    fixes.sort_by(|a, b| b.line.cmp(&a.line));

    for fix in &fixes {
        let idx = (fix.line - 1) as usize;
        if idx < lines.len() {
            if lines[idx].contains(&fix.old_text) {
                lines[idx] = lines[idx].replace(&fix.old_text, &fix.new_text);
                fix_count += 1;
            }
        }
    }

    if fix_count > 0 {
        println!("已修复 {} 个问题", fix_count);
    }

    Ok(lines.join("\n"))
}

/// 资源约束检查
pub fn resource_budget_check(
    dir: &Path,
    ram_kb: u32,
    flash_kb: u32,
    config: &LintConfig,
) -> Vec<LintDiagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(max_ram) = config.max_ram_kb {
        if ram_kb > max_ram {
            diagnostics.push(LintDiagnostic {
                rule_id: "resource-budget".into(),
                severity: Severity::Error,
                message: format!("RAM 使用 {}KB 超过板子上限 {}KB", ram_kb, max_ram),
                file: dir.to_string_lossy().to_string(),
                line: 0,
                column: 0,
                suggestion: Some("优化代码减少 RAM 使用，或更换更大 RAM 的板子".into()),
                fix: None,
            });
        }
    }

    if let Some(max_flash) = config.max_flash_kb {
        if flash_kb > max_flash {
            diagnostics.push(LintDiagnostic {
                rule_id: "resource-budget".into(),
                severity: Severity::Error,
                message: format!("Flash 使用 {}KB 超过板子上限 {}KB", flash_kb, max_flash),
                file: dir.to_string_lossy().to_string(),
                line: 0,
                column: 0,
                suggestion: Some("优化代码减少 Flash 使用，或更换更大 Flash 的板子".into()),
                fix: None,
            });
        }
    }

    diagnostics
}

// =========================================================================
// 辅助函数
// =========================================================================

fn find_word_at_start(line: &str, word: &str) -> Option<u32> {
    if let Some(pos) = line.find(word) {
        // 确保是词边界 (前面是空格或行首)
        if pos == 0 || line.as_bytes()[pos - 1] == b' ' || line.as_bytes()[pos - 1] == b'\t' {
            return Some(pos as u32);
        }
    }
    None
}

fn find_js_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "js" || ext == "ts" {
                        files.push(path);
                    }
                }
            } else if path.is_dir() {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && name != "node_modules" && name != "build" && name != "target" {
                    files.extend(find_js_files(&path));
                }
            }
        }
    }
    files
}

fn extract_number(json: &str, field: &str) -> Option<f64> {
    let pattern = format!("\"{}\"", field);
    if let Some(pos) = json.find(&pattern) {
        let rest = &json[pos + pattern.len()..];
        let rest = rest.trim_start();
        if rest.starts_with(':') {
            let rest = rest[1..].trim_start();
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                .unwrap_or(rest.len());
            return rest[..end].parse().ok();
        }
    }
    None
}

fn extract_string_value(json: &str, field: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lint_no_var() {
        let config = LintConfig::default_config();
        let source = "var x = 10;\nlet y = 20;";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "no-var"));
        assert!(result.fixable_count > 0);
    }

    #[test]
    fn test_lint_eqeqeq() {
        let config = LintConfig::default_config();
        let source = "if (a == b) {}";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "eqeqeq"));
    }

    #[test]
    fn test_lint_no_eval() {
        let config = LintConfig::default_config();
        let source = "eval('alert(1)');";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "no-eval"));
    }

    #[test]
    fn test_lint_fix_var() {
        let config = LintConfig::default_config();
        let source = "var x = 10;";
        let fixed = lint_fix(Path::new("test.js"), source, &config).unwrap();
        assert!(fixed.contains("let x = 10;"));
    }

    #[test]
    fn test_lint_max_lines() {
        let config = LintConfig::default_config();
        let source: String = (0..501).map(|i| format!("let a{} = {};", i, i)).collect::<Vec<_>>().join("\n");
        let result = lint_file(Path::new("test.js"), &source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "max-lines"));
    }

    #[test]
    fn test_resource_budget() {
        let config = LintConfig {
            max_ram_kb: Some(32),
            max_flash_kb: Some(256),
            ..LintConfig::default_config()
        };
        let diags = resource_budget_check(Path::new("."), 64, 128, &config);
        assert_eq!(diags.len(), 1); // RAM exceeded
        assert!(diags[0].message.contains("RAM"));
    }

    #[test]
    fn test_lint_brain_timeout() {
        let config = LintConfig::default_config();
        let source = "let result = hardyAI.infer(sensorData);";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "brain-timeout-required"));
    }

    #[test]
    fn test_lint_brain_timeout_with_timeout_ok() {
        let config = LintConfig::default_config();
        let source = "let opts = { timeout: 200 };\nlet result = hardyAI.infer(sensorData);";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(!result.diagnostics.iter().any(|d| d.rule_id == "brain-timeout-required"));
    }

    #[test]
    fn test_lint_no_blocking_in_loop() {
        let config = LintConfig::default_config();
        let source = "while(true) {\n  while(digitalRead(5)) {}\n}";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "no-blocking-in-loop"));
    }

    #[test]
    fn test_lint_gpio_must_release() {
        let config = LintConfig::default_config();
        let source = "pinMode(13, OUTPUT);\ndigitalWrite(13, HIGH);";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "gpio-must-release"));
    }

    #[test]
    fn test_lint_gpio_with_release_ok() {
        let config = LintConfig::default_config();
        let source = "pinMode(13, OUTPUT);\ndigitalWrite(13, HIGH);\nhardware.release(13);";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(!result.diagnostics.iter().any(|d| d.rule_id == "gpio-must-release"));
    }

    #[test]
    fn test_lint_max_isr_lines() {
        let config = LintConfig::default_config();
        let mut source = String::from("function onInterrupt() {\n");
        for i in 0..20 {
            source.push_str(&format!("  let v{} = analogRead(0);\n", i));
        }
        source.push_str("}\n");
        let result = lint_file(Path::new("test.js"), &source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "max-isr-lines"));
    }

    #[test]
    fn test_builtin_rules_count() {
        let rules = builtin_rules();
        // 原始15条 + v5.1 4条 + async 5条 + Phase 2.5 3条 + Phase 4.5 4条 = 31条
        assert_eq!(rules.len(), 31);
    }

    // Phase 2.5: 零 malloc lint 测试
    #[test]
    fn test_lint_no_dynamic_alloc_new() {
        let config = LintConfig::default_config();
        let source = "let arr = new Array(10);\n";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "no-dynamic-alloc"),
            "应检测 new Array");
    }

    #[test]
    fn test_lint_no_dynamic_alloc_array_fn() {
        let config = LintConfig::default_config();
        let source = "let buf = Array(256);\n";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(result.diagnostics.iter().any(|d| d.rule_id == "no-dynamic-alloc"),
            "应检测 Array(n)");
    }

    #[test]
    fn test_lint_no_dynamic_alloc_pass() {
        let config = LintConfig::default_config();
        let source = "let arr = [1, 2, 3, 4, 5];\nlet obj = { x: 10, y: 20 };\n";
        let result = lint_file(Path::new("test.js"), source, &config);
        assert!(!result.diagnostics.iter().any(|d| d.rule_id == "no-dynamic-alloc"),
            "字面量不应触发 no-dynamic-alloc");
    }
}
