//! Phase EDU: 教育平台垂直模块
//!
//! DUANYAN 天然教育产品："用 JavaScript 写嵌入式"
//! - 渐进式课程生成 (LED → 传感器 → RTOS → 通信)
//! - 编译可视化 (JS → AST → IR → ASM 逐步展示)
//! - 交互式实验 (硬件仿真 + 即时反馈)
//! - 能力评估 (自动出题 + 代码评判)

// =============================================================================
// 课程体系
// =============================================================================

/// 课程难度级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CourseLevel {
    /// L1: 入门 (LED 闪烁、数字 IO)
    Beginner,
    /// L2: 基础 (ADC、PWM、定时器)
    Elementary,
    /// L3: 进阶 (中断、DMA、通信协议)
    Intermediate,
    /// L4: 高级 (RTOS、多任务、文件系统)
    Advanced,
    /// L5: 专家 (确定性、WCET、安全认证)
    Expert,
}

impl CourseLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Beginner => "L1 入门",
            Self::Elementary => "L2 基础",
            Self::Intermediate => "L3 进阶",
            Self::Advanced => "L4 高级",
            Self::Expert => "L5 专家",
        }
    }

    pub fn target_hours(&self) -> u32 {
        match self {
            Self::Beginner => 8,
            Self::Elementary => 16,
            Self::Intermediate => 24,
            Self::Advanced => 32,
            Self::Expert => 40,
        }
    }
}

/// 课程单元
#[derive(Debug, Clone)]
pub struct LessonUnit {
    /// 标题
    pub title: String,
    /// 级别
    pub level: CourseLevel,
    /// 学习目标
    pub objectives: Vec<String>,
    /// 示例代码 (DUANYAN JS)
    pub example_code: String,
    /// 练习题
    pub exercises: Vec<Exercise>,
    /// 涉及硬件外设
    pub peripherals: Vec<String>,
}

/// 练习题
#[derive(Debug, Clone)]
pub struct Exercise {
    /// 题目描述
    pub description: String,
    /// 难度 (1-5)
    pub difficulty: u8,
    /// 预期输出/行为
    pub expected_behavior: String,
    /// 提示
    pub hints: Vec<String>,
}

// =============================================================================
// 编译可视化
// =============================================================================

/// 编译阶段 (用于教学展示)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileStage {
    /// 源代码
    Source,
    /// 词法分析 → Token 流
    Lexer,
    /// 语法分析 → AST
    Parser,
    /// 中间表示 → IR
    IrGen,
    /// 优化 → 优化后 IR
    Optimizer,
    /// 代码生成 → 目标代码
    CodeGen,
    /// 链接 → 最终二进制
    Linker,
}

impl CompileStage {
    pub fn all() -> &'static [CompileStage] {
        &[Self::Source, Self::Lexer, Self::Parser, Self::IrGen, Self::Optimizer, Self::CodeGen, Self::Linker]
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Source => "Source (JavaScript)",
            Self::Lexer => "Lexer → Token Stream",
            Self::Parser => "Parser → AST",
            Self::IrGen => "IR Generation",
            Self::Optimizer => "Optimization Passes",
            Self::CodeGen => "Code Generation (C/ASM)",
            Self::Linker => "Linker → Binary",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Source => "你写的 JavaScript 代码，和浏览器里一样",
            Self::Lexer => "把代码拆成一个个'单词'(Token): 关键字、数字、运算符",
            Self::Parser => "把 Token 组装成语法树(AST)，表达代码的逻辑结构",
            Self::IrGen => "转成中间表示(IR)，和具体硬件无关的通用指令",
            Self::Optimizer => "删除无用代码、常量折叠、循环优化",
            Self::CodeGen => "翻译成目标 C 代码或汇编，针对具体芯片",
            Self::Linker => "把所有代码拼在一起，生成可烧录的 .bin/.hex",
        }
    }
}

/// 生成编译可视化教学文本
pub fn generate_compile_visualization(source: &str, stage: CompileStage) -> String {
    let mut out = String::new();
    out.push_str(&format!("=== 编译阶段: {} ===\n", stage.as_str()));
    out.push_str(&format!("说明: {}\n\n", stage.description()));

    match stage {
        CompileStage::Source => {
            out.push_str("```javascript\n");
            out.push_str(source);
            out.push_str("\n```\n");
        }
        CompileStage::Lexer => {
            out.push_str("Token 流:\n");
            // 简单 token 化展示
            for (i, word) in source.split_whitespace().enumerate().take(20) {
                let kind = classify_token(word);
                out.push_str(&format!("  [{:2}] {:<15} → {}\n", i, word, kind));
            }
        }
        CompileStage::Parser => {
            out.push_str("AST (简化):\n");
            out.push_str("  Program\n");
            for line in source.lines().take(10) {
                let trimmed = line.trim();
                if trimmed.is_empty() { continue; }
                if trimmed.starts_with("let ") || trimmed.starts_with("var ") {
                    out.push_str(&format!("    ├─ VarDecl: {}\n", trimmed));
                } else if trimmed.starts_with("function ") {
                    out.push_str(&format!("    ├─ FuncDecl: {}\n", trimmed));
                } else {
                    out.push_str(&format!("    ├─ Stmt: {}\n", trimmed));
                }
            }
        }
        CompileStage::IrGen => {
            out.push_str("IR (三地址码):\n");
            let mut reg = 0;
            for line in source.lines().take(10) {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with("//") { continue; }
                out.push_str(&format!("  t{} = translate({})\n", reg, trimmed));
                reg += 1;
            }
        }
        CompileStage::Optimizer => {
            out.push_str("优化 Pass:\n");
            out.push_str("  [1] Dead Code Elimination → 删除未使用变量\n");
            out.push_str("  [2] Constant Folding → 编译期计算常量表达式\n");
            out.push_str("  [3] Loop Unrolling → 展开小循环\n");
            out.push_str("  [4] Register Allocation → 分配物理寄存器\n");
            out.push_str("  [5] WCET Annotation → 标注最坏执行路径\n");
        }
        CompileStage::CodeGen => {
            out.push_str("生成的 C 代码 (片段):\n");
            out.push_str("```c\n");
            out.push_str("void app_main(void) {\n");
            out.push_str("    /* 由 DUANYAN 自动生成 */\n");
            out.push_str("    /* 所有变量: 栈上分配, 无 malloc */\n");
            out.push_str("    /* 所有循环: 有界, WCET 可计算 */\n");
            out.push_str("}\n");
            out.push_str("```\n");
        }
        CompileStage::Linker => {
            out.push_str("输出文件:\n");
            out.push_str("  app.bin  → 可烧录二进制 (裸机)\n");
            out.push_str("  app.map  → 内存映射 (Flash/RAM 分布)\n");
            out.push_str("  app.lst  → 反汇编清单 (调试用)\n");
            out.push_str("  app.wcet → WCET 分析报告\n");
        }
    }

    out
}

fn classify_token(word: &str) -> &'static str {
    let clean = word.trim_end_matches(|c| c == ';' || c == ',' || c == '(' || c == ')');
    match clean {
        "let" | "var" | "const" => "KEYWORD",
        "function" | "return" => "KEYWORD",
        "if" | "else" | "while" | "for" => "KEYWORD",
        "true" | "false" => "LITERAL_BOOL",
        _ if clean.parse::<f64>().is_ok() => "LITERAL_NUM",
        _ if clean.starts_with('"') => "LITERAL_STR",
        _ if clean == "+" || clean == "-" || clean == "*" || clean == "/" => "OPERATOR",
        _ if clean == "=" || clean == "==" || clean == "!=" => "OPERATOR",
        _ => "IDENTIFIER",
    }
}

// =============================================================================
// 课程生成
// =============================================================================

/// 生成标准课程大纲
pub fn generate_curriculum() -> Vec<LessonUnit> {
    vec![
        LessonUnit {
            title: "Hello LED: 第一个嵌入式程序".into(),
            level: CourseLevel::Beginner,
            objectives: vec![
                "理解嵌入式与 Web 开发的区别".into(),
                "掌握 GPIO 输出控制".into(),
                "理解'确定性'的含义".into(),
            ],
            example_code: r#"let led = 13;
pinMode(led, "output");
while (true) {
    digitalWrite(led, 1);
    delay(500);
    digitalWrite(led, 0);
    delay(500);
}"#.into(),
            exercises: vec![
                Exercise {
                    description: "修改闪烁频率为 2Hz (250ms 亮, 250ms 灭)".into(),
                    difficulty: 1,
                    expected_behavior: "LED 每秒闪烁 2 次".into(),
                    hints: vec!["delay 参数单位是毫秒".into()],
                },
                Exercise {
                    description: "用 for 循环让 LED 闪烁 10 次后停止".into(),
                    difficulty: 2,
                    expected_behavior: "LED 闪烁 10 次后常灭".into(),
                    hints: vec!["for (let i = 0; i < 10; i++)".into()],
                },
            ],
            peripherals: vec!["GPIO".into()],
        },
        LessonUnit {
            title: "传感器读取: ADC 与模拟世界".into(),
            level: CourseLevel::Elementary,
            objectives: vec![
                "理解 ADC 采样原理".into(),
                "掌握模拟量到数字量的转换".into(),
                "学会数据滤波 (移动平均)".into(),
            ],
            example_code: r#"let sensor = 0;
let sum = 0;
for (let i = 0; i < 16; i++) {
    sum = sum + analogRead(sensor);
}
let avg = sum / 16;
if (avg > 2048) {
    digitalWrite(13, 1);
}"#.into(),
            exercises: vec![
                Exercise {
                    description: "实现中值滤波: 读 5 次取中间值".into(),
                    difficulty: 3,
                    expected_behavior: "输出不受单次噪声干扰".into(),
                    hints: vec!["先排序，再取第 3 个".into(), "冒泡排序 5 个元素".into()],
                },
            ],
            peripherals: vec!["ADC".into(), "GPIO".into()],
        },
        LessonUnit {
            title: "实时调度: 让任务准时运行".into(),
            level: CourseLevel::Advanced,
            objectives: vec![
                "理解实时系统的'截止期'概念".into(),
                "掌握优先级调度原理".into(),
                "用 DUANYAN 生成 RTOS 任务".into(),
            ],
            example_code: r#"function sensorTask() {
    let val = analogRead(0);
    if (val > 3000) { alarm(); }
}
function commTask() {
    uartSend(readSensor());
}
// 调度: sensorTask 每 10ms, commTask 每 100ms"#.into(),
            exercises: vec![
                Exercise {
                    description: "计算两个任务的 CPU 利用率，判断是否可调度".into(),
                    difficulty: 4,
                    expected_behavior: "U = C1/T1 + C2/T2 < 0.693 (RMS)".into(),
                    hints: vec!["RMS 可调度充分条件: U ≤ n(2^(1/n)-1)".into()],
                },
            ],
            peripherals: vec!["Timer".into(), "UART".into(), "ADC".into()],
        },
    ]
}

/// 生成学习进度报告
pub fn generate_progress_report(student_name: &str, completed: &[CourseLevel]) -> String {
    let mut report = String::new();
    report.push_str(&format!("=== DUANYAN 学习进度: {} ===\n\n", student_name));

    let total_levels = 5;
    let done = completed.len();
    report.push_str(&format!("完成进度: {}/{} 级别 ({}%)\n\n", done, total_levels, done * 100 / total_levels));

    for level in CourseLevel::all_levels() {
        let status = if completed.contains(level) { "[✓]" } else { "[ ]" };
        report.push_str(&format!("  {} {} ({}h)\n", status, level.as_str(), level.target_hours()));
    }

    if done >= 3 {
        report.push_str("\n🎉 已具备嵌入式中级开发能力！\n");
    }
    if done >= 5 {
        report.push_str("🏆 恭喜！你已掌握确定性嵌入式开发全部技能！\n");
    }

    report
}

impl CourseLevel {
    pub fn all_levels() -> &'static [CourseLevel] {
        &[Self::Beginner, Self::Elementary, Self::Intermediate, Self::Advanced, Self::Expert]
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_course_level() {
        assert!(CourseLevel::Expert > CourseLevel::Beginner);
        assert_eq!(CourseLevel::Beginner.target_hours(), 8);
        assert_eq!(CourseLevel::Expert.target_hours(), 40);
        assert_eq!(CourseLevel::all_levels().len(), 5);
    }

    #[test]
    fn test_compile_stages() {
        assert_eq!(CompileStage::all().len(), 7);
        assert_eq!(CompileStage::Source.as_str(), "Source (JavaScript)");
        assert!(CompileStage::Parser.description().contains("AST"));
    }

    #[test]
    fn test_compile_visualization() {
        let source = "let x = 42;\nlet y = x + 1;";
        let vis = generate_compile_visualization(source, CompileStage::Lexer);
        assert!(vis.contains("Token"));
        assert!(vis.contains("KEYWORD")); // "let"
        assert!(vis.contains("LITERAL_NUM")); // "42"

        let vis2 = generate_compile_visualization(source, CompileStage::Parser);
        assert!(vis2.contains("AST"));
        assert!(vis2.contains("VarDecl"));
    }

    #[test]
    fn test_curriculum() {
        let curriculum = generate_curriculum();
        assert_eq!(curriculum.len(), 3);
        assert_eq!(curriculum[0].level, CourseLevel::Beginner);
        assert_eq!(curriculum[2].level, CourseLevel::Advanced);
        assert!(!curriculum[0].exercises.is_empty());
        assert!(curriculum[0].example_code.contains("digitalWrite"));
    }

    #[test]
    fn test_progress_report() {
        let report = generate_progress_report("Alice", &[CourseLevel::Beginner, CourseLevel::Elementary, CourseLevel::Intermediate]);
        assert!(report.contains("Alice"));
        assert!(report.contains("3/5"));
        assert!(report.contains("60%"));
        assert!(report.contains("中级开发能力"));
    }

    #[test]
    fn test_token_classification() {
        assert_eq!(classify_token("let"), "KEYWORD");
        assert_eq!(classify_token("42"), "LITERAL_NUM");
        assert_eq!(classify_token("myVar"), "IDENTIFIER");
        assert_eq!(classify_token("+"), "OPERATOR");
        assert_eq!(classify_token("true"), "LITERAL_BOOL");
    }
}
