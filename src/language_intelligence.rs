//! DUANYAN 语言智能引擎
//!
//! 为 IDE 提供 DUANYAN 专属智能服务：
//! - 实时诊断 (语法/语义/确定性违规)
//! - 上下文补全 (关键字/硬件API/数学函数/行业API)
//! - 悬停信息 (函数签名/变量类型/WCET标注)
//! - 行业规则检查 (9大行业确定性约束)

// =============================================================================
// 诊断
// =============================================================================

/// 诊断严重级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

/// 单条诊断
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub line: usize,
    pub col: usize,
    pub message: String,
    pub code: String,
}

/// 补全项
#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: String,
    pub insert_text: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompletionKind {
    Keyword,
    Function,
    HardwareApi,
    MathApi,
    IndustryApi,
    Variable,
    Snippet,
}

/// 悬停信息
#[derive(Debug, Clone)]
pub struct HoverInfo {
    pub title: String,
    pub signature: String,
    pub description: String,
    pub wcet_hint: Option<String>,
}

// =============================================================================
// 诊断引擎
// =============================================================================

/// 对源码执行全量诊断
pub fn diagnose(source: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    // 文本级检查 (不依赖解析)
    check_common_mistakes(source, &mut diags);

    // Phase 1: 词法检查
    match crate::lexer::tokenize(source) {
        Ok(tokens) => {
            // Phase 2: 语法检查
            match crate::parser::parse(&tokens) {
                Ok(program) => {
                    // Phase 3: 语义检查
                    check_determinism(source, &program, &mut diags);
                    check_realtime_annotations(&program, &mut diags);
                }
                Err(e) => {
                    let line = extract_line_from_error(&e);
                    diags.push(Diagnostic {
                        severity: Severity::Error,
                        line,
                        col: 1,
                        message: format!("语法错误: {}", e),
                        code: "HS001".into(),
                    });
                }
            }
        }
        Err(e) => {
            diags.push(Diagnostic {
                severity: Severity::Error,
                line: 1,
                col: 1,
                message: format!("词法错误: {}", e),
                code: "HS000".into(),
            });
        }
    }

    diags
}

/// 确定性违规检查
fn check_determinism(source: &str, program: &crate::parser::Program, diags: &mut Vec<Diagnostic>) {
    for line in source.lines().enumerate() {
        let (i, text) = (line.0 + 1, line.1.trim());

        // 禁止 while(true) 无上界
        if text.contains("while (true)") || text.contains("while(true)") {
            let has_break = source.lines().skip(i).take(50).any(|l| l.trim().starts_with("break"));
            if !has_break {
                diags.push(Diagnostic {
                    severity: Severity::Warning,
                    line: i,
                    col: 1,
                    message: "while(true) 无 break: WCET 不可计算，违反确定性约束".into(),
                    code: "HS101".into(),
                });
            }
        }

        // 警告: 浮点除法
        if text.contains('/') && !text.starts_with("//") && !text.contains("/*") {
            if text.chars().any(|c| c.is_ascii_digit()) && text.contains(" / ") {
                diags.push(Diagnostic {
                    severity: Severity::Info,
                    line: i,
                    col: 1,
                    message: "浮点除法: 确认目标芯片有 FPU，否则 WCET 将显著增加".into(),
                    code: "HS105".into(),
                });
            }
        }
    }

    // 函数级检查
    for sf in &program.functions {
        for stmt in &sf.func.body {
            if let crate::parser::Statement::For { .. } = &stmt.stmt {
                // For 循环通常有界，OK
            }
        }
    }
}

/// 实时注解检查
fn check_realtime_annotations(program: &crate::parser::Program, diags: &mut Vec<Diagnostic>) {
    for sf in &program.functions {
        // 有 @realtime 但无 wcet_limit
        if sf.func.is_realtime && sf.func.wcet_limit_us.is_none() {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                line: sf.line,
                col: 1,
                message: format!("@realtime 函数 '{}' 缺少 wcet_limit_us 约束", sf.func.name),
                code: "HS201".into(),
            });
        }

        // 有 @interrupt 但无 period
        if sf.func.interrupt_timer.is_some() && sf.func.interrupt_period_us.is_none() {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                line: sf.line,
                col: 1,
                message: format!("@interrupt 函数 '{}' 缺少 period_us 参数", sf.func.name),
                code: "HS202".into(),
            });
        }
    }
}

/// 常见错误检查 (文本级, 不依赖解析)
fn check_common_mistakes(source: &str, diags: &mut Vec<Diagnostic>) {
    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        let line_num = i + 1;

        // 禁止 new (无动态分配)
        if trimmed.starts_with("new ") || trimmed.contains("= new ") {
            diags.push(Diagnostic {
                severity: Severity::Error,
                line: line_num,
                col: 1,
                message: "禁止 new: DUANYAN 无堆分配，使用 let 声明栈变量".into(),
                code: "HS102".into(),
            });
        }

        // 禁止 setTimeout/setInterval
        if trimmed.contains("setTimeout") || trimmed.contains("setInterval") {
            diags.push(Diagnostic {
                severity: Severity::Error,
                line: line_num,
                col: 1,
                message: "禁止 setTimeout/setInterval: 使用 @interrupt 注解实现定时".into(),
                code: "HS103".into(),
            });
        }

        // console.log 提示
        if trimmed.contains("console.log") {
            diags.push(Diagnostic {
                severity: Severity::Hint,
                line: line_num,
                col: 1,
                message: "console.log 将被编译为 uartSend 调试输出 (可通过 --strip 移除)".into(),
                code: "HS104".into(),
            });
        }

        // == vs ===
        if trimmed.contains("===") {
            diags.push(Diagnostic {
                severity: Severity::Hint,
                line: line_num,
                col: 1,
                message: "DUANYAN 中 == 即为严格相等，无需 ===".into(),
                code: "HS301".into(),
            });
        }

        // var
        if trimmed.starts_with("var ") {
            diags.push(Diagnostic {
                severity: Severity::Warning,
                line: line_num,
                col: 1,
                message: "建议用 let/const 替代 var (DUANYAN 无变量提升)".into(),
                code: "HS302".into(),
            });
        }

        // 空语句
        if trimmed == ";" {
            diags.push(Diagnostic {
                severity: Severity::Hint,
                line: line_num,
                col: 1,
                message: "空语句".into(),
                code: "HS303".into(),
            });
        }
    }
}

fn extract_line_from_error(err: &str) -> usize {
    // 尝试从错误消息中提取行号
    if let Some(pos) = err.find("line ") {
        let rest = &err[pos + 5..];
        if let Some(num_str) = rest.split_whitespace().next() {
            if let Ok(n) = num_str.parse::<usize>() {
                return n;
            }
        }
    }
    1
}

// =============================================================================
// 补全引擎
// =============================================================================

/// 硬件 API 定义
const HARDWARE_APIS: &[(&str, &str, &str)] = &[
    ("pinMode", "pinMode(pin, mode)", "设置引脚模式: \"input\" | \"output\" | \"analog\""),
    ("digitalWrite", "digitalWrite(pin, value)", "数字输出: value = 0 | 1"),
    ("digitalRead", "digitalRead(pin)", "数字输入 → 0 | 1"),
    ("analogRead", "analogRead(pin)", "ADC 读取 → 0~4095 (12-bit)"),
    ("analogWrite", "analogWrite(pin, duty)", "PWM 输出: duty = 0~255"),
    ("delay", "delay(ms)", "阻塞延时 (毫秒)"),
    ("delayMicroseconds", "delayMicroseconds(us)", "阻塞延时 (微秒)"),
    ("millis", "millis()", "系统运行时间 (ms)"),
    ("micros", "micros()", "系统运行时间 (μs)"),
    ("uartSend", "uartSend(data)", "UART 发送"),
    ("uartRead", "uartRead()", "UART 接收 → byte | -1"),
    ("spiTransfer", "spiTransfer(data)", "SPI 全双工传输"),
    ("i2cWrite", "i2cWrite(addr, data)", "I2C 写"),
    ("i2cRead", "i2cRead(addr, len)", "I2C 读"),
    ("servoWrite", "servoWrite(pin, angle)", "舵机控制: angle = 0~180"),
    ("tone", "tone(pin, freq, duration)", "蜂鸣器: freq(Hz), duration(ms)"),
    ("attachInterrupt", "attachInterrupt(pin, callback, mode)", "外部中断绑定"),
];

/// 数学 API
const MATH_APIS: &[(&str, &str, &str)] = &[
    ("Math.abs", "Math.abs(x)", "绝对值"),
    ("Math.min", "Math.min(a, b)", "最小值"),
    ("Math.max", "Math.max(a, b)", "最大值"),
    ("Math.floor", "Math.floor(x)", "向下取整"),
    ("Math.ceil", "Math.ceil(x)", "向上取整"),
    ("Math.round", "Math.round(x)", "四舍五入"),
    ("Math.sqrt", "Math.sqrt(x)", "平方根 (需 FPU)"),
    ("Math.sin", "Math.sin(x)", "正弦 (需 FPU, 或表驱动)"),
    ("Math.cos", "Math.cos(x)", "余弦 (需 FPU, 或表驱动)"),
    ("Math.atan2", "Math.atan2(y, x)", "反正切 (需 FPU)"),
    ("Math.pow", "Math.pow(base, exp)", "幂运算"),
    ("Math.random", "Math.random()", "⚠ 非确定性! 嵌入式禁用"),
];

/// 关键字
const KEYWORDS: &[&str] = &[
    "let", "const", "function", "return", "if", "else", "while", "for",
    "break", "continue", "true", "false", "async", "await",
];

/// 行业特定 API (根据 target)
fn industry_apis(target: &str) -> Vec<(&'static str, String, String)> {
    match target {
        "ic" => vec![
            ("plcCycle", "plcCycle(us)".into(), "PLC 扫描周期设置".into()),
            ("ecatSend", "ecatSend(slave, data)".into(), "EtherCAT 过程数据发送".into()),
            ("safetyVote", "safetyVote(ch1, ch2)".into(), "SIL-3 双通道表决".into()),
        ],
        "ad" => vec![
            ("canSend", "canSend(id, data)".into(), "CAN FD 消息发送".into()),
            ("asilCheck", "asilCheck(level)".into(), "ASIL 等级校验".into()),
        ],
        "rb" => vec![
            ("jointMove", "jointMove(id, angle, speed)".into(), "关节运动指令".into()),
            ("forceRead", "forceRead(axis)".into(), "力矩传感器读取".into()),
            ("impedanceCtrl", "impedanceCtrl(M, D, K)".into(), "阻抗控制参数".into()),
        ],
        "md" => vec![
            ("alarmSet", "alarmSet(condition, level)".into(), "医疗报警设置".into()),
            ("safeShutdown", "safeShutdown()".into(), "安全关机序列".into()),
        ],
        _ => vec![],
    }
}

/// 上下文补全
pub fn complete(source: &str, line: usize, col: usize, target: Option<&str>) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    // 获取当前行前缀
    let current_line = source.lines().nth(line.saturating_sub(1)).unwrap_or("");
    let prefix = if col <= current_line.len() {
        &current_line[..col.min(current_line.len())]
    } else {
        current_line
    };
    let word = prefix.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '_')
        .last()
        .unwrap_or("");

    // 关键字补全
    for kw in KEYWORDS {
        if kw.starts_with(word) && !word.is_empty() {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: CompletionKind::Keyword,
                detail: "关键字".into(),
                insert_text: kw.to_string(),
            });
        }
    }

    // 硬件 API 补全
    for (name, sig, desc) in HARDWARE_APIS {
        if name.starts_with(word) && !word.is_empty() {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: CompletionKind::HardwareApi,
                detail: format!("{} — {}", sig, desc),
                insert_text: format!("{}(${{}})", name),
            });
        }
    }

    // 数学 API 补全 (当输入 "Math" 或 "M" 时)
    if word.starts_with('M') || word.starts_with("Math") {
        for (name, sig, desc) in MATH_APIS {
            if name.starts_with(word) || name.to_lowercase().starts_with(&word.to_lowercase()) {
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: CompletionKind::MathApi,
                    detail: format!("{} — {}", sig, desc),
                    insert_text: format!("{}(${{}})", name),
                });
            }
        }
    }

    // 行业 API 补全
    if let Some(t) = target {
        for (name, sig, desc) in industry_apis(t) {
            if name.starts_with(word) && !word.is_empty() {
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: CompletionKind::IndustryApi,
                    detail: format!("{} — {}", sig, desc),
                    insert_text: format!("{}(${{}})", name),
                });
            }
        }
    }

    // 代码片段补全
    if word.is_empty() || "for".starts_with(word) {
        items.push(CompletionItem {
            label: "for (bounded)".into(),
            kind: CompletionKind::Snippet,
            detail: "有界 for 循环 (WCET 可计算)".into(),
            insert_text: "for (let i = 0; i < ${1:N}; i++) {\n    $0\n}".into(),
        });
    }
    if word.is_empty() || "function".starts_with(word) {
        items.push(CompletionItem {
            label: "function @realtime".into(),
            kind: CompletionKind::Snippet,
            detail: "实时函数 (带 WCET 约束)".into(),
            insert_text: "// @realtime\n// @wcet(100)\nfunction ${1:name}() {\n    $0\n}".into(),
        });
    }
    if word.is_empty() || "interrupt".starts_with(word) {
        items.push(CompletionItem {
            label: "@interrupt handler".into(),
            kind: CompletionKind::Snippet,
            detail: "定时器中断处理函数".into(),
            insert_text: "// @interrupt(\"TIM2\", period_us: 1000)\nfunction ${1:isr}() {\n    $0\n}".into(),
        });
    }

    items
}

// =============================================================================
// 悬停信息
// =============================================================================

/// 获取悬停信息
pub fn hover(source: &str, line: usize, col: usize) -> Option<HoverInfo> {
    let current_line = source.lines().nth(line.saturating_sub(1))?;
    let word = extract_word_at(current_line, col)?;

    // 硬件 API
    for (name, sig, desc) in HARDWARE_APIS {
        if *name == word {
            return Some(HoverInfo {
                title: name.to_string(),
                signature: sig.to_string(),
                description: desc.to_string(),
                wcet_hint: Some(match *name {
                    "digitalWrite" | "digitalRead" => "1~3 cycles (GPIO 直写)",
                    "analogRead" => "12~84 cycles (取决于采样时间配置)",
                    "delay" => "精确: cycles = ms × (CPU_FREQ / 1000)",
                    "uartSend" => "取决于波特率: 10 bits × (1/baud)",
                    _ => "外设相关，参见芯片手册",
                }.into()),
            });
        }
    }

    // 数学 API
    for (name, sig, desc) in MATH_APIS {
        if *name == word || word == name.split('.').last().unwrap_or("") {
            return Some(HoverInfo {
                title: name.to_string(),
                signature: sig.to_string(),
                description: desc.to_string(),
                wcet_hint: if name.contains("sin") || name.contains("cos") || name.contains("atan") {
                    Some("⚠ 高开销: 无 FPU 时 200~500 cycles, 建议表驱动".into())
                } else if name.contains("sqrt") || name.contains("pow") {
                    Some("需 FPU: 有 FPU 约 14 cycles, 无 FPU 约 100+ cycles".into())
                } else {
                    Some("低开销: 1~5 cycles".into())
                },
            });
        }
    }

    // 关键字
    match word.as_str() {
        "let" => Some(HoverInfo {
            title: "let".into(),
            signature: "let name = value;".into(),
            description: "声明栈上变量 (编译期分配, 无堆)".into(),
            wcet_hint: Some("0 cycles (编译期决定)".into()),
        }),
        "while" => Some(HoverInfo {
            title: "while".into(),
            signature: "while (condition) { ... }".into(),
            description: "循环. ⚠ 必须有确定上界或 break, 否则 WCET 不可计算".into(),
            wcet_hint: Some("WCET = 最大迭代次数 × 单次循环体".into()),
        }),
        "for" => Some(HoverInfo {
            title: "for".into(),
            signature: "for (let i = 0; i < N; i++) { ... }".into(),
            description: "有界循环 (推荐: WCET 自动计算)".into(),
            wcet_hint: Some("WCET = N × 循环体 cycles".into()),
        }),
        _ => None,
    }
}

fn extract_word_at(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if col == 0 || col > bytes.len() {
        return None;
    }
    let mut start = col.saturating_sub(1);
    let mut end = col.saturating_sub(1);

    while start > 0 && (bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_' || bytes[start - 1] == b'.') {
        start -= 1;
    }
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'.') {
        end += 1;
    }

    if start == end {
        return None;
    }
    Some(line[start..end].to_string())
}

// =============================================================================
// 行业规则检查
// =============================================================================

/// 针对特定行业的额外规则检查
pub fn check_industry_rules(source: &str, target: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    match target {
        "md" => {
            // 医疗: 禁止无保护的除法 (除零风险)
            for (i, line) in source.lines().enumerate() {
                if line.contains(" / ") && !line.contains("// ") {
                    diags.push(Diagnostic {
                        severity: Severity::Warning,
                        line: i + 1,
                        col: 1,
                        message: "[IEC 62304] 除法操作: 需确保除数非零 (Class C 强制)".into(),
                        code: "MD001".into(),
                    });
                }
            }
        }
        "rw" => {
            // 轨交: 所有函数必须有 SIL 标注
            if !source.contains("@sil") && !source.contains("@realtime") {
                diags.push(Diagnostic {
                    severity: Severity::Warning,
                    line: 1,
                    col: 1,
                    message: "[EN 50128] 未检测到安全完整性标注 (@sil / @realtime)".into(),
                    code: "RW001".into(),
                });
            }
        }
        "av" => {
            // 航空: 禁止浮点 (除非显式声明)
            if source.contains("Math.sin") || source.contains("Math.cos") {
                if !source.contains("@float_deterministic") {
                    diags.push(Diagnostic {
                        severity: Severity::Warning,
                        line: 1,
                        col: 1,
                        message: "[DO-178C] 浮点三角函数: 需声明 @float_deterministic 确保位精确".into(),
                        code: "AV001".into(),
                    });
                }
            }
        }
        "ad" => {
            // 汽车: 禁止无 ASIL 标注的中断
            if source.contains("@interrupt") && !source.contains("@asil") {
                diags.push(Diagnostic {
                    severity: Severity::Info,
                    line: 1,
                    col: 1,
                    message: "[ISO 26262] 中断处理函数建议标注 @asil 等级".into(),
                    code: "AD001".into(),
                });
            }
        }
        "se" => {
            // 半导体: 伺服环周期必须声明
            if source.contains("servo") && !source.contains("@period") {
                diags.push(Diagnostic {
                    severity: Severity::Info,
                    line: 1,
                    col: 1,
                    message: "[SE] 伺服控制函数建议标注 @period 确保确定性周期".into(),
                    code: "SE001".into(),
                });
            }
        }
        _ => {}
    }

    diags
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnose_clean_code() {
        let source = "let x = 42;\nlet y = x + 1;";
        let diags = diagnose(source);
        // 可能有浮点除法的 info, 但不应有 error
        assert!(diags.iter().all(|d| d.severity != Severity::Error));
    }

    #[test]
    fn test_diagnose_while_true() {
        let source = "while (true) {\n  let x = 1;\n}";
        let diags = diagnose(source);
        assert!(diags.iter().any(|d| d.code == "HS101"));
    }

    #[test]
    fn test_diagnose_new_forbidden() {
        let source = "let obj = new Object();";
        let diags = diagnose(source);
        assert!(diags.iter().any(|d| d.code == "HS102"));
    }

    #[test]
    fn test_diagnose_settimeout() {
        let source = "setTimeout(() => {}, 100);";
        let diags = diagnose(source);
        assert!(diags.iter().any(|d| d.code == "HS103"));
    }

    #[test]
    fn test_complete_hardware() {
        let items = complete("dig", 1, 3, None);
        assert!(items.iter().any(|i| i.label == "digitalWrite"));
        assert!(items.iter().any(|i| i.label == "digitalRead"));
    }

    #[test]
    fn test_complete_math() {
        let items = complete("Math.s", 1, 6, None);
        assert!(items.iter().any(|i| i.label == "Math.sin"));
        assert!(items.iter().any(|i| i.label == "Math.sqrt"));
    }

    #[test]
    fn test_complete_industry() {
        let items = complete("can", 1, 3, Some("ad"));
        assert!(items.iter().any(|i| i.label == "canSend"));
    }

    #[test]
    fn test_hover_hardware() {
        let info = hover("digitalWrite(13, 1);", 1, 5);
        assert!(info.is_some());
        let info = info.unwrap();
        assert_eq!(info.title, "digitalWrite");
        assert!(info.wcet_hint.is_some());
    }

    #[test]
    fn test_hover_keyword() {
        let info = hover("while (x > 0) {}", 1, 3);
        assert!(info.is_some());
        assert!(info.unwrap().description.contains("WCET"));
    }

    #[test]
    fn test_industry_rules_md() {
        let diags = check_industry_rules("let r = a / b;", "md");
        assert!(diags.iter().any(|d| d.code == "MD001"));
    }

    #[test]
    fn test_industry_rules_rw() {
        let diags = check_industry_rules("let x = 1;", "rw");
        assert!(diags.iter().any(|d| d.code == "RW001"));
    }

    #[test]
    fn test_industry_rules_av() {
        let diags = check_industry_rules("let y = Math.sin(x);", "av");
        assert!(diags.iter().any(|d| d.code == "AV001"));
    }
}
