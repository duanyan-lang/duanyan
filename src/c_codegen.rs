//! C 代码生成器 (AST → C transpiler)
//!
//! Phase C: ESP32 后端 | Phase D: Arduino 后端
//! 将 SNARjs AST 转译为 C 源码，供 ESP-IDF / Arduino Core / STM32 HAL 编译

use crate::parser::{
    Program, Statement, Expr, BinOp, MathOp,
    Function, ArrowBody, TemplateExprPart,
};
use crate::hardware_api::HardwareModule;
use std::collections::HashSet;

/// C 编译目标平台
#[derive(Debug, Clone, PartialEq)]
pub enum CTarget {
    Esp32,
    Stm32,
    ArduinoAvr,
    ArduinoArm,
    RaspberryPi,
}

impl CTarget {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "esp32" | "esp" => Some(Self::Esp32),
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
}

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
    let mut gen = CCodeGenerator::new(target.clone());
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
                            self.uses_wifi = true;
                            for item in items {
                                self.hw_imports.insert(format!("esp32:{}", item));
                            }
                        }
                        HardwareModule::Ai => {
                            self.uses_ai = true;
                            for item in items {
                                self.hw_imports.insert(format!("ai:{}", item));
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

        // 输出提升的箭头函数
        for hoisted in &self.hoisted_functions.clone() {
            self.output.push_str(hoisted);
            self.output.push('\n');
        }

        // 输出用户定义的函数
        for func_code in &func_bodies {
            self.output.push_str(func_code);
            self.output.push('\n');
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
            self.output.push_str("}\n\n");
            self.output.push_str("void loop() {\n");
            self.output.push_str(&loop_body);
            self.output.push_str("}\n");
        } else if self.target == CTarget::Esp32 {
            self.output.push_str("void app_main(void)\n{\n");
            self.output.push_str(&main_body);
            self.output.push_str("}\n");
        } else {
            self.output.push_str("int main(void)\n{\n");
            self.output.push_str(&main_body);
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
        self.output.push_str("/* Auto-generated by SNARjs v");
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
                if self.hw_imports.contains("Timer") {
                    // FreeRTOS task delay already included
                }
                if self.uses_wifi {
                    self.output.push_str("#include \"esp_wifi.h\"\n");
                    self.output.push_str("#include \"esp_event.h\"\n");
                    self.output.push_str("#include \"nvs_flash.h\"\n");
                    self.output.push_str("#include \"esp_log.h\"\n");
                }
                if self.uses_ai {
                    self.output.push_str("#include \"snar_ai.h\"\n");
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

    /// 生成硬件 API 辅助函数
    fn generate_hw_helpers(&mut self) {
        match self.target {
            CTarget::Esp32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* SNARjs GPIO helpers */\n");
                    self.output.push_str("static inline void snar_gpio_output(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_INPUT);\n");
                    self.output.push_str("    gpio_set_pull_mode(pin, GPIO_PULLUP_ONLY);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_high(int pin) { gpio_set_level(pin, 1); }\n");
                    self.output.push_str("static inline void snar_gpio_low(int pin)  { gpio_set_level(pin, 0); }\n");
                    self.output.push_str("static inline int  snar_gpio_read(int pin) { return gpio_get_level(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* SNARjs UART helpers */\n");
                    self.output.push_str("static inline void snar_uart_init(int baud) {\n");
                    self.output.push_str("    uart_config_t cfg = {\n");
                    self.output.push_str("        .baud_rate = baud,\n");
                    self.output.push_str("        .data_bits = UART_DATA_8_BITS,\n");
                    self.output.push_str("        .parity    = UART_PARITY_DISABLE,\n");
                    self.output.push_str("        .stop_bits = UART_STOP_BITS_1,\n");
                    self.output.push_str("        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    uart_param_config(UART_NUM_0, &cfg);\n");
                    self.output.push_str("    uart_driver_install(UART_NUM_0, 1024, 0, 0, NULL, 0);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_uart_write(const char* str) {\n");
                    self.output.push_str("    uart_write_bytes(UART_NUM_0, str, strlen(str));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* SNARjs PWM (LEDC) helpers */\n");
                    self.output.push_str("static int _snar_pwm_channel = 0;\n");
                    self.output.push_str("static inline int snar_pwm_output(int pin, int freq) {\n");
                    self.output.push_str("    int ch = _snar_pwm_channel++;\n");
                    self.output.push_str("    ledc_timer_config_t tcfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .duty_resolution = LEDC_TIMER_8_BIT,\n");
                    self.output.push_str("        .timer_num = LEDC_TIMER_0,\n");
                    self.output.push_str("        .freq_hz = freq,\n");
                    self.output.push_str("        .clk_cfg = LEDC_AUTO_CLK,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_timer_config(&tcfg);\n");
                    self.output.push_str("    ledc_channel_config_t ccfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .channel = ch,\n");
                    self.output.push_str("        .timer_sel = LEDC_TIMER_0,\n");
                    self.output.push_str("        .intr_type = LEDC_INTR_DISABLE,\n");
                    self.output.push_str("        .gpio_num = pin,\n");
                    self.output.push_str("        .duty = 0,\n");
                    self.output.push_str("        .hpoint = 0,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_channel_config(&ccfg);\n");
                    self.output.push_str("    return ch;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_pwm_duty(int channel, int duty) {\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, channel, duty);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* SNARjs ADC helpers */\n");
                    self.output.push_str("static inline void snar_adc_init(int channel) {\n");
                    self.output.push_str("    adc1_config_width(ADC_WIDTH_BIT_12);\n");
                    self.output.push_str("    adc1_config_channel_atten(channel, ADC_ATTEN_DB_11);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int snar_adc_read(int channel) {\n");
                    self.output.push_str("    return adc1_get_raw(channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_wifi {
                    self.output.push_str("/* SNARjs WiFi helper */\n");
                    self.output.push_str("static void snar_wifi_connect(const char* ssid, const char* pass) {\n");
                    self.output.push_str("    nvs_flash_init();\n");
                    self.output.push_str("    esp_netif_init();\n");
                    self.output.push_str("    esp_event_loop_create_default();\n");
                    self.output.push_str("    esp_netif_create_default_wifi_sta();\n");
                    self.output.push_str("    wifi_init_config_t cfg = WIFI_INIT_CONFIG_DEFAULT();\n");
                    self.output.push_str("    esp_wifi_init(&cfg);\n");
                    self.output.push_str("    wifi_config_t wcfg = { .sta = { .threshold.authmode = WIFI_AUTH_WPA2_PSK } };\n");
                    self.output.push_str("    strncpy((char*)wcfg.sta.ssid, ssid, sizeof(wcfg.sta.ssid)-1);\n");
                    self.output.push_str("    strncpy((char*)wcfg.sta.password, pass, sizeof(wcfg.sta.password)-1);\n");
                    self.output.push_str("    esp_wifi_set_mode(WIFI_MODE_STA);\n");
                    self.output.push_str("    esp_wifi_set_config(WIFI_IF_STA, &wcfg);\n");
                    self.output.push_str("    esp_wifi_start();\n");
                    self.output.push_str("    esp_wifi_connect();\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_ai {
                    self.output.push_str("/* SNARjs 千漉Mini AI helpers */\n");
                    self.output.push_str("static snar_brain_t _snar_brain;\n");
                    self.output.push_str("static inline int snar_brain_init(const char* model_path) {\n");
                    self.output.push_str("    return snar_ai_load_model(&_snar_brain, model_path);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int snar_brain_classify(float* input, int input_len,\n");
                    self.output.push_str("                                       const char** labels, int n_labels) {\n");
                    self.output.push_str("    return snar_ai_classify(&_snar_brain, input, input_len, labels, n_labels);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::ArduinoAvr => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* SNARjs GPIO helpers (Arduino AVR) */\n");
                    self.output.push_str("static inline void snar_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void snar_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void snar_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void snar_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  snar_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* SNARjs UART helpers (Arduino) */\n");
                    self.output.push_str("static inline void snar_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void snar_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* SNARjs Timer helpers (Arduino) */\n");
                    self.output.push_str("static inline void snar_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* SNARjs PWM helpers (Arduino AVR, 8-bit duty 0-255) */\n");
                    self.output.push_str("static inline void snar_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void snar_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* SNARjs ADC helpers (Arduino AVR, 10-bit) */\n");
                    self.output.push_str("static inline int snar_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
            }
            CTarget::ArduinoArm => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* SNARjs GPIO helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void snar_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void snar_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void snar_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void snar_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  snar_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* SNARjs UART helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void snar_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void snar_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* SNARjs Timer helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void snar_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* SNARjs PWM helpers (Arduino ARM, 12-bit duty 0-4095) */\n");
                    self.output.push_str("static inline void snar_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void snar_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* SNARjs ADC helpers (Arduino ARM, 12-bit) */\n");
                    self.output.push_str("static inline int snar_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
            }
            CTarget::Stm32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* SNARjs GPIO helpers (STM32 HAL) */\n");
                    self.output.push_str("static GPIO_TypeDef* snar_gpio_port(int pin) {\n");
                    self.output.push_str("    if (pin >= 16) return GPIOB;\n");
                    self.output.push_str("    return GPIOA;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static uint16_t snar_gpio_pin_mask(int pin) {\n");
                    self.output.push_str("    return (uint16_t)(1 << (pin & 0xF));\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_output(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = snar_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_OUTPUT_PP;\n");
                    self.output.push_str("    gpio.Pull = GPIO_NOPULL;\n");
                    self.output.push_str("    gpio.Speed = GPIO_SPEED_FREQ_LOW;\n");
                    self.output.push_str("    HAL_GPIO_Init(snar_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = snar_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_INPUT;\n");
                    self.output.push_str("    gpio.Pull = GPIO_PULLUP;\n");
                    self.output.push_str("    HAL_GPIO_Init(snar_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_high(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(snar_gpio_port(pin), snar_gpio_pin_mask(pin), GPIO_PIN_SET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_low(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(snar_gpio_port(pin), snar_gpio_pin_mask(pin), GPIO_PIN_RESET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int snar_gpio_read(int pin) {\n");
                    self.output.push_str("    return HAL_GPIO_ReadPin(snar_gpio_port(pin), snar_gpio_pin_mask(pin));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* SNARjs Timer helpers (STM32 HAL) */\n");
                    self.output.push_str("static inline void snar_timer_delay(int ms) { HAL_Delay(ms); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* SNARjs UART helpers (STM32 HAL) */\n");
                    self.output.push_str("static UART_HandleTypeDef _snar_huart;\n");
                    self.output.push_str("static inline void snar_uart_init(int baud) {\n");
                    self.output.push_str("    _snar_huart.Instance = USART1;\n");
                    self.output.push_str("    _snar_huart.Init.BaudRate = baud;\n");
                    self.output.push_str("    _snar_huart.Init.WordLength = UART_WORDLENGTH_8B;\n");
                    self.output.push_str("    _snar_huart.Init.StopBits = UART_STOPBITS_1;\n");
                    self.output.push_str("    _snar_huart.Init.Parity = UART_PARITY_NONE;\n");
                    self.output.push_str("    _snar_huart.Init.Mode = UART_MODE_TX_RX;\n");
                    self.output.push_str("    HAL_UART_Init(&_snar_huart);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_uart_write(const char* str) {\n");
                    self.output.push_str("    HAL_UART_Transmit(&_snar_huart, (uint8_t*)str, strlen(str), HAL_MAX_DELAY);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* SNARjs PWM helpers (STM32 TIM, 16-bit duty) */\n");
                    self.output.push_str("static inline void snar_pwm_duty(int pin, int duty) {\n");
                    self.output.push_str("    /* User must configure TIM per pin; this is a placeholder */\n");
                    self.output.push_str("    (void)pin; (void)duty;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* SNARjs ADC helpers (STM32 HAL) */\n");
                    self.output.push_str("static ADC_HandleTypeDef _snar_hadc;\n");
                    self.output.push_str("static inline int snar_adc_read(int channel) {\n");
                    self.output.push_str("    ADC_ChannelConfTypeDef sConfig = {0};\n");
                    self.output.push_str("    sConfig.Channel = channel;\n");
                    self.output.push_str("    HAL_ADC_ConfigChannel(&_snar_hadc, &sConfig);\n");
                    self.output.push_str("    HAL_ADC_Start(&_snar_hadc);\n");
                    self.output.push_str("    HAL_ADC_PollForConversion(&_snar_hadc, 100);\n");
                    self.output.push_str("    int val = HAL_ADC_GetValue(&_snar_hadc);\n");
                    self.output.push_str("    HAL_ADC_Stop(&_snar_hadc);\n");
                    self.output.push_str("    return val;\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::RaspberryPi => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* SNARjs GPIO helpers (Linux sysfs) */\n");
                    self.output.push_str("static inline void snar_gpio_export(int pin) {\n");
                    self.output.push_str("    char buf[64];\n");
                    self.output.push_str("    int fd = open(\"/sys/class/gpio/export\", O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) {\n");
                    self.output.push_str("        int n = snprintf(buf, sizeof(buf), \"%d\", pin);\n");
                    self.output.push_str("        write(fd, buf, n);\n");
                    self.output.push_str("        close(fd);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_output(int pin) {\n");
                    self.output.push_str("    snar_gpio_export(pin);\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/direction\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"out\", 3); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_high(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"1\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void snar_gpio_low(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"0\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int snar_gpio_read(int pin) {\n");
                    self.output.push_str("    char path[128], val;\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_RDONLY);\n");
                    self.output.push_str("    if (fd >= 0) { read(fd, &val, 1); close(fd); return val - '0'; }\n");
                    self.output.push_str("    return -1;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* SNARjs Timer helpers (Linux usleep) */\n");
                    self.output.push_str("static inline void snar_timer_delay(int ms) { usleep(ms * 1000); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* SNARjs UART helpers (Linux serial) */\n");
                    self.output.push_str("static inline void snar_uart_init(int baud) { (void)baud; /* TODO: termios */ }\n");
                    self.output.push_str("static inline void snar_uart_write(const char* str) { printf(\"%s\", str); }\n\n");
                }
            }
            _ => {} // 其他平台后续实现
        }
    }

    fn generate_function(&mut self, func: &Function) -> Result<String, String> {
        // Phase v1.5: async 函数生成状态机
        if func.is_async {
            return self.generate_async_function(func);
        }

        let mut code = String::new();
        code.push_str(&format!("int32_t {}(", func.name));
        if func.params.is_empty() {
            code.push_str("void");
        } else {
            let params: Vec<String> = func.params.iter()
                .map(|p| format!("int32_t {}", p))
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
        code.push_str(&format!("/* async function {}() — SNARjs v1.5 状态机 */\n", func.name));
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
        code.push_str(&format!("snar_status_t {}_poll({}_ctx* ctx) {{\n", func.name, func.name));
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
                code.push_str("            return SNAR_PENDING;\n");
            } else if state.is_final {
                code.push_str("            return SNAR_READY;\n");
            }
            code.push_str("\n");
        }

        code.push_str("    }\n");
        code.push_str("    return SNAR_READY;\n");
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
        code.push_str(&format!("    snar_spawn((snar_status_t (*)(void*)){}_poll, &ctx);\n", func.name));
        code.push_str("}\n");

        Ok(code)
    }

    fn generate_statement(&mut self, stmt: &Statement) -> Result<String, String> {
        let pad = self.indent_str();
        match stmt {
            Statement::Let { name, value } => {
                // 检查是否是箭头函数赋值
                if let Expr::ArrowFunc { params, body, .. } = value {
                    return self.generate_arrow_assignment(name, params, body);
                }
                let c_expr = self.generate_expr(value)?;
                // 检查是否是 GPIO.output / GPIO.input 调用
                if let Expr::Call { name: call_name, args } = value {
                    if call_name == "GPIO.output" {
                        let pin_expr = self.generate_expr(&args[0])?;
                        self.gpio_pins.push((name.clone(), pin_expr.clone()));
                        return Ok(format!("{}int32_t {} = {}; snar_gpio_output({});\n", pad, name, pin_expr, pin_expr));
                    }
                    if call_name == "GPIO.input" {
                        let pull = if args.len() > 1 { "pullup" } else { "none" };
                        let pin_expr = self.generate_expr(&args[0])?;
                        self.gpio_pins.push((name.clone(), pin_expr.clone()));
                        return Ok(format!("{}int32_t {} = {}; snar_gpio_input_{}({});\n", pad, name, pin_expr, pull, pin_expr));
                    }
                    if call_name == "PWM.output" {
                        let pin = self.generate_expr(&args[0])?;
                        let freq = if args.len() > 1 { self.generate_expr(&args[1])? } else { "1000".to_string() };
                        return Ok(format!("{}int32_t {} = snar_pwm_output({}, {});\n", pad, name, pin, freq));
                    }
                    if call_name == "ADC.channel" {
                        let ch = self.generate_expr(&args[0])?;
                        return Ok(format!("{}int32_t {} = {}; snar_adc_init({});\n", pad, name, ch, ch));
                    }
                }
                Ok(format!("{}int32_t {} = {};\n", pad, name, c_expr))
            }
            Statement::Const { name, value } => {
                let c_expr = self.generate_expr(value)?;
                Ok(format!("{}const int32_t {} = {};\n", pad, name, c_expr))
            }
            Statement::Assign { name, value } => {
                let c_expr = self.generate_expr(value)?;
                Ok(format!("{}{} = {};\n", pad, name, c_expr))
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
            Statement::PostInc(name) => Ok(format!("{}{}++;\n", pad, name)),
            Statement::PostDec(name) => Ok(format!("{}{}--;\n", pad, name)),
            Statement::PropAssign { object, property, value } => {
                let val = self.generate_expr(value)?;
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
                let c_expr = self.generate_expr(expr)?;
                Ok(format!("{}{};\n", pad, c_expr))
            }
        }
    }

    /// 生成 console.log
    fn generate_console_log(&mut self, args: &[Expr], pad: &str) -> Result<String, String> {
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
                    code.push_str(&format!("{}printf(\"%d\\n\", (int32_t)({}));\n", pad, c_expr));
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
            // led.high() → snar_gpio_high(pin)
            Expr::Call { name, args } => {
                match name.as_str() {
                    // 对象方法形式: led.high() 被解析为 Call { name: "led.high" }
                    n if n.ends_with(".high") => {
                        let var = n.trim_end_matches(".high");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("snar_gpio_high({});", pin)));
                        }
                    }
                    n if n.ends_with(".low") => {
                        let var = n.trim_end_matches(".low");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("snar_gpio_low({});", pin)));
                        }
                    }
                    n if n.ends_with(".toggle") => {
                        let var = n.trim_end_matches(".toggle");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            // toggle: read current state and invert
                            return Ok(Some(format!("snar_gpio_high({}); /* TODO: toggle */", pin)));
                        }
                    }
                    n if n.ends_with(".read") && args.is_empty() => {
                        let var = n.trim_end_matches(".read");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("snar_gpio_read({})", pin)));
                        }
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
                            return Ok(Some(format!("snar_pwm_duty({}, {});", first_arg, duty)));
                        }
                        n if n.ends_with(".read") && args.is_empty() => {
                            // already handled above
                        }
                        // UART / Timer 直接调用
                        "UART.init" => {
                            return Ok(Some(format!("snar_uart_init({});", first_arg)));
                        }
                        "UART.write" => {
                            self.needs_string = true;
                            let s = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("snar_uart_write({});", s)));
                        }
                        "Timer.delay" => {
                            match self.target {
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
                            return Ok(Some(format!("snar_wifi_connect({}, {});", ssid, pass)));
                        }
                        // Phase F: AI
                        "Brain.init" => {
                            let path = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("snar_brain_init({});", path)));
                        }
                        "Brain.classify" if args.len() >= 2 => {
                            let input = self.generate_expr(&args[0])?;
                            let labels = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("snar_brain_classify((float*){}, {}, (const char**){}, {});",
                                input, "/* input_len */", labels, "/* n_labels */")));
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

    fn generate_expr(&mut self, expr: &Expr) -> Result<String, String> {
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
                let func_name = format!("_snar_arrow_{}", self.arrow_counter);
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
                    let _buf_name = format!("_snar_tmpl_{}", self.arrow_counter);
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
                    _ => Ok(format!("/* unknown string method: {} */ 0", method)),
                }
            }
            // Phase v1.5: await expr → 状态机 poll 调用（当前简化为编译内部表达式）
            Expr::Await { expr } => {
                // TODO: 完整状态机变换在 Task 4 实现
                self.generate_expr(expr)
            }
            // Phase v1.5: Promise.all([...]) → 并行等待结构体
            Expr::PromiseAll { promises } => {
                // TODO: 完整并行等待在 Task 4 实现
                if let Some(first) = promises.first() {
                    self.generate_expr(first)
                } else {
                    Ok("0".to_string())
                }
            }
            // Phase v1.5: Promise.race([...]) → 竞争等待结构体
            Expr::PromiseRace { promises } => {
                // TODO: 完整竞争等待在 Task 4 实现
                if let Some(first) = promises.first() {
                    self.generate_expr(first)
                } else {
                    Ok("0".to_string())
                }
            }
        }
    }

    /// 箭头函数赋值提升
    fn generate_arrow_assignment(&mut self, name: &str, params: &[String], body: &ArrowBody) -> Result<String, String> {
        self.arrow_counter += 1;
        let func_name = format!("_snar_fn_{}_{}", name, self.arrow_counter);

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
                Ok(format!("int32_t {} = {}", name, val))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn compile_to_c(source: &str, target: CTarget) -> String {
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let result = generate_c_source(&ast, &target).unwrap();
        result.source
    }

    #[test]
    fn test_simple_variable() {
        let c = compile_to_c("let x = 42;", CTarget::Esp32);
        assert!(c.contains("int32_t x = 42;"));
    }

    #[test]
    fn test_const_declaration() {
        let c = compile_to_c("const MAX = 100;", CTarget::Esp32);
        assert!(c.contains("const int32_t MAX = 100;"));
    }

    #[test]
    fn test_function_generation() {
        let c = compile_to_c("function add(a, b) { return a + b; }", CTarget::Esp32);
        assert!(c.contains("int32_t add(int32_t a, int32_t b)"));
        assert!(c.contains("return (a + b);"));
    }

    #[test]
    fn test_console_log() {
        let c = compile_to_c("console.log(42);", CTarget::Esp32);
        assert!(c.contains("printf(\"%d\\n\", 42)"));
        assert!(c.contains("#include <stdio.h>"));
    }

    #[test]
    fn test_while_loop() {
        let c = compile_to_c("let i = 0; while (i < 10) { i++; }", CTarget::Esp32);
        assert!(c.contains("while ((i < 10))"));
        assert!(c.contains("i++;"));
    }

    #[test]
    fn test_if_else() {
        let c = compile_to_c("let x = 10; if (x > 5) { console.log(1); } else { console.log(0); }", CTarget::Esp32);
        assert!(c.contains("if ((x > 5))"));
        assert!(c.contains("} else {"));
    }

    #[test]
    fn test_esp32_includes() {
        let c = compile_to_c(
            "import { GPIO, Timer } from \"snar:hw\"; let led = GPIO.output(2);",
            CTarget::Esp32
        );
        assert!(c.contains("#include \"driver/gpio.h\""));
        assert!(c.contains("#include \"freertos/FreeRTOS.h\""));
    }

    #[test]
    fn test_esp32_app_main() {
        let c = compile_to_c("let x = 1;", CTarget::Esp32);
        assert!(c.contains("void app_main(void)"));
    }

    #[test]
    fn test_gpio_output() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let led = GPIO.output(2);",
            CTarget::Esp32
        );
        assert!(c.contains("snar_gpio_output(2)"));
    }

    #[test]
    fn test_timer_delay_esp32() {
        let c = compile_to_c(
            "import { Timer } from \"snar:hw\"; Timer.delay(500);",
            CTarget::Esp32
        );
        assert!(c.contains("vTaskDelay(pdMS_TO_TICKS(500))"));
    }

    #[test]
    fn test_switch_case() {
        let c = compile_to_c(
            "let x = 1; switch (x) { case 1: console.log(1); break; default: console.log(0); break; }",
            CTarget::Esp32
        );
        assert!(c.contains("switch (x)"));
        assert!(c.contains("case 1:"));
        assert!(c.contains("default:"));
    }

    #[test]
    fn test_arrow_function_hoist() {
        let c = compile_to_c("let add = (a, b) => a + b;", CTarget::Esp32);
        assert!(c.contains("_snar_fn_add"));
        assert!(c.contains("return (a + b);"));
    }

    #[test]
    fn test_ternary() {
        let c = compile_to_c("let x = 10; let y = x > 5 ? 1 : 0;", CTarget::Esp32);
        assert!(c.contains("(x > 5) ? 1 : 0"));
    }

    #[test]
    fn test_for_loop() {
        let c = compile_to_c("for (let i = 0; i < 10; i++) { console.log(i); }", CTarget::Esp32);
        assert!(c.contains("for (int32_t i = 0;"));
        assert!(c.contains("i < 10"));
        assert!(c.contains("i++"));
    }

    #[test]
    fn test_string_literal() {
        let c = compile_to_c("let s = \"hello\";", CTarget::Esp32);
        assert!(c.contains("\"hello\""));
    }

    #[test]
    fn test_bool_null() {
        let c = compile_to_c("let a = true; let b = false; let c = null;", CTarget::Esp32);
        assert!(c.contains("int32_t a = 1;"));
        assert!(c.contains("int32_t b = 0;"));
        assert!(c.contains("int32_t c = 0;"));
    }

    // ========== Arduino 后端测试 ==========

    #[test]
    fn test_arduino_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let led = GPIO.output(13);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("#include <Arduino.h>"));
        // Arduino 不需要 stdint.h/stdio.h
        assert!(!c.contains("#include <stdint.h>"));
        assert!(!c.contains("#include <stdio.h>"));
    }

    #[test]
    fn test_arduino_setup_loop() {
        let c = compile_to_c(
            "import { GPIO, Timer } from \"snar:hw\"; let led = GPIO.output(13); while (true) { led.high(); Timer.delay(1000); led.low(); Timer.delay(1000); }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("void setup()"));
        assert!(c.contains("void loop()"));
        assert!(c.contains("delay(1000)"));
        // setup 中应有 GPIO 初始化
        assert!(c.contains("snar_gpio_output(13)"));
    }

    #[test]
    fn test_arduino_setup_only() {
        // 没有 while(true) 时，所有代码放入 setup()
        let c = compile_to_c("let x = 42;", CTarget::ArduinoAvr);
        assert!(c.contains("void setup()"));
        assert!(c.contains("void loop()"));
        assert!(c.contains("int32_t x = 42;"));
    }

    #[test]
    fn test_arduino_console_log() {
        let c = compile_to_c("console.log(42);", CTarget::ArduinoAvr);
        assert!(c.contains("Serial.println(42)"));
        assert!(c.contains("Serial.begin(115200)"));
    }

    #[test]
    fn test_arduino_console_log_string() {
        let c = compile_to_c("console.log(\"hello\");", CTarget::ArduinoAvr);
        assert!(c.contains("Serial.println(\"hello\")"));
    }

    #[test]
    fn test_arduino_console_log_empty() {
        let c = compile_to_c("console.log();", CTarget::ArduinoAvr);
        assert!(c.contains("Serial.println()"));
    }

    #[test]
    fn test_arduino_gpio_high_low() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let led = GPIO.output(13); while (true) { led.high(); led.low(); }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("snar_gpio_high(13)"));
        assert!(c.contains("snar_gpio_low(13)"));
    }

    #[test]
    fn test_arduino_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"snar:hw\"; Timer.delay(500);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("delay(500)"));
    }

    #[test]
    fn test_arduino_arm_target() {
        let c = compile_to_c(
            "import { GPIO, Timer } from \"snar:hw\"; let led = GPIO.output(2); while (true) { led.high(); Timer.delay(100); led.low(); Timer.delay(100); }",
            CTarget::ArduinoArm
        );
        assert!(c.contains("#include <Arduino.h>"));
        assert!(c.contains("void setup()"));
        assert!(c.contains("void loop()"));
        assert!(c.contains("delay(100)"));
    }

    #[test]
    fn test_arduino_gpio_helpers_comment() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let x = GPIO.output(5);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("SNARjs GPIO helpers (Arduino AVR)"));
    }

    #[test]
    fn test_arduino_arm_helpers_comment() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let x = GPIO.output(5);",
            CTarget::ArduinoArm
        );
        assert!(c.contains("SNARjs GPIO helpers (Arduino ARM)"));
    }

    // ========== STM32 后端测试 ==========

    #[test]
    fn test_stm32_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let led = GPIO.output(5);",
            CTarget::Stm32
        );
        assert!(c.contains("#include \"stm32f1xx_hal.h\""));
    }

    #[test]
    fn test_stm32_gpio_helpers() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let led = GPIO.output(5); while (true) { led.high(); led.low(); }",
            CTarget::Stm32
        );
        assert!(c.contains("SNARjs GPIO helpers (STM32 HAL)"));
        assert!(c.contains("HAL_GPIO_WritePin"));
        assert!(c.contains("snar_gpio_high(5)"));
        assert!(c.contains("snar_gpio_low(5)"));
    }

    #[test]
    fn test_stm32_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"snar:hw\"; Timer.delay(500);",
            CTarget::Stm32
        );
        assert!(c.contains("HAL_Delay(500)"));
    }

    #[test]
    fn test_stm32_main_function() {
        let c = compile_to_c("let x = 1;", CTarget::Stm32);
        assert!(c.contains("int main(void)"));
    }

    #[test]
    fn test_stm32_uart_helpers() {
        let c = compile_to_c(
            "import { UART } from \"snar:hw\"; UART.init(115200);",
            CTarget::Stm32
        );
        assert!(c.contains("#include <string.h>"));
        assert!(c.contains("snar_uart_init(115200)"));
    }

    // ========== 树莓派后端测试 ==========

    #[test]
    fn test_rpi_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let led = GPIO.output(17);",
            CTarget::RaspberryPi
        );
        assert!(c.contains("#include <fcntl.h>"));
        assert!(c.contains("#include <unistd.h>"));
    }

    #[test]
    fn test_rpi_gpio_sysfs() {
        let c = compile_to_c(
            "import { GPIO } from \"snar:hw\"; let led = GPIO.output(17); while (true) { led.high(); led.low(); }",
            CTarget::RaspberryPi
        );
        assert!(c.contains("SNARjs GPIO helpers (Linux sysfs)"));
        assert!(c.contains("/sys/class/gpio/"));
        assert!(c.contains("snar_gpio_high(17)"));
        assert!(c.contains("snar_gpio_low(17)"));
    }

    #[test]
    fn test_rpi_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"snar:hw\"; Timer.delay(1000);",
            CTarget::RaspberryPi
        );
        assert!(c.contains("usleep(1000 * 1000)"));
    }

    #[test]
    fn test_rpi_main_function() {
        let c = compile_to_c("let x = 1;", CTarget::RaspberryPi);
        assert!(c.contains("int main(void)"));
    }

    #[test]
    fn test_rpi_console_log() {
        let c = compile_to_c("console.log(42);", CTarget::RaspberryPi);
        assert!(c.contains("printf"));
    }

    // Phase v1.5: async 函数 C 代码生成测试
    #[test]
    fn test_cgen_async_function() {
        let c = compile_to_c("async function readSensor() { let val = await ADC.read(0); return val; }", CTarget::Esp32);
        assert!(c.contains("readSensor_ctx"), "应生成上下文结构体");
        assert!(c.contains("readSensor_poll"), "应生成 poll 函数");
        assert!(c.contains("SNAR_PENDING"), "应返回 PENDING");
        assert!(c.contains("SNAR_READY"), "应返回 READY");
        assert!(c.contains("switch (ctx->state)"), "应包含状态分发");
    }

    #[test]
    fn test_cgen_async_spawn() {
        let c = compile_to_c("async function foo() { let x = await bar(); return x; }", CTarget::Esp32);
        assert!(c.contains("foo_spawn"), "应生成 spawn 函数");
        assert!(c.contains("snar_spawn"), "应调用 snar_spawn");
    }

    #[test]
    fn test_cgen_async_multi_await() {
        let c = compile_to_c(
            "async function multi() { let a = await r1(); let b = await r2(); return a; }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("multi_ctx"));
        assert!(c.contains("multi_poll"));
        // 应该有多个 case
        assert!(c.contains("case 0:"));
        assert!(c.contains("case 1:"));
    }
}
