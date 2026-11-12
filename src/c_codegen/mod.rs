//! C 代码生成器 (AST → C transpiler)
//!
//! Phase C: ESP32 后端 | Phase D: Arduino 后端
//! 将 DUANYAN AST 转译为 C 源码，供 ESP-IDF / Arduino Core / STM32 HAL 编译

use crate::parser::{
    Program, Statement, Expr, BinOp, MathOp,
    Function, ArrowBody, TemplateExprPart,
};
use crate::hardware_api::HardwareModule;
use crate::scheduler::Scheduler;
use crate::strip::{RegisterDef, F32_DECIMAL_SENTINEL};
use std::collections::{HashSet, HashMap};

/// C 编译目标平台
#[derive(Debug, Clone, PartialEq)]
pub enum CTarget {
    Esp32,
    Stm32,
    ArduinoAvr,
    ArduinoArm,
    RaspberryPi,
    /// ESP32-S3 裸机 (无 ESP-IDF): 寄存器级 HAL, 直出可烧录镜像
    Esp32S3Bare,
}

impl CTarget {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "esp32" | "esp" => Some(Self::Esp32),
            "esp32s3-bare" | "esp32-bare" | "esp32s3" | "bare" => Some(Self::Esp32S3Bare),
            // v1.6 P3: DUANYAN 脊髄端 (脑体分离: Pi=脑, S3=脊髄) — 复用裸机镜像路径 + Mode-B 应答骨架
            "duanyan-spine" | "spine" => Some(Self::Esp32S3Bare),
            "stm32" => Some(Self::Stm32),
            "arduino-avr" | "arduino" => Some(Self::ArduinoAvr),
            "arduino-arm" => Some(Self::ArduinoArm),
            "rpi" | "raspberry" => Some(Self::RaspberryPi),
            _ => None,
        }
    }
}

/// C 代码生成器
struct CCodeGenerator {
    target: CTarget,
    output: String,
    indent: usize,
    /// 已导入的硬件模块
    hw_imports: HashSet<String>,
    /// 是否需要 stdio.h
    needs_stdio: bool,
    /// 是否需要 string.h
    needs_string: bool,
    /// 是否需要 stdint.h
    needs_stdint: bool,
    /// 箭头函数提升为全局函数的计数器
    arrow_counter: usize,
    /// 提升的箭头函数定义
    hoisted_functions: Vec<String>,
    /// GPIO 引脚跟踪 (变量名 → pin号)
    gpio_pins: Vec<(String, String)>,
    /// 是否有 WiFi 使用
    uses_wifi: bool,
    /// 是否有 AI 使用 (Phase F)
    uses_ai: bool,
    /// Promise.all/race 计数器 (生成唯一变量名)
    promise_counter: usize,
    /// 提升的 Promise 辅助代码
    promise_helpers: Vec<String>,
    /// Phase 1.3: 多任务调度器
    scheduler: Scheduler,
    /// Phase 2.3: 对象 struct 定义
    struct_defs: Vec<String>,
    /// Phase 2.3: 对象变量跟踪 (变量名 → 字段列表)
    object_vars: Vec<(String, Vec<String>)>,
    /// Phase 2.4: 数组变量跟踪 (变量名 → 长度)
    array_vars: Vec<(String, usize)>,
    /// Phase 2.4: 是否需要数组边界检查 helper
    needs_arr_helper: bool,
    /// 语言完备性批 2: Math.sqrt/pow 需要整数 helper
    needs_math_helper: bool,
    /// Phase 3.3: 中断函数跟踪 (func_name, timer, period_us)
    interrupt_fns: Vec<(String, String, u32)>,
    /// Phase 4: 芯片信息 API (duanyan:esp32 Chip)
    needs_chip_info: bool,
    /// 扩展 1/3: 旁路宽度/定点类型表 (变量名 → u8/u16/u32/i8/i16/i32/q15/q16/f32)
    width_types: HashMap<String, String>,
    /// 扩展 2: 旁路寄存器位域表 (变量名 → RegisterDef)
    registers: HashMap<String, RegisterDef>,
    /// 扩展 2: 是否需要寄存器访问宏
    needs_register: bool,
    /// H1: 已声明的寄存器名（检测同名重复声明 → 与普通对象冲突）
    register_declared: HashSet<String>,
    /// H3: 运行期长度数组（filter/slice 结果），禁止再作高阶方法源（长度编译期不确定）
    dynamic_len_arrays: HashSet<String>,
}

mod hal;
use hal::ESP32S3_BARE_HAL;
mod ext;
mod hw_helpers;

/// 生成结果
pub struct CGenerateResult {
    pub source: String,
    pub target: CTarget,
    pub hw_imports: HashSet<String>,
    pub uses_wifi: bool,
    pub uses_ai: bool,
}

/// 将 AST 生成 C 源码
pub fn generate_c_source(program: &Program, target: &CTarget) -> Result<CGenerateResult, String> {
    generate_c_source_with_widths(program, target, &HashMap::new())
}

/// 将 AST 生成 C 源码（带旁路宽度/定点类型表，扩展 1/3）
pub fn generate_c_source_with_widths(
    program: &Program,
    target: &CTarget,
    width_types: &HashMap<String, String>,
) -> Result<CGenerateResult, String> {
    generate_c_source_with_ext(program, target, width_types, &HashMap::new())
}

/// 将 AST 生成 C 源码（带旁路宽度表 + 寄存器位域表，扩展 1/2/3）
pub fn generate_c_source_with_ext(
    program: &Program,
    target: &CTarget,
    width_types: &HashMap<String, String>,
    registers: &HashMap<String, RegisterDef>,
) -> Result<CGenerateResult, String> {
    let mut gen = CCodeGenerator::new(target.clone());
    gen.width_types = width_types.clone();
    gen.registers = registers.clone();
    gen.generate_program(program)?;
    let uses_wifi = gen.uses_wifi;
    let uses_ai = gen.uses_ai;
    let hw_imports = gen.hw_imports.clone();
    Ok(CGenerateResult {
        source: gen.finalize(),
        target: target.clone(),
        hw_imports,
        uses_wifi,
        uses_ai,
    })
}

impl CCodeGenerator {
    fn new(target: CTarget) -> Self {
        Self {
            target,
            output: String::new(),
            indent: 0,
            hw_imports: HashSet::new(),
            needs_stdio: false,
            needs_string: false,
            needs_stdint: true,
            arrow_counter: 0,
            hoisted_functions: Vec::new(),
            gpio_pins: Vec::new(),
            uses_wifi: false,
            uses_ai: false,
            promise_counter: 0,
            promise_helpers: Vec::new(),
            scheduler: Scheduler::new(),
            struct_defs: Vec::new(),
            object_vars: Vec::new(),
            array_vars: Vec::new(),
            needs_arr_helper: false,
            needs_math_helper: false,
            interrupt_fns: Vec::new(),
            needs_chip_info: false,
            width_types: HashMap::new(),
            registers: HashMap::new(),
            needs_register: false,
            register_declared: HashSet::new(),
            dynamic_len_arrays: HashSet::new(),
        }
    }

    fn generate_program(&mut self, program: &Program) -> Result<(), String> {
        // 第一遍: 扫描 import 语句，确定需要的头文件
        for stmt in &program.statements {
            if let Statement::Import { items, source } = &stmt.stmt {
                if let Some(hw_mod) = HardwareModule::from_import_source(source) {
                    match hw_mod {
                        HardwareModule::Core => {
                            for item in items {
                                self.hw_imports.insert(item.clone());
                            }
                        }
                        HardwareModule::Esp32 => {
                            for item in items {
                                if item == "WiFi" {
                                    self.uses_wifi = true;
                                }
                                if item == "Chip" {
                                    self.needs_chip_info = true;
                                }
                                self.hw_imports.insert(format!("esp32:{}", item));
                            }
                        }
                        HardwareModule::Ai => {
                            self.uses_ai = true;
                            for item in items {
                                self.hw_imports.insert(format!("ai:{}", item));
                            }
                        }
                        HardwareModule::Motor => {
                            // Phase 1.2: Servo/Motor 模块
                            for item in items {
                                self.hw_imports.insert(format!("motor:{}", item));
                            }
                        }
                        HardwareModule::Servo => {
                            // v1.6 P0: 关节/舵机模块 (Joint/Pca9685/BusServo)
                            for item in items {
                                self.hw_imports.insert(format!("servo:{}", item));
                            }
                        }
                        HardwareModule::Robot => {
                            // v1.6 P2: 机器人轨迹/运动学模块 (Traj/Joint)
                            for item in items {
                                self.hw_imports.insert(format!("robot:{}", item));
                            }
                        }
                        HardwareModule::ModeB => {
                            // v1.6 P3: Mode-B 线协议直发模块 (ModeB)
                            for item in items {
                                self.hw_imports.insert(format!("modeb:{}", item));
                            }
                        }
                        _ => {}
                    }
                }
            }
            // 检测 console.log 使用
            self.scan_for_console_log_stmt(&stmt.stmt);
        }
        for sf in &program.functions {
            for stmt in &sf.func.body {
                self.scan_for_console_log_stmt(&stmt.stmt);
            }
        }

        // 第二遍: 生成函数体和箭头函数提升
        let mut func_bodies = Vec::new();
        let mut top_level_stmts = Vec::new();

        // 处理顶层函数
        for sf in &program.functions {
            let code = self.generate_function(&sf.func)?;
            func_bodies.push(code);
        }

        // 处理顶层语句（非 import 的收集到 main 中）
        for stmt in &program.statements {
            match &stmt.stmt {
                Statement::Import { .. } | Statement::Export { .. } => {} // 跳过
                _ => top_level_stmts.push(stmt),
            }
        }

        // 先生成提升的箭头函数（需要预处理）
        // 我们先暂存 top_level_stmts 代码，处理箭头函数提升
        let mut main_body = String::new();
        let saved_output = std::mem::take(&mut self.output);

        self.indent = 1;
        for stmt in &top_level_stmts {
            let line = self.generate_statement(&stmt.stmt)?;
            main_body.push_str(&line);
        }
        self.indent = 0;

        // 组装最终输出
        self.output = saved_output;
        self.generate_header();
        self.generate_hw_helpers();

        // Phase 2.3: 输出对象 struct 定义
        if !self.struct_defs.is_empty() {
            self.output.push_str("/* DUANYAN object structs */\n");
            for def in &self.struct_defs.clone() {
                self.output.push_str(def);
            }
            self.output.push('\n');
        }

        // 扩展 2: 寄存器位域访问宏 (masked RMW, 源码零指针)
        if self.needs_register {
            self.output.push_str("/* DUANYAN register bit-field access (扩展 2) */\n");
            self.output.push_str("#ifndef DUANYAN_REG32\n");
            self.output.push_str("#define DUANYAN_REG32(addr) (*(volatile uint32_t *)(addr))\n");
            self.output.push_str("#endif\n\n");
        }

        // Phase 2.4: 输出数组边界检查 helper
        if self.needs_arr_helper {
            self.output.push_str("/* DUANYAN array bounds check */\n");
            self.output.push_str("static inline int32_t _duanyan_arr_get(int32_t* arr, int32_t len, int32_t idx) {\n");
            self.output.push_str("    if (idx < 0 || idx >= len) { while(1); /* ARRAY_OOB */ }\n");
            self.output.push_str("    return arr[idx];\n");
            self.output.push_str("}\n\n");
        }

        // 语言完备性批 2: Math.sqrt/pow 整数 helper（无 libm，裸机可用）
        if self.needs_math_helper {
            self.output.push_str("/* DUANYAN integer math helpers (no libm) */\n");
            self.output.push_str("static int32_t duanyan_isqrt(int32_t n) {\n");
            self.output.push_str("    if (n < 0) return 0;\n");
            self.output.push_str("    if (n == 0) return 0;\n");
            self.output.push_str("    int32_t x = n;\n");
            self.output.push_str("    int32_t y = (x + 1) / 2;\n");
            self.output.push_str("    while (y < x) { x = y; y = (x + n / x) / 2; }\n");
            self.output.push_str("    return x;\n");
            self.output.push_str("}\n");
            self.output.push_str("static int32_t duanyan_ipow(int32_t b, int32_t e) {\n");
            self.output.push_str("    int32_t r = 1;\n");
            self.output.push_str("    while (e > 0) { if (e & 1) r *= b; b *= b; e >>= 1; }\n");
            self.output.push_str("    return r;\n");
            self.output.push_str("}\n\n");
        }

        // 输出提升的箭头函数
        for hoisted in &self.hoisted_functions.clone() {
            self.output.push_str(hoisted);
            self.output.push('\n');
        }

        // 输出 Promise.all/race 辅助代码
        for helper in &self.promise_helpers.clone() {
            self.output.push_str(helper);
            self.output.push('\n');
        }

        // 输出用户定义的函数
        for func_code in &func_bodies {
            self.output.push_str(func_code);
            self.output.push('\n');
        }

        // Phase 1.3: 输出调度器任务包装函数
        if self.scheduler.has_tasks() {
            let wrappers = self.scheduler.generate_task_wrappers(&self.target);
            self.output.push_str(&wrappers);
        }

        if matches!(self.target, CTarget::ArduinoAvr | CTarget::ArduinoArm) {
            // Arduino: setup() + loop() 结构
            let (setup_body, loop_body) = self.split_setup_loop(&main_body);
            self.output.push_str("void setup() {\n");
            // setup 中初始化 Serial
            if self.needs_stdio {
                self.output.push_str("    Serial.begin(115200);\n");
            }
            self.output.push_str(&setup_body);
            // Phase 1.3: 调度器初始化
            if self.scheduler.has_tasks() {
                let init = self.scheduler.generate_task_init(&self.target);
                self.output.push_str(&init);
            }
            // Phase 3.3: 定时器中断初始化
            if !self.interrupt_fns.is_empty() {
                let isr_init = self.generate_timer_isr_init();
                self.output.push_str(&isr_init);
            }
            self.output.push_str("}\n\n");
            self.output.push_str("void loop() {\n");
            self.output.push_str(&loop_body);
            // Phase 1.3: 调度器 poll (Arduino 协作式)
            if self.scheduler.has_tasks() {
                let poll = self.scheduler.generate_arduino_loop_call();
                self.output.push_str(&poll);
            }
            self.output.push_str("}\n");
        } else if self.target == CTarget::Esp32S3Bare {
            // 裸机: _start 入口 (section .entry, 与 native_flasher 链接脚本对齐)
            self.output.push_str("void _start(void) __attribute__((section(\".entry\"), used));\n");
            self.output.push_str("void _start(void)\n{\n");
            self.output.push_str("    duanyan_boot_init();\n");
            self.output.push_str(&main_body);
            self.output.push_str("    for (;;) { }\n");
            self.output.push_str("}\n");
        } else if self.target == CTarget::Esp32 {
            self.output.push_str("void app_main(void)\n{\n");
            self.output.push_str(&main_body);
            // Phase 1.3: FreeRTOS 任务创建
            if self.scheduler.has_tasks() {
                let init = self.scheduler.generate_task_init(&self.target);
                self.output.push_str(&init);
            }
            // Phase 3.3: 定时器中断初始化
            if !self.interrupt_fns.is_empty() {
                let isr_init = self.generate_timer_isr_init();
                self.output.push_str(&isr_init);
            }
            self.output.push_str("}\n");
        } else {
            self.output.push_str("int main(void)\n{\n");
            self.output.push_str(&main_body);
            // Phase 1.3: 调度器初始化 (STM32/RPi)
            if self.scheduler.has_tasks() {
                let init = self.scheduler.generate_task_init(&self.target);
                self.output.push_str(&init);
                // STM32: 主循环中调用 poll
                if self.target == CTarget::Stm32 {
                    self.output.push_str("    for (;;) { _duanyan_scheduler_poll(); }\n");
                }
            }
            // Phase 3.3: 定时器中断初始化
            if !self.interrupt_fns.is_empty() {
                let isr_init = self.generate_timer_isr_init();
                self.output.push_str(&isr_init);
            }
            self.output.push_str("    return 0;\n");
            self.output.push_str("}\n");
        }

        Ok(())
    }

    /// Arduino: 将 main_body 拆分为 setup 代码 和 loop 代码
    /// 规则: while(true){...} 的循环体 → loop()，其前的语句 → setup()
    fn split_setup_loop(&self, main_body: &str) -> (String, String) {
        // 查找顶层 while(true) / while(1) 的位置
        // 简单策略：按行查找 "    while (1) {" 或 "    while (true) {"
        let lines: Vec<&str> = main_body.lines().collect();
        let mut split_idx = lines.len(); // 默认: 没有 while(true)，全部在 setup

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("while (1) {") || trimmed.starts_with("while (true) {") {
                split_idx = i;
                break;
            }
        }

        let setup_lines = &lines[..split_idx];
        let setup_body = setup_lines.iter().map(|l| format!("{}\n", l)).collect::<String>();

        if split_idx < lines.len() {
            // 找到 while(true) { ... }，提取循环体
            let mut brace_depth = 0;
            let mut loop_lines = Vec::new();
            let mut in_body = false;
            for line in &lines[split_idx..] {
                let trimmed = line.trim();
                for ch in trimmed.chars() {
                    if ch == '{' { brace_depth += 1; in_body = true; }
                    if ch == '}' { brace_depth -= 1; }
                }
                // 跳过 "while (1) {" 这一行和最后的 "}"
                if in_body && brace_depth > 0 && !trimmed.starts_with("while (") {
                    loop_lines.push(*line);
                }
            }
            let loop_body = loop_lines.iter().map(|l| format!("{}\n", l)).collect::<String>();
            (setup_body, loop_body)
        } else {
            // 没有 while(true)，全部在 setup，loop 为空
            (setup_body, String::new())
        }
    }

    /// 扫描语句中是否使用了 console.log
    fn scan_for_console_log_stmt(&mut self, stmt: &Statement) {
        match stmt {
            Statement::ConsoleLog(_) => { self.needs_stdio = true; }
            Statement::If { then_body, else_body, .. } => {
                for s in then_body { self.scan_for_console_log_stmt(&s.stmt); }
                if let Some(eb) = else_body {
                    for s in eb { self.scan_for_console_log_stmt(&s.stmt); }
                }
            }
            Statement::While { body, .. } | Statement::For { body, .. } => {
                for s in body { self.scan_for_console_log_stmt(&s.stmt); }
            }
            Statement::Switch { cases, .. } => {
                for c in cases {
                    for s in &c.body { self.scan_for_console_log_stmt(&s.stmt); }
                }
            }
            _ => {}
        }
    }

    /// 生成文件头 (#include)
    fn generate_header(&mut self) {
        self.output.push_str("/* Auto-generated by DUANYAN v");
        self.output.push_str(env!("CARGO_PKG_VERSION"));
        self.output.push_str(" — JS → C transpiler */\n");

        // Arduino 不需要额外 include（Arduino.h 已包含 stdint/stdio/string）
        if !matches!(self.target, CTarget::ArduinoAvr | CTarget::ArduinoArm) {
            if self.needs_stdint {
                self.output.push_str("#include <stdint.h>\n");
            }
            if self.needs_stdio {
                self.output.push_str("#include <stdio.h>\n");
            }
            if self.needs_string {
                self.output.push_str("#include <string.h>\n");
            }
        }

        match self.target {
            CTarget::Esp32S3Bare => {
                self.output.push_str("#include <stdint.h>\n");
                self.output.push_str(ESP32S3_BARE_HAL);
            }
            CTarget::Esp32 => {
                self.output.push_str("#include \"freertos/FreeRTOS.h\"\n");
                self.output.push_str("#include \"freertos/task.h\"\n");
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("#include \"driver/gpio.h\"\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("#include \"driver/uart.h\"\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("#include \"driver/ledc.h\"\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("#include \"driver/adc.h\"\n");
                }
                if self.hw_imports.contains("motor:Servo") || self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("#include \"driver/ledc.h\"\n");
                    self.output.push_str("#include \"driver/gpio.h\"\n");
                }
                // v1.6 P0: duanyan:servo 关节层 — PCA9685 走 I²C, ZX361 走 UART
                // v1.6 P2: duanyan:robot 轨迹/glide 复用关节 helper, 故一并拉入
                if self.hw_imports.iter().any(|s| s.starts_with("servo:") || s.starts_with("robot:") || s.starts_with("modeb:")) {
                    self.output.push_str("#include \"driver/i2c.h\"\n");
                    self.output.push_str("#include \"driver/uart.h\"\n");
                    self.output.push_str("#include <string.h>\n");
                    self.output.push_str("#include <stdio.h>\n");
                    self.output.push_str("#include <stdlib.h>\n");
                }
                if self.hw_imports.iter().any(|s| s.starts_with("robot:")) {
                    self.output.push_str("#include <math.h>\n");
                }
                if self.hw_imports.contains("Timer") {
                    // FreeRTOS task delay already included
                }
                if self.uses_wifi {
                    self.output.push_str("#include \"esp_wifi.h\"\n");
                    self.output.push_str("#include \"esp_event.h\"\n");
                    self.output.push_str("#include \"nvs_flash.h\"\n");
                    self.output.push_str("#include \"esp_log.h\"\n");
                }
                if self.needs_chip_info {
                    self.output.push_str("#include \"esp_chip_info.h\"\n");
                    self.output.push_str("#include \"esp_flash.h\"\n");
                    self.output.push_str("#include \"esp_system.h\"\n");
                    self.output.push_str("#include \"esp_mac.h\"\n");
                    self.output.push_str("#include \"mbedtls/sha256.h\"\n");
                }
                if self.uses_ai {
                    self.output.push_str("#include \"duanyan_ai.h\"\n");
                }
            }
            CTarget::Stm32 => {
                self.output.push_str("#include \"stm32f1xx_hal.h\"\n");
                if self.hw_imports.contains("UART") {
                    self.output.push_str("#include <string.h>\n");
                }
            }
            CTarget::ArduinoAvr | CTarget::ArduinoArm => {
                self.output.push_str("#include <Arduino.h>\n");
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("#include <Servo.h>\n");
                }
            }
            CTarget::RaspberryPi => {
                self.output.push_str("#include <stdio.h>\n");
                self.output.push_str("#include <stdlib.h>\n");
                self.output.push_str("#include <fcntl.h>\n");
                self.output.push_str("#include <unistd.h>\n");
                self.output.push_str("#include <string.h>\n");
            }
        }
        self.output.push('\n');
    }

    fn generate_function(&mut self, func: &Function) -> Result<String, String> {
        // Phase v1.5: async 函数生成状态机
        if func.is_async {
            return self.generate_async_function(func);
        }

        // Phase 3.3: @interrupt 函数跟踪
        if let (Some(timer), Some(period)) = (&func.interrupt_timer, func.interrupt_period_us) {
            self.interrupt_fns.push((func.name.clone(), timer.clone(), period));
        }

        let mut code = String::new();

        // Phase 3.3: @realtime 标记注释
        if func.is_realtime {
            code.push_str(&format!("/* @realtime */\n"));
        }

        code.push_str(&format!("int32_t {}(", func.name));
        if func.params.is_empty() {
            code.push_str("void");
        } else {
            let params: Vec<String> = func.params.iter()
                .map(|p| format!("{} {}", self.c_width_type(p), p))
                .collect();
            code.push_str(&params.join(", "));
        }
        code.push_str(")\n{\n");

        self.indent = 1;
        for stmt in &func.body {
            let line = self.generate_statement(&stmt.stmt)?;
            code.push_str(&line);
        }
        self.indent = 0;

        code.push_str("}\n");
        Ok(code)
    }

    /// Phase v1.5: 生成 async 函数的状态机 C 代码
    fn generate_async_function(&mut self, func: &Function) -> Result<String, String> {
        use crate::async_transform;

        let sm = async_transform::transform_to_state_machine(func)
            .map_err(|e| format!("async 变换失败: {}", e))?;

        let mut code = String::new();

        // 1. 上下文结构体
        code.push_str(&format!("/* async function {}() — DUANYAN v1.5 状态机 */\n", func.name));
        code.push_str(&format!("typedef struct {{\n"));
        code.push_str("    uint8_t state;\n");
        // 参数也保存在 ctx 中
        for p in &func.params {
            code.push_str(&format!("    int32_t {};\n", p));
        }
        for var in &sm.ctx_vars {
            code.push_str(&format!("    int32_t {};\n", var.name));
        }
        code.push_str("    int32_t result;\n");
        code.push_str(&format!("}} {}_ctx;\n\n", func.name));

        // 2. poll 函数
        code.push_str(&format!("duanyan_status_t {}_poll({}_ctx* ctx) {{\n", func.name, func.name));
        code.push_str("    switch (ctx->state) {\n");

        for state in &sm.states {
            code.push_str(&format!("        case {}:\n", state.state_id));

            // 生成语句代码
            self.indent = 3;
            for ss in &state.statements {
                // 跳过包含 await 的语句（由 await_expr 处理）
                if let Some(ref _await_expr) = state.await_expr {
                    // 只生成 await 之前的部分
                    let stmt_code = self.generate_statement(&ss.stmt)?;
                    code.push_str(&stmt_code);
                } else {
                    let stmt_code = self.generate_statement(&ss.stmt)?;
                    code.push_str(&stmt_code);
                }
            }
            self.indent = 0;

            if let Some(ref await_expr) = state.await_expr {
                code.push_str(&format!("            /* await: {} */\n", format_expr_short(await_expr)));
                code.push_str(&format!("            ctx->state = {};\n", state.next_state));
                code.push_str("            return DUANYAN_PENDING;\n");
            } else if state.is_final {
                code.push_str("            return DUANYAN_READY;\n");
            }
            code.push_str("\n");
        }

        code.push_str("    }\n");
        code.push_str("    return DUANYAN_READY;\n");
        code.push_str("}\n");

        // 3. 启动函数 (spawn wrapper)
        code.push_str(&format!("\nvoid {}_spawn(", func.name));
        if func.params.is_empty() {
            code.push_str("void");
        } else {
            let params: Vec<String> = func.params.iter()
                .map(|p| format!("int32_t {}", p))
                .collect();
            code.push_str(&params.join(", "));
        }
        code.push_str(") {\n");
        code.push_str(&format!("    static {}_ctx ctx;\n", func.name));
        code.push_str("    ctx.state = 0;\n");
        for p in &func.params {
            code.push_str(&format!("    ctx.{} = {};\n", p, p));
        }
        code.push_str(&format!("    duanyan_spawn((duanyan_status_t (*)(void*)){}_poll, &ctx);\n", func.name));
        code.push_str("}\n");

        Ok(code)
    }

    fn generate_statement(&mut self, stmt: &Statement) -> Result<String, String> {
        let pad = self.indent_str();
        match stmt {
            Statement::Let { name, value } => {
                // 扩展 2: 寄存器声明（let 形式）— 无存储，发射注释占位
                if let Some(def) = self.registers.get(name).cloned() {
                    // H1: 同名重复声明（@register 名又被普通 let/const 复用）→ 冲突，防误展开为裸寄存器访问
                    if !self.register_declared.insert(name.clone()) {
                        return Err(format!("扩展 2: 名字 '{}' 重复声明——@register 寄存器名不可与普通变量/对象同名（旁路表按名全局生效，会误展开为裸寄存器访问）", name));
                    }
                    self.needs_register = true;
                    let fields: Vec<String> = def.fields.iter()
                        .map(|(n, hi, lo)| format!("{}[{}:{}]", n, hi, lo)).collect();
                    return Ok(format!("{}/* @register {} @ 0x{:08X}: {} */\n",
                        pad, name, def.base, fields.join(" ")));
                }
                // 扩展 4: 数组高阶方法 let 绑定 → 确定性 for 循环
                if let Some((arr, method, args)) = Self::match_array_hof(value) {
                    if let Some(len) = self.array_len(&arr) {
                        return self.emit_array_hof(Some(name.clone()), &arr, len, &method, args);
                    }
                }
                // 检查是否是箭头函数赋值
                if let Expr::ArrowFunc { params, body, .. } = value {
                    return self.generate_arrow_assignment(name, params, body);
                }
                // Phase 2.3: 对象字面量 → C struct
                if let Expr::ObjectLit { keys, values } = value {
                    return self.generate_object_let(name, keys, values);
                }
                // Phase 2.4: 数组字面量 → 定长数组 + 跟踪长度
                if let Expr::Array(elems) = value {
                    let len = elems.len();
                    self.array_vars.push((name.clone(), len));
                    let items: Vec<String> = elems.iter()
                        .map(|e| self.generate_expr(e))
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(format!("{}int32_t {}[{}] = {{ {} }};\n", pad, name, len, items.join(", ")));
                }
                let c_expr = self.generate_expr(value)?;
                // 检查是否是 GPIO.output / GPIO.input 调用
                if let Expr::Call { name: call_name, args } = value {
                    if call_name == "GPIO.output" {
                        let pin_expr = self.generate_expr(&args[0])?;
                        self.gpio_pins.push((name.clone(), pin_expr.clone()));
                        return Ok(format!("{}int32_t {} = {}; duanyan_gpio_output({});\n", pad, name, pin_expr, pin_expr));
                    }
                    if call_name == "GPIO.input" {
                        let pull = if args.len() > 1 { "pullup" } else { "none" };
                        let pin_expr = self.generate_expr(&args[0])?;
                        self.gpio_pins.push((name.clone(), pin_expr.clone()));
                        return Ok(format!("{}int32_t {} = {}; duanyan_gpio_input_{}({});\n", pad, name, pin_expr, pull, pin_expr));
                    }
                    if call_name == "PWM.output" {
                        let pin = self.generate_expr(&args[0])?;
                        let freq = if args.len() > 1 { self.generate_expr(&args[1])? } else { "1000".to_string() };
                        return Ok(format!("{}int32_t {} = duanyan_pwm_output({}, {});\n", pad, name, pin, freq));
                    }
                    if call_name == "ADC.channel" {
                        let ch = self.generate_expr(&args[0])?;
                        return Ok(format!("{}int32_t {} = {}; duanyan_adc_init({});\n", pad, name, ch, ch));
                    }
                }
                // 扩展 1: 宽度安全整数 — 按旁路表发射 uintN_t/intN_t + 截断
                if let Expr::Number(n) = value {
                    self.lint_width_literal(name, *n)?;
                }
                self.check_f32_decimal(name, value)?;
                let cty = self.c_width_type(name);
                let expr_out = match self.width_cast(name) {
                    Some(cast) => format!("{}({})", cast, c_expr),
                    None => c_expr.clone(),
                };
                Ok(format!("{}{} {} = {};\n", pad, cty, name, expr_out))
            }
            Statement::Const { name, value } => {
                // 扩展 2: 寄存器声明 — 无存储，发射注释占位（位域经旁路表在访问点展开）
                if let Some(def) = self.registers.get(name).cloned() {
                    // H1: 同名重复声明（@register 名又被普通 let/const 复用）→ 冲突，防误展开为裸寄存器访问
                    if !self.register_declared.insert(name.clone()) {
                        return Err(format!("扩展 2: 名字 '{}' 重复声明——@register 寄存器名不可与普通变量/对象同名（旁路表按名全局生效，会误展开为裸寄存器访问）", name));
                    }
                    self.needs_register = true;
                    let fields: Vec<String> = def.fields.iter()
                        .map(|(n, hi, lo)| format!("{}[{}:{}]", n, hi, lo)).collect();
                    return Ok(format!("{}/* @register {} @ 0x{:08X}: {} */\n",
                        pad, name, def.base, fields.join(" ")));
                }
                // 扩展 4: 数组高阶方法 const 绑定 → 确定性 for 循环
                if let Some((arr, method, args)) = Self::match_array_hof(value) {
                    if let Some(len) = self.array_len(&arr) {
                        return self.emit_array_hof(Some(name.clone()), &arr, len, &method, args);
                    }
                }
                let c_expr = self.generate_expr(value)?;
                if let Expr::Number(n) = value {
                    self.lint_width_literal(name, *n)?;
                }
                self.check_f32_decimal(name, value)?;
                let cty = self.c_width_type(name);
                let expr_out = match self.width_cast(name) {
                    Some(cast) => format!("{}({})", cast, c_expr),
                    None => c_expr.clone(),
                };
                Ok(format!("{}const {} {} = {};\n", pad, cty, name, expr_out))
            }
            Statement::Assign { name, value } => {
                let c_expr = self.generate_expr(value)?;
                if let Expr::Number(n) = value {
                    self.lint_width_literal(name, *n)?;
                }
                self.check_f32_decimal(name, value)?;
                let expr_out = match self.width_cast(name) {
                    Some(cast) => format!("{}({})", cast, c_expr),
                    None => c_expr.clone(),
                };
                Ok(format!("{}{} = {};\n", pad, name, expr_out))
            }
            Statement::IndexAssign { name, index, value } => {
                let idx = self.generate_expr(index)?;
                let val = self.generate_expr(value)?;
                Ok(format!("{}{}[{}] = {};\n", pad, name, idx, val))
            }
            Statement::Return(expr) => {
                let c_expr = self.generate_expr(expr)?;
                Ok(format!("{}return {};\n", pad, c_expr))
            }
            Statement::If { condition, then_body, else_body } => {
                let cond = self.generate_expr(condition)?;
                let mut code = format!("{}if ({}) {{\n", pad, cond);
                self.indent += 1;
                for s in then_body {
                    code.push_str(&self.generate_statement(&s.stmt)?);
                }
                self.indent -= 1;
                if let Some(eb) = else_body {
                    code.push_str(&format!("{}}} else {{\n", pad));
                    self.indent += 1;
                    for s in eb {
                        code.push_str(&self.generate_statement(&s.stmt)?);
                    }
                    self.indent -= 1;
                }
                code.push_str(&format!("{}}}\n", pad));
                Ok(code)
            }
            Statement::While { condition, body } => {
                let cond = self.generate_expr(condition)?;
                let mut code = format!("{}while ({}) {{\n", pad, cond);
                self.indent += 1;
                for s in body {
                    code.push_str(&self.generate_statement(&s.stmt)?);
                }
                self.indent -= 1;
                code.push_str(&format!("{}}}\n", pad));
                Ok(code)
            }
            Statement::For { init, condition, update, body } => {
                let init_code = self.generate_for_init(&**init)?;
                let cond = self.generate_expr(condition)?;
                let update_code = self.generate_for_update(&**update)?;
                let mut code = format!("{}for ({}; {}; {}) {{\n", pad, init_code, cond, update_code);
                self.indent += 1;
                for s in body {
                    code.push_str(&self.generate_statement(&s.stmt)?);
                }
                self.indent -= 1;
                code.push_str(&format!("{}}}\n", pad));
                Ok(code)
            }
            Statement::Switch { discriminant, cases } => {
                let disc = self.generate_expr(discriminant)?;
                let mut code = format!("{}switch ({}) {{\n", pad, disc);
                for case in cases {
                    if let Some(test) = &case.test {
                        let val = self.generate_expr(test)?;
                        code.push_str(&format!("{}case {}:\n", pad, val));
                    } else {
                        code.push_str(&format!("{}default:\n", pad));
                    }
                    self.indent += 1;
                    for s in &case.body {
                        code.push_str(&self.generate_statement(&s.stmt)?);
                    }
                    self.indent -= 1;
                }
                code.push_str(&format!("{}}}\n", pad));
                Ok(code)
            }
            Statement::Break => Ok(format!("{}break;\n", pad)),
            Statement::Continue => Ok(format!("{}continue;\n", pad)),
            Statement::PostInc(name) => Ok(format!("{}{}++;\n", pad, name)),
            Statement::PostDec(name) => Ok(format!("{}{}--;\n", pad, name)),
            Statement::PropAssign { object, property, value } => {
                let val = self.generate_expr(value)?;
                // 扩展 2: 寄存器位域写 → masked RMW
                if let Some(stmt) = self.register_write_stmt(&pad, object, property, &val) {
                    return Ok(stmt);
                }
                // 检查是否是 PWM duty 调用: motor.duty(val) 已被解析为 PropAssign
                Ok(format!("{}{}.{} = {};\n", pad, object, property, val))
            }
            Statement::ConsoleLog(args) => {
                self.needs_stdio = true;
                self.generate_console_log(args, &pad)
            }
            Statement::Import { .. } => Ok(String::new()), // 已在头部处理
            Statement::Export { item } => self.generate_statement(&**item),
            Statement::DestructLet { names, source } => {
                let src = self.generate_expr(source)?;
                let mut code = String::new();
                for name in names {
                    code.push_str(&format!("{}int32_t {} = {}.{};\n", pad, name, src, name));
                }
                Ok(code)
            }
            Statement::Expr(expr) => {
                // 处理硬件 API 方法调用
                if let Some(hw_line) = self.try_generate_hw_method(expr)? {
                    return Ok(format!("{}{}\n", pad, hw_line));
                }
                // 扩展 4: 数组高阶方法作为表达式语句（forEach 等）
                if let Some((arr, method, args)) = Self::match_array_hof(expr) {
                    if let Some(len) = self.array_len(&arr) {
                        let nm = if method == "forEach" { None } else { Some(format!("_he{}", self.arrow_counter)) };
                        return self.emit_array_hof(nm, &arr, len, &method, args);
                    }
                }
                let c_expr = self.generate_expr(expr)?;
                Ok(format!("{}{};\n", pad, c_expr))
            }
        }
    }

    /// 生成 console.log
    fn generate_console_log(&mut self, args: &[Expr], pad: &str) -> Result<String, String> {
        // 裸机目标: 用 UART0 输出 (无 stdio)
        if self.target == CTarget::Esp32S3Bare {
            let mut code = String::new();
            for arg in args {
                match arg {
                    Expr::StringLit(s) => {
                        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                        code.push_str(&format!("{}duanyan_uart_puts(\"{}\\r\\n\");\n", pad, escaped));
                    }
                    Expr::Number(n) => {
                        code.push_str(&format!("{}duanyan_uart_num({});\n", pad, n));
                        code.push_str(&format!("{}duanyan_uart_puts(\"\\r\\n\");\n", pad));
                    }
                    _ => {
                        let c_expr = self.generate_expr(arg)?;
                        code.push_str(&format!("{}duanyan_uart_num((u32)({}));\n", pad, c_expr));
                        code.push_str(&format!("{}duanyan_uart_puts(\"\\r\\n\");\n", pad));
                    }
                }
            }
            return Ok(code);
        }
        // Arduino 使用 Serial.println 而非 printf
        let is_arduino = matches!(self.target, CTarget::ArduinoAvr | CTarget::ArduinoArm);

        if is_arduino {
            self.needs_stdio = true; // 标记需要 Serial 初始化
            return self.generate_arduino_console_log(args, pad);
        }

        if args.is_empty() {
            return Ok(format!("{}printf(\"\\n\");\n", pad));
        }

        let mut code = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                code.push_str(&format!("{}printf(\" \");\n", pad));
            }
            match arg {
                Expr::StringLit(s) => {
                    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                    code.push_str(&format!("{}printf(\"{}\\n\");\n", pad, escaped));
                }
                Expr::Number(n) => {
                    code.push_str(&format!("{}printf(\"%d\\n\", {});\n", pad, n));
                }
                Expr::Bool(b) => {
                    code.push_str(&format!("{}printf(\"%s\\n\", {} ? \"true\" : \"false\");\n", pad, if *b { "1" } else { "0" }));
                }
                _ => {
                    let c_expr = self.generate_expr(arg)?;
                    // Phase 4: 检测字符串返回的表达式 (Chip.modelName / Chip.sdkVersion)
                    if self.is_string_expr(arg) {
                        code.push_str(&format!("{}printf(\"%s\\n\", {});\n", pad, c_expr));
                    } else {
                        code.push_str(&format!("{}printf(\"%d\\n\", (int32_t)({}));\n", pad, c_expr));
                    }
                }
            }
        }
        Ok(code)
    }

    /// 生成 Arduino Serial.println 版本的 console.log
    fn generate_arduino_console_log(&mut self, args: &[Expr], pad: &str) -> Result<String, String> {
        if args.is_empty() {
            return Ok(format!("{}Serial.println();\n", pad));
        }

        let mut code = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                code.push_str(&format!("{}Serial.print(\" \");\n", pad));
            }
            match arg {
                Expr::StringLit(s) => {
                    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                    code.push_str(&format!("{}Serial.println(\"{}\");\n", pad, escaped));
                }
                Expr::Number(n) => {
                    code.push_str(&format!("{}Serial.println({});\n", pad, n));
                }
                Expr::Bool(b) => {
                    let s = if *b { "true" } else { "false" };
                    code.push_str(&format!("{}Serial.println(\"{}\");\n", pad, s));
                }
                _ => {
                    let c_expr = self.generate_expr(arg)?;
                    code.push_str(&format!("{}Serial.println({});\n", pad, c_expr));
                }
            }
        }
        Ok(code)
    }

    /// 尝试将表达式识别为硬件 API 方法调用
    fn try_generate_hw_method(&mut self, expr: &Expr) -> Result<Option<String>, String> {
        match expr {
            // led.high() → duanyan_gpio_high(pin)
            Expr::Call { name, args } => {
                match name.as_str() {
                    // 对象方法形式: led.high() 被解析为 Call { name: "led.high" }
                    n if n.ends_with(".high") => {
                        let var = n.trim_end_matches(".high");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("duanyan_gpio_high({});", pin)));
                        }
                    }
                    n if n.ends_with(".low") => {
                        let var = n.trim_end_matches(".low");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("duanyan_gpio_low({});", pin)));
                        }
                    }
                    n if n.ends_with(".toggle") => {
                        let var = n.trim_end_matches(".toggle");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            // toggle: read current state and invert
                            return Ok(Some(format!("duanyan_gpio_high({}); /* TODO: toggle */", pin)));
                        }
                    }
                    n if n.ends_with(".read") && args.is_empty() => {
                        let var = n.trim_end_matches(".read");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("duanyan_gpio_read({})", pin)));
                        }
                    }
                    // Phase 4: Chip Info API (duanyan:esp32) — 无参调用
                    "Chip.modelName" | "Chip.model" => {
                        return Ok(Some("duanyan_chip_model_name()".to_string()));
                    }
                    "Chip.cores" => {
                        return Ok(Some("duanyan_chip_cores()".to_string()));
                    }
                    "Chip.revision" => {
                        return Ok(Some("duanyan_chip_revision()".to_string()));
                    }
                    "Chip.flashSize" => {
                        return Ok(Some("duanyan_chip_flash_size()".to_string()));
                    }
                    "Chip.freeHeap" => {
                        return Ok(Some("duanyan_chip_free_heap()".to_string()));
                    }
                    "Chip.cpuFreq" => {
                        return Ok(Some("duanyan_chip_cpu_freq()".to_string()));
                    }
                    "Chip.id" => {
                        return Ok(Some("duanyan_chip_id()".to_string()));
                    }
                    "Chip.sdkVersion" => {
                        return Ok(Some("duanyan_chip_sdk_version()".to_string()));
                    }
                    "Chip.deviceId" => {
                        return Ok(Some("duanyan_chip_device_id()".to_string()));
                    }
                    "Chip.mac" => {
                        return Ok(Some("duanyan_chip_mac()".to_string()));
                    }
                    // v1.6 P0: Joint.home() 无参 → 全关节归中
                    "Joint.home" => {
                        return Ok(Some("duanyan_joint_home();".to_string()));
                    }
                    // v1.6 P3: ModeB.armHome() 无参 → 0x22 四关节归中 (黄金帧)
                    "ModeB.armHome" => {
                        return Ok(Some("_duanyan_modeb_arm_home();".to_string()));
                    }
                    _ => {}
                }
                // 有参数的调用
                if args.len() >= 1 {
                    let first_arg = self.generate_expr(&args[0])?;
                    match name.as_str() {
                        n if n.ends_with(".duty") && args.len() >= 2 => {
                            let _var = n.trim_end_matches(".duty");
                            let duty = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("duanyan_pwm_duty({}, {});", first_arg, duty)));
                        }
                        n if n.ends_with(".read") && args.is_empty() => {
                            // already handled above
                        }
                        // RGB 灯 (ESP32-S3 板载 WS2812)
                        "ADC.read" => {
                            return Ok(Some(format!("duanyan_adc_read({})", first_arg)));
                        }
                        "RGB.setColor" if args.len() >= 3 => {
                            let g = self.generate_expr(&args[1])?;
                            let b = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("duanyan_rgb_set({}, {}, {});", first_arg, g, b)));
                        }
                        "RGB.setHue" => {
                            return Ok(Some(format!("duanyan_rgb_hue({});", first_arg)));
                        }
                        // UART / Timer 直接调用
                        "UART.init" => {
                            return Ok(Some(format!("duanyan_uart_init({});", first_arg)));
                        }
                        "UART.write" => {
                            self.needs_string = true;
                            let s = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("duanyan_uart_write({});", s)));
                        }
                        "Timer.delay" => {
                            match self.target {
                                CTarget::Esp32S3Bare => {
                                    return Ok(Some(format!("duanyan_delay_ms({});", first_arg)));
                                }
                                CTarget::Esp32 => {
                                    return Ok(Some(format!("vTaskDelay(pdMS_TO_TICKS({}));", first_arg)));
                                }
                                CTarget::ArduinoAvr | CTarget::ArduinoArm => {
                                    return Ok(Some(format!("delay({});", first_arg)));
                                }
                                CTarget::Stm32 => {
                                    return Ok(Some(format!("HAL_Delay({});", first_arg)));
                                }
                                CTarget::RaspberryPi => {
                                    return Ok(Some(format!("usleep({} * 1000);", first_arg)));
                                }
                                _ => {
                                    return Ok(Some(format!("/* Timer.delay({}) — not yet mapped */", first_arg)));
                                }
                            }
                        }
                        // WiFi
                        "WiFi.connect" if args.len() >= 2 => {
                            let ssid = self.generate_expr(&args[0])?;
                            let pass = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("duanyan_wifi_connect({}, {});", ssid, pass)));
                        }
                        // Phase F: AI
                        "Brain.init" => {
                            let path = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("duanyan_brain_init({});", path)));
                        }
                        "Brain.classify" if args.len() >= 2 => {
                            let input = self.generate_expr(&args[0])?;
                            let labels = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("duanyan_brain_classify((float*){}, {}, (const char**){}, {});",
                                input, "/* input_len */", labels, "/* n_labels */")));
                        }
                        // Phase 1.2: Servo/Motor API
                        "Servo.attach" => {
                            let pin = self.generate_expr(&args[0])?;
                            let min_us = if args.len() >= 2 { self.generate_expr(&args[1])? } else { "500".to_string() };
                            let max_us = if args.len() >= 3 { self.generate_expr(&args[2])? } else { "2500".to_string() };
                            return Ok(Some(format!("duanyan_servo_attach({}, {}, {});", pin, min_us, max_us)));
                        }
                        "Servo.write" if args.len() >= 2 => {
                            let pin = self.generate_expr(&args[0])?;
                            let angle = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("duanyan_servo_write({}, {});", pin, angle)));
                        }
                        "Motor.init" if args.len() >= 3 => {
                            let in1 = self.generate_expr(&args[0])?;
                            let in2 = self.generate_expr(&args[1])?;
                            let ena = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("duanyan_motor_init({}, {}, {});", in1, in2, ena)));
                        }
                        "Motor.setSpeed" if args.len() >= 2 => {
                            let id = self.generate_expr(&args[0])?;
                            let speed = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("duanyan_motor_set_speed({}, {});", id, speed)));
                        }
                        "Motor.brake" => {
                            let id = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("duanyan_motor_brake({});", id)));
                        }
                        // v1.6 P0: Joint.* (duanyan:servo)
                        // Joint.bind(joint, backend, chan_id, min, max, home) — backend: 0=pca9685,1=zx361
                        "Joint.bind" if args.len() >= 6 => {
                            let be = self.generate_expr(&args[1])?;
                            let ch = self.generate_expr(&args[2])?;
                            let mn = self.generate_expr(&args[3])?;
                            let mx = self.generate_expr(&args[4])?;
                            let hm = self.generate_expr(&args[5])?;
                            return Ok(Some(format!("duanyan_joint_bind({}, {}, {}, {}, {}, {});", first_arg, be, ch, mn, mx, hm)));
                        }
                        "Joint.write" if args.len() >= 2 => {
                            let deg = self.generate_expr(&args[1])?;
                            let ms = if args.len() >= 3 { self.generate_expr(&args[2])? } else { "0".to_string() };
                            return Ok(Some(format!("duanyan_joint_write({}, {}, {});", first_arg, deg, ms)));
                        }
                        "Joint.read" => {
                            return Ok(Some(format!("duanyan_joint_read({})", first_arg)));
                        }
                        // v1.6 P1: Safety.* (duanyan:servo)
                        "Safety.check" if args.len() >= 3 => {
                            let temp = self.generate_expr(&args[1])?;
                            let stall = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("_duanyan_safety_check({}, {}, {})", first_arg, temp, stall)));
                        }
                        "Safety.limits" if args.len() >= 3 => {
                            let temp = self.generate_expr(&args[1])?;
                            let stall = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("duanyan_safety_limits({}, {}, {});", first_arg, temp, stall)));
                        }
                        // v1.6 P2: Traj.* / Joint.glide (duanyan:robot)
                        "Traj.quintic" if args.len() >= 3 => {
                            let qf = self.generate_expr(&args[1])?;
                            let t = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("_duanyan_quintic_plan({}, {}, {})", first_arg, qf, t)));
                        }
                        "Traj.sample" if args.len() >= 2 => {
                            let t = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("_duanyan_quintic_sample({}, {})", first_arg, t)));
                        }
                        "Traj.trapezoid" if args.len() >= 3 => {
                            let vmax = self.generate_expr(&args[1])?;
                            let amax = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("_duanyan_trapezoid_plan({}, {}, {})", first_arg, vmax, amax)));
                        }
                        "Joint.glide" if args.len() >= 5 => {
                            let from = self.generate_expr(&args[1])?;
                            let to = self.generate_expr(&args[2])?;
                            let t = self.generate_expr(&args[3])?;
                            let steps = self.generate_expr(&args[4])?;
                            return Ok(Some(format!("_duanyan_joint_glide({}, {}, {}, {}, {});", first_arg, from, to, t, steps)));
                        }
                        // v1.6 P3: ModeB.* (duanyan:modeb — Mode-B 线协议直发)
                        "ModeB.send" if args.len() >= 3 => {
                            let pl = self.generate_expr(&args[1])?;
                            let len = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("_duanyan_modeb_send({}, {}, {});", first_arg, pl, len)));
                        }
                        "ModeB.frame" if args.len() >= 3 => {
                            let pl = self.generate_expr(&args[1])?;
                            let len = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("_duanyan_modeb_send({}, {}, {})", first_arg, pl, len)));
                        }
                        "ModeB.jointSet" if args.len() >= 4 => {
                            let ang = self.generate_expr(&args[1])?;
                            let lo = self.generate_expr(&args[2])?;
                            let hi = self.generate_expr(&args[3])?;
                            return Ok(Some(format!("_duanyan_modeb_joint_set({}, {}, {}, {});", first_arg, ang, lo, hi)));
                        }
                        "ModeB.armSync" if args.len() >= 2 => {
                            let n = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("_duanyan_modeb_arm_sync({}, {});", first_arg, n)));
                        }
                        "ModeB.heartbeat" if args.len() >= 1 => {
                            return Ok(Some(format!("_duanyan_modeb_heartbeat({});", first_arg)));
                        }
                        // Phase 1.3: setInterval / setTimeout → 调度器
                        "setInterval" if args.len() >= 2 => {
                            let func_name = self.resolve_scheduler_callback(&args[0])?;
                            let ms_expr = self.generate_expr(&args[1])?;
                            let ms: u32 = ms_expr.parse().unwrap_or(100);
                            self.scheduler.add_interval(&func_name, ms);
                            return Ok(Some(format!("/* setInterval({}, {}) — scheduled */", func_name, ms)));
                        }
                        "setTimeout" if args.len() >= 2 => {
                            let func_name = self.resolve_scheduler_callback(&args[0])?;
                            let ms_expr = self.generate_expr(&args[1])?;
                            let ms: u32 = ms_expr.parse().unwrap_or(100);
                            self.scheduler.add_timeout(&func_name, ms);
                            return Ok(Some(format!("/* setTimeout({}, {}) — scheduled */", func_name, ms)));
                        }
                        _ => {}
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn find_gpio_pin(&self, var_name: &str) -> Option<String> {
        for (name, pin) in &self.gpio_pins {
            if name == var_name {
                return Some(pin.clone());
            }
        }
        None
    }

    /// Phase 4: 判断表达式是否返回字符串 (const char*)
    fn is_string_expr(&self, expr: &Expr) -> bool {
        match expr {
            Expr::Call { name, .. } => {
                matches!(name.as_str(), "Chip.modelName" | "Chip.model" | "Chip.sdkVersion" | "Chip.deviceId" | "Chip.mac")
            }
            Expr::StringLit(_) => true,
            _ => false,
        }
    }

    /// Phase 1.3: 解析 setInterval/setTimeout 的回调参数，返回 C 函数名
    fn resolve_scheduler_callback(&mut self, arg: &Expr) -> Result<String, String> {
        match arg {
            // 函数引用: setInterval(grabAndPour, 5000)
            Expr::Ident(name) => Ok(name.clone()),
            // 箭头函数: setInterval(() => { readSensors(); }, 10)
            Expr::ArrowFunc { params, body, .. } => {
                self.arrow_counter += 1;
                let func_name = format!("_duanyan_task_fn_{}", self.arrow_counter);
                let mut code = String::new();
                code.push_str(&format!("int32_t {}(", func_name));
                if params.is_empty() {
                    code.push_str("void");
                } else {
                    let ps: Vec<String> = params.iter().map(|p| format!("int32_t {}", p)).collect();
                    code.push_str(&ps.join(", "));
                }
                code.push_str(")\n{\n");
                self.indent = 1;
                match body {
                    ArrowBody::Expr(expr) => {
                        let c_expr = self.generate_expr(expr)?;
                        code.push_str(&format!("    return {};\n", c_expr));
                    }
                    ArrowBody::Block(stmts) => {
                        for stmt in stmts {
                            code.push_str(&self.generate_statement(&stmt.stmt)?);
                        }
                    }
                }
                self.indent = 0;
                code.push_str("}\n");
                self.hoisted_functions.push(code);
                Ok(func_name)
            }
            _ => Err("调度器回调必须是函数名或箭头函数".to_string()),
        }
    }

    fn generate_expr(&mut self, expr: &Expr) -> Result<String, String> {
        // L5: 表达式位置/嵌套的数组高阶方法无法静态降级 → 显式报错（避免发射不存在的 C 函数调用）
        // 注：slice 与字符串方法同名，仅当对象确为已知数组时才判定为数组 HOF，不误伤 str.slice()
        if let Some((arr, method, _)) = Self::match_array_hof(expr) {
            let is_array_hof = if method == "slice" {
                self.array_len(&arr).is_some() || self.dynamic_len_arrays.contains(&arr)
            } else {
                true // map/filter/forEach/reduce 非字符串方法
            };
            if is_array_hof {
                return Err(format!("扩展 4: 暂不支持表达式位置或嵌套的数组高阶方法 {}（只能作为语句或 let/const 初始化值）", method));
            }
        }
        match expr {
            Expr::Number(n) => Ok(format!("{}", n)),
            Expr::StringLit(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                Ok(format!("\"{}\"", escaped))
            }
            Expr::Ident(name) => Ok(name.clone()),
            Expr::Bool(b) => Ok(if *b { "1".to_string() } else { "0".to_string() }),
            Expr::Null => Ok("0".to_string()),
            Expr::BinOp { op, left, right } => {
                let l = self.generate_expr(left)?;
                let r = self.generate_expr(right)?;
                // L3: 除数为字面量 0 → 编译期拦截（避免 C 运行期除零 UB）
                if matches!(op, BinOp::Div) && matches!(&**right, Expr::Number(0)) {
                    return Err("除零：除数为字面量 0（编译期拦截）".to_string());
                }
                // 扩展 3: q15/q16 定点乘除重写（确定性、无 FPU、64 位中间量防溢出）
                // M2: 仅当两侧均为同一标度定点时才移位重写；定点×普通整数走普通算术（标度天然正确）
                let lsh = self.fixed_shift_of(left);
                let rsh = self.fixed_shift_of(right);
                if let (Some(sa), Some(sb)) = (lsh, rsh) {
                    if sa != sb {
                        return Err("扩展 3: 不支持 q15 与 q16 混合定点运算（标度不一致，无法确定移位量）".to_string());
                    }
                    let sh = sa;
                    match op {
                        BinOp::Mul => {
                            return Ok(format!("((int32_t)(((int64_t)({}) * (int64_t)({})) >> {}))", l, r, sh));
                        }
                        BinOp::Div => {
                            return Ok(format!("((int32_t)((((int64_t)({}) << {}) / (int64_t)({}))))", l, sh, r));
                        }
                        _ => {}
                    }
                }
                let op_str = match op {
                    BinOp::Add => "+",
                    BinOp::Sub => "-",
                    BinOp::Mul => "*",
                    BinOp::Div => "/",
                    BinOp::Mod => "%",
                    BinOp::Eq | BinOp::StrictEq => "==",
                    BinOp::NotEq | BinOp::StrictNotEq => "!=",
                    BinOp::Lt => "<",
                    BinOp::Gt => ">",
                    BinOp::Le => "<=",
                    BinOp::Ge => ">=",
                    BinOp::And => "&&",
                    BinOp::Or => "||",
                    // 位运算债清偿: & | ^ << >> >>>
                    BinOp::BitAnd => "&",
                    BinOp::BitOr => "|",
                    BinOp::BitXor => "^",
                    // 移位量按 JS 32 位语义掩码 & 31：既避免 C 语言移位 >= 位宽的未定义行为，
                    // 又与字节码引擎的掩码口径保持语义一致（引擎为 i64 域 & 63）。
                    BinOp::Shl => {
                        return Ok(format!("({} << (({}) & 31))", l, r));
                    }
                    BinOp::Shr => {
                        return Ok(format!("({} >> (({}) & 31))", l, r));
                    }
                    // >>> 按 JS 32 位无符号语义（C 路径整数为 int32_t）
                    BinOp::UShr => {
                        return Ok(format!("((int32_t)((uint32_t)({}) >> (({}) & 31)))", l, r));
                    }
                };
                Ok(format!("({} {} {})", l, op_str, r))
            }
            Expr::Call { name, args } => {
                // 先检查硬件 API 方法
                if let Some(hw_line) = self.try_generate_hw_method(expr)? {
                    return Ok(hw_line.trim_end_matches(';').to_string());
                }
                // Math intrinsics
                match name.as_str() {
                    "Math.max" if args.len() == 2 => {
                        let a = self.generate_expr(&args[0])?;
                        let b = self.generate_expr(&args[1])?;
                        return Ok(format!("(({} > {}) ? {} : {})", a, b, a, b));
                    }
                    "Math.min" if args.len() == 2 => {
                        let a = self.generate_expr(&args[0])?;
                        let b = self.generate_expr(&args[1])?;
                        return Ok(format!("(({} < {}) ? {} : {})", a, b, a, b));
                    }
                    "Math.abs" if args.len() == 1 => {
                        let a = self.generate_expr(&args[0])?;
                        return Ok(format!("(({} < 0) ? -({}) : ({}))", a, a, a));
                    }
                    _ => {}
                }
                // 普通函数调用
                let c_args: Vec<String> = args.iter()
                    .map(|a| self.generate_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("{}({})", name, c_args.join(", ")))
            }
            Expr::Array(elems) => {
                let items: Vec<String> = elems.iter()
                    .map(|e| self.generate_expr(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("{{ {} }}", items.join(", ")))
            }
            Expr::Index { array, index } => {
                let arr = self.generate_expr(array)?;
                let idx = self.generate_expr(index)?;
                // Phase 2.4: 对已知长度的数组生成边界检查
                if let Expr::Ident(arr_name) = array.as_ref() {
                    if let Some((_, len)) = self.array_vars.iter().find(|(n, _)| n == arr_name) {
                        self.needs_arr_helper = true;
                        return Ok(format!("_duanyan_arr_get({}, {}, {})", arr, len, idx));
                    }
                }
                Ok(format!("{}[{}]", arr, idx))
            }
            Expr::MathIntrinsic { op, args } => {
                let a = self.generate_expr(&args[0])?;
                match op {
                    MathOp::Max if args.len() >= 2 => {
                        let b = self.generate_expr(&args[1])?;
                        Ok(format!("(({} > {}) ? {} : {})", a, b, a, b))
                    }
                    MathOp::Min if args.len() >= 2 => {
                        let b = self.generate_expr(&args[1])?;
                        Ok(format!("(({} < {}) ? {} : {})", a, b, a, b))
                    }
                    MathOp::Abs => Ok(format!("(({} < 0) ? -({}) : ({}))", a, a, a)),
                    MathOp::Sqrt => {
                        self.needs_math_helper = true;
                        Ok(format!("duanyan_isqrt({})", a))
                    }
                    MathOp::Pow => {
                        self.needs_math_helper = true;
                        if args.len() < 2 {
                            return Err("Math.pow 需要 2 个参数".to_string());
                        }
                        let b = self.generate_expr(&args[1])?;
                        Ok(format!("duanyan_ipow({}, {})", a, b))
                    }
                    _ => Ok(a),
                }
            }
            Expr::ObjectLit { keys, values } => {
                // 简化为 C struct 初始化
                self.needs_stdint = true;
                let mut parts = Vec::new();
                for (k, v) in keys.iter().zip(values.iter()) {
                    let val = self.generate_expr(v)?;
                    parts.push(format!(".{} = {}", k, val));
                }
                Ok(format!("{{ {} }}", parts.join(", ")))
            }
            Expr::PropAccess { object, property } => {
                // 扩展 2: 寄存器位域读 → masked read
                if let Expr::Ident(reg) = object.as_ref() {
                    if let Some(expr) = self.register_read_expr(reg, property) {
                        return Ok(expr);
                    }
                }
                let obj = self.generate_expr(object)?;
                Ok(format!("{}.{}", obj, property))
            }
            Expr::Ternary { condition, then_expr, else_expr } => {
                let c = self.generate_expr(condition)?;
                let t = self.generate_expr(then_expr)?;
                let e = self.generate_expr(else_expr)?;
                Ok(format!("({} ? {} : {})", c, t, e))
            }
            Expr::ArrowFunc { params, body, .. } => {
                // 提升箭头函数为全局函数
                self.arrow_counter += 1;
                let func_name = format!("_duanyan_arrow_{}", self.arrow_counter);
                let mut code = String::new();
                code.push_str(&format!("int32_t {}(", func_name));
                if params.is_empty() {
                    code.push_str("void");
                } else {
                    let ps: Vec<String> = params.iter().map(|p| format!("int32_t {}", p)).collect();
                    code.push_str(&ps.join(", "));
                }
                code.push_str(")\n{\n");
                match body {
                    ArrowBody::Expr(expr) => {
                        let e = self.generate_expr(expr)?;
                        code.push_str(&format!("    return {};\n", e));
                    }
                    ArrowBody::Block(stmts) => {
                        let saved_indent = self.indent;
                        self.indent = 1;
                        for s in stmts {
                            code.push_str(&self.generate_statement(&s.stmt)?);
                        }
                        self.indent = saved_indent;
                    }
                }
                code.push_str("}\n");
                self.hoisted_functions.push(code);
                Ok(func_name)
            }
            Expr::TemplateStr { parts } => {
                self.needs_stdio = true;
                self.needs_string = true;
                // 用 snprintf 生成
                let mut fmt = String::new();
                let mut fmt_args = Vec::new();
                for part in parts {
                    match part {
                        TemplateExprPart::Text(t) => {
                            fmt.push_str(&t.replace('%', "%%"));
                        }
                        TemplateExprPart::Expr(e) => {
                            match e {
                                Expr::StringLit(_) => {
                                    fmt.push_str("%s");
                                }
                                _ => {
                                    fmt.push_str("%d");
                                }
                            }
                            fmt_args.push(self.generate_expr(e)?);
                        }
                    }
                }
                if fmt_args.is_empty() {
                    Ok(format!("\"{}\"", fmt))
                } else {
                    // 使用 snprintf 到临时缓冲区
                    let _buf_name = format!("_duanyan_tmpl_{}", self.arrow_counter);
                    self.arrow_counter += 1;
                    // 在语句层面处理更好，但这里先返回一个简化版本
                    Ok(format!("\"{}\" /* TODO: template with args */", fmt))
                }
            }
            Expr::StringMethod { object, method, args } => {
                self.needs_string = true;
                let obj = self.generate_expr(object)?;
                match method.as_str() {
                    "length" => Ok(format!("(int32_t)strlen({})", obj)),
                    "charAt" if !args.is_empty() => {
                        let idx = self.generate_expr(&args[0])?;
                        Ok(format!("(int32_t){}[{}]", obj, idx))
                    }
                    "slice" if args.len() >= 2 => {
                        let start = self.generate_expr(&args[0])?;
                        let end = self.generate_expr(&args[1])?;
                        Ok(format!("/* slice */ strncpy(_buf, {} + {}, {} - {})", obj, start, end, start))
                    }
                    "slice" if args.len() == 1 => {
                        let start = self.generate_expr(&args[0])?;
                        Ok(format!("({} + {})", obj, start))
                    }
                    // 语言完备性批 2：纯表达式形式的谓词/访问器（返回 0/1 或字符码）
                    "startsWith" if !args.is_empty() => {
                        let p = self.generate_expr(&args[0])?;
                        Ok(format!("(strncmp({}, {}, strlen({})) == 0 ? 1 : 0)", obj, p, p))
                    }
                    "endsWith" if !args.is_empty() => {
                        let p = self.generate_expr(&args[0])?;
                        Ok(format!("(strlen({}) >= strlen({}) && strcmp({} + strlen({}) - strlen({}), {}) == 0 ? 1 : 0)", obj, p, obj, obj, p, p))
                    }
                    "includes" if !args.is_empty() => {
                        let p = self.generate_expr(&args[0])?;
                        Ok(format!("(strstr({}, {}) != 0 ? 1 : 0)", obj, p))
                    }
                    "charCodeAt" if !args.is_empty() => {
                        let idx = self.generate_expr(&args[0])?;
                        Ok(format!("(int32_t){}[{}]", obj, idx))
                    }
                    _ => Ok(format!("/* unknown string method: {} */ 0", method)),
                }
            }
            // Phase v1.5: await expr → 状态机 poll 调用
            Expr::Await { expr } => {
                // await 内部表达式直接编译，状态机在 poll 中处理挂起/恢复
                self.generate_expr(expr)
            }
            // Phase 1.1: Promise.all([...]) → 并行等待，所有子任务完成才继续
            Expr::PromiseAll { promises } => {
                let n = promises.len();
                if n == 0 {
                    return Ok("1".to_string()); // 空数组立即完成
                }
                self.promise_counter += 1;
                let id = self.promise_counter;
                let mask = (1u32 << n) - 1;
                // 生成辅助函数：轮询所有子任务，返回是否全部完成
                let mut helper = String::new();
                helper.push_str(&format!("/* Promise.all #{}: {} parallel tasks */\n", id, n));
                helper.push_str(&format!("static uint8_t _duanyan_pall_{}_done = 0;\n", id));
                helper.push_str(&format!("static int32_t _duanyan_pall_{}_results[{}];\n", id, n));
                helper.push_str(&format!("static uint8_t _duanyan_promise_all_{}(void) {{\n", id));
                for (i, p) in promises.iter().enumerate() {
                    let expr_code = self.generate_expr(p)?;
                    helper.push_str(&format!("    if (!(_duanyan_pall_{}_done & {})) {{\n", id, 1u32 << i));
                    helper.push_str(&format!("        int32_t _r = {};\n", expr_code));
                    helper.push_str(&format!("        _duanyan_pall_{}_results[{}] = _r;\n", id, i));
                    helper.push_str(&format!("        _duanyan_pall_{}_done |= {};\n", id, 1u32 << i));
                    helper.push_str("    }\n");
                }
                helper.push_str(&format!("    return (_duanyan_pall_{}_done == 0x{:X}) ? 1 : 0;\n", id, mask));
                helper.push_str("}\n");
                self.promise_helpers.push(helper);
                Ok(format!("_duanyan_promise_all_{}()", id))
            }
            // Phase 1.1: Promise.race([...]) → 竞争等待，任一子任务完成即继续
            Expr::PromiseRace { promises } => {
                let n = promises.len();
                if n == 0 {
                    return Ok("0".to_string());
                }
                self.promise_counter += 1;
                let id = self.promise_counter;
                // 生成辅助函数：轮询所有子任务，返回第一个完成的结果
                let mut helper = String::new();
                helper.push_str(&format!("/* Promise.race #{}: {} racing tasks */\n", id, n));
                helper.push_str(&format!("static uint8_t _duanyan_prace_{}_done = 0;\n", id));
                helper.push_str(&format!("static int32_t _duanyan_prace_{}_result = 0;\n", id));
                helper.push_str(&format!("static uint8_t _duanyan_promise_race_{}(void) {{\n", id));
                helper.push_str(&format!("    if (_duanyan_prace_{}_done) return 1;\n", id));
                for (i, p) in promises.iter().enumerate() {
                    let expr_code = self.generate_expr(p)?;
                    helper.push_str(&format!("    {{ int32_t _r = {}; if (_r != 0) {{ _duanyan_prace_{}_result = _r; _duanyan_prace_{}_done = {}; return 1; }} }}\n",
                        expr_code, id, id, i + 1));
                }
                helper.push_str("    return 0;\n");
                helper.push_str("}\n");
                helper.push_str(&format!("static int32_t _duanyan_promise_race_{}_value(void) {{ return _duanyan_prace_{}_result; }}\n", id, id));
                self.promise_helpers.push(helper);
                Ok(format!("_duanyan_promise_race_{}()", id))
            }
        }
    }

    /// Phase 3.3: 生成定时器中断初始化代码
    fn generate_timer_isr_init(&self) -> String {
        let mut code = String::new();
        code.push_str("    /* DUANYAN timer ISR init */\n");

        for (func_name, timer, period_us) in &self.interrupt_fns {
            match self.target {
                CTarget::Esp32 => {
                    // ESP32: 使用 ESP-IDF timer driver
                    code.push_str(&format!("    // @interrupt(\"{}\", period_us: {}) -> {}\n", timer, period_us, func_name));
                    code.push_str(&format!("    // ESP32 timer: {} @ {}us interval\n", timer, period_us));
                    code.push_str(&format!("    // TODO: timer_config_t + timer_init + timer_set_alarm_value({}, {})\n", timer, period_us));
                    code.push_str(&format!("    // ISR callback: {}()\n", func_name));
                }
                CTarget::Stm32 => {
                    // STM32: HAL TIM
                    let freq_mhz = 72; // STM32F103 default
                    let prescaler = freq_mhz - 1; // 1MHz tick
                    let period = period_us; // 直接映射
                    code.push_str(&format!("    // @interrupt(\"{}\", period_us: {}) -> {}\n", timer, period_us, func_name));
                    code.push_str(&format!("    // STM32 HAL: {} Prescaler={} Period={}\n", timer, prescaler, period));
                    code.push_str(&format!("    // HAL_TIM_PeriodElapsedCallback -> {}()\n", func_name));
                }
                CTarget::ArduinoAvr => {
                    // Arduino: ISR(TIMERx_COMPA_vect)
                    code.push_str(&format!("    // @interrupt(\"{}\", period_us: {}) -> {}\n", timer, period_us, func_name));
                    code.push_str(&format!("    // Arduino AVR: ISR({}_COMPA_vect) @ {}us\n", timer, period_us));
                    code.push_str(&format!("    // Setup: TCCR2A/B, OCR2A = {}\n", period_us / 16));
                }
                _ => {
                    code.push_str(&format!("    // @interrupt(\"{}\", period_us: {}) -> {}\n", timer, period_us, func_name));
                }
            }
        }
        code.push('\n');
        code
    }

    /// 箭头函数赋值提升
    /// Phase 2.3: 对象字面量 → C struct 声明
    fn generate_object_let(&mut self, name: &str, keys: &[String], values: &[Expr]) -> Result<String, String> {
        let struct_name = format!("_duanyan_obj_{}", name);

        // 生成 struct 定义 (只生成一次)
        if !self.struct_defs.iter().any(|s| s.contains(&struct_name)) {
            let mut def = String::new();
            def.push_str(&format!("struct {} {{\n", struct_name));
            for key in keys {
                def.push_str(&format!("    int32_t {};\n", key));
            }
            def.push_str("};\n");
            self.struct_defs.push(def);
        }

        // 跟踪对象变量
        self.object_vars.push((name.to_string(), keys.to_vec()));

        // 生成初始化
        let mut inits = Vec::new();
        for (k, v) in keys.iter().zip(values.iter()) {
            let val = self.generate_expr(v)?;
            inits.push(format!(".{} = {}", k, val));
        }

        let pad = " ".repeat(self.indent * 4);
        Ok(format!("{}struct {} {} = {{ {} }};\n", pad, struct_name, name, inits.join(", ")))
    }

    fn generate_arrow_assignment(&mut self, name: &str, params: &[String], body: &ArrowBody) -> Result<String, String> {
        self.arrow_counter += 1;
        let func_name = format!("_duanyan_fn_{}_{}", name, self.arrow_counter);

        let mut code = String::new();
        code.push_str(&format!("int32_t {}(", func_name));
        if params.is_empty() {
            code.push_str("void");
        } else {
            let ps: Vec<String> = params.iter().map(|p| format!("int32_t {}", p)).collect();
            code.push_str(&ps.join(", "));
        }
        code.push_str(")\n{\n");
        match body {
            ArrowBody::Expr(expr) => {
                let e = self.generate_expr(expr)?;
                code.push_str(&format!("    return {};\n", e));
            }
            ArrowBody::Block(stmts) => {
                let saved_indent = self.indent;
                self.indent = 1;
                for s in stmts {
                    code.push_str(&self.generate_statement(&s.stmt)?);
                }
                self.indent = saved_indent;
            }
        }
        code.push_str("}\n");
        self.hoisted_functions.push(code);

        // 在 main 中赋值
        let pad = self.indent_str();
        Ok(format!("{}int32_t (*{})({}) = {};\n", pad, name,
            if params.is_empty() { "void".to_string() } else { params.iter().map(|_| "int32_t").collect::<Vec<_>>().join(", ") },
            func_name))
    }

    fn generate_for_init(&mut self, stmt: &Statement) -> Result<String, String> {
        match stmt {
            Statement::Let { name, value } => {
                let val = self.generate_expr(value)?;
                if let Expr::Number(n) = value {
                    self.lint_width_literal(name, *n)?;
                }
                let cty = self.c_width_type(name);
                let expr_out = match self.width_cast(name) {
                    Some(cast) => format!("{}({})", cast, val),
                    None => val.clone(),
                };
                Ok(format!("{} {} = {}", cty, name, expr_out))
            }
            Statement::Assign { name, value } => {
                let val = self.generate_expr(value)?;
                Ok(format!("{} = {}", name, val))
            }
            _ => Ok(String::new()),
        }
    }

    fn generate_for_update(&mut self, stmt: &Statement) -> Result<String, String> {
        match stmt {
            Statement::PostInc(name) => Ok(format!("{}++", name)),
            Statement::PostDec(name) => Ok(format!("{}--", name)),
            Statement::Assign { name, value } => {
                let val = self.generate_expr(value)?;
                Ok(format!("{} = {}", name, val))
            }
            _ => Ok(String::new()),
        }
    }

    fn indent_str(&self) -> String {
        "    ".repeat(self.indent)
    }

    fn finalize(self) -> String {
        self.output
    }
}

/// Phase v1.5: 表达式简述 (用于 C 注释)
fn format_expr_short(expr: &Expr) -> String {
    match expr {
        Expr::Call { name, .. } => format!("{}(...)", name),
        Expr::Ident(name) => name.clone(),
        Expr::PromiseAll { promises } => format!("Promise.all([{}])", promises.len()),
        Expr::PromiseRace { promises } => format!("Promise.race([{}])", promises.len()),
        Expr::Number(n) => format!("{}", n),
        _ => "expr".to_string(),
    }
}


// 单元测试已拆分至 c_codegen_tests.rs (2026-08 工程治理)
#[cfg(test)]
#[path = "../c_codegen_tests.rs"]
mod tests;
