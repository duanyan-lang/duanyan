//! C 代码生成器 (AST → C transpiler)
//!
//! Phase C: ESP32 后端 | Phase D: Arduino 后端
//! 将 HardyScript AST 转译为 C 源码，供 ESP-IDF / Arduino Core / STM32 HAL 编译

use crate::parser::{
    Program, Statement, Expr, BinOp, MathOp,
    Function, ArrowBody, TemplateExprPart,
};
use crate::hardware_api::HardwareModule;
use crate::scheduler::Scheduler;
use std::collections::HashSet;

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
    /// Phase 3.3: 中断函数跟踪 (func_name, timer, period_us)
    interrupt_fns: Vec<(String, String, u32)>,
    /// Phase 4: 芯片信息 API (hardy:esp32 Chip)
    needs_chip_info: bool,
}

/// ESP32-S3 裸机 HAL (源自 DUANYAN 实测跑通的 led_on.c / gradient.c)
/// 数据脚 GPIO48 (RGB_BUILTIN), 供电使能脚 GPIO47
const ESP32S3_BARE_HAL: &str = r#"
/* ===== HardyScript ESP32-S3 bare-metal HAL (WS2812 onboard RGB) ===== */
typedef unsigned int u32;
#define HARDY_REG(addr) (*(volatile u32 *)(addr))

#define HARDY_UART0_FIFO   HARDY_REG(0x60000000)
#define HARDY_UART0_STATUS HARDY_REG(0x6000001C)
#define HARDY_RTC_WDTCONFIG0  HARDY_REG(0x60008098)
#define HARDY_RTC_WDTFEED     HARDY_REG(0x600080AC)
#define HARDY_RTC_WDTWPROTECT HARDY_REG(0x600080B0)
#define HARDY_RTC_SWD_CONF    HARDY_REG(0x600080B4)
#define HARDY_RTC_SWD_WPROT   HARDY_REG(0x600080B8)
#define HARDY_TIMG0_T0CONFIG  HARDY_REG(0x6001F000)
#define HARDY_TIMG0_T0LO      HARDY_REG(0x6001F004)
#define HARDY_TIMG0_T0UPDATE  HARDY_REG(0x6001F00C)
#define HARDY_TIMG0_WDTCONFIG0  HARDY_REG(0x6001F048)
#define HARDY_TIMG0_WDTWPROTECT HARDY_REG(0x6001F064)
#define HARDY_TIMG0_INT_CLR     HARDY_REG(0x6001F07C)
#define HARDY_GPIO_BASE  0x60004000UL
#define HARDY_IOMUX_BASE 0x60009000UL

/* RMT 外设 (硬件波形发生器, 时序由 80MHz 时钟树保证, 与 CPU 频率无关) */
#define HARDY_PERIP_CLK_EN0  HARDY_REG(0x600C0018)
#define HARDY_RMT_BASE       0x60016000UL
#define HARDY_RMT_RAM_CH0    0x60016800UL
#define HARDY_RMT_CH0CONF0   HARDY_REG(HARDY_RMT_BASE + 0x20)
/* S3 真实偏移 (核对 esp-idf rmt_struct.h): 早期用的 0x74/0x78/0x7C
 * 落在载波占空比寄存器上, 导致中断从未被正确清除/轮询 → 帧撕裂闪烁 */
#define HARDY_RMT_INT_RAW    HARDY_REG(HARDY_RMT_BASE + 0x60)
#define HARDY_RMT_INT_ST     HARDY_REG(HARDY_RMT_BASE + 0x64)
#define HARDY_RMT_INT_ENA    HARDY_REG(HARDY_RMT_BASE + 0x68)
#define HARDY_RMT_INT_CLR    HARDY_REG(HARDY_RMT_BASE + 0x6C)
#define HARDY_RMT_SYS_CONF   HARDY_REG(HARDY_RMT_BASE + 0xC0)
/* WS2812 时序 @ RMT sclk 实测 12.71MHz (tick≈79ns)。
 * 测量方法: 单帧拉长 144000 ticks + UART 5字符(434us)锚定
 * 标定 CPU 轮询速率 + GPIO48 电平轮询测总时长, 三次重复一致。
 * 此前误设 10MHz → T0H 仅 0.236us 压在 WS2812 判决门限上,
 * "0"码被随机误读为 1 → 约半数帧被污染偏白 → 肉眼闪烁。
 * 现 T0H=0.39us/T0L=0.87us/T1H=0.87us/T1L=0.39us, 门限余量充足 */
#define HARDY_WS_T0H 5u
#define HARDY_WS_T0L 11u
#define HARDY_WS_T1H 11u
#define HARDY_WS_T1L 5u

static u32 _hardy_ticks_per_us = 10;

static void hardy_uart_putc(char c) {
    while (((HARDY_UART0_STATUS >> 16) & 0x3FF) >= 127) { }
    HARDY_UART0_FIFO = (u32)c;
}
static void hardy_uart_puts(const char *s) { while (*s) hardy_uart_putc(*s++); }
static void hardy_uart_num(u32 v) {
    char tmp[10]; int t = 0;
    do { tmp[t++] = (char)('0' + (v % 10)); v /= 10; } while (v > 0);
    while (t > 0) hardy_uart_putc(tmp[--t]);
}
static u32 hardy_timer_now(void) { HARDY_TIMG0_T0UPDATE = 1; return HARDY_TIMG0_T0LO; }
static void hardy_wait_ticks(u32 start, u32 ticks) {
    u32 cap = (ticks + 10) * 100 + 100000;
    while ((hardy_timer_now() - start) < ticks && cap--) { }
}
static void hardy_delay_us(u32 us) { hardy_wait_ticks(hardy_timer_now(), us * _hardy_ticks_per_us); }
static void hardy_delay_ms(u32 ms) { hardy_wait_ticks(hardy_timer_now(), ms * 1000 * _hardy_ticks_per_us); }
/* UART 锚定等待: 1 字符 = 86.8us 绝对基准 (115200 波特), 完全不依赖
 * timer/校准/时钟树——这块板 timer 与校准均不可信, UART 是
 * 全系统唯一被实测验证精确的时间基准 */
static void hardy_uart_drain(void) {
    while (((HARDY_UART0_STATUS >> 16) & 0x3FF) != 0) { }
}
/* 开机自校准: 用 UART 115200 绝对基准实测 ticks/us。
 * 长采样 1000 字符 (~87ms): 不同板卡 timer 频率差异巨大 (实测过
 * 10MHz 与 155MHz 两种), 短采样 10 字符的量化误差会把 tpu 测错
 * 数倍, 导致所有延时/帧等待失真 → WS2812 帧尾撕裂闪烁 */
static void hardy_calibrate(void) {
    u32 t0 = hardy_timer_now();
    for (int i = 0; i < 1000; i++) hardy_uart_putc('~');
    while (((HARDY_UART0_STATUS >> 16) & 0x3FF) != 0) { }
    u32 ticks = hardy_timer_now() - t0;
    _hardy_ticks_per_us = ticks / 86800;
    if (_hardy_ticks_per_us == 0) _hardy_ticks_per_us = 10;
}
static void hardy_pin_setup(u32 pin) {
    HARDY_REG(HARDY_IOMUX_BASE + pin * 4) = (1u << 12) | (3u << 10);
    HARDY_REG(HARDY_GPIO_BASE + 0x554 + pin * 4) = pin;
    if (pin >= 32) HARDY_REG(HARDY_GPIO_BASE + 0x30) = 1u << (pin - 32);
    else HARDY_REG(HARDY_GPIO_BASE + 0x24) = 1u << pin;
}
static void hardy_pin_hi(u32 pin) {
    if (pin >= 32) HARDY_REG(HARDY_GPIO_BASE + 0x14) = 1u << (pin - 32);
    else HARDY_REG(HARDY_GPIO_BASE + 0x08) = 1u << pin;
}
static void hardy_pin_lo(u32 pin) {
    if (pin >= 32) HARDY_REG(HARDY_GPIO_BASE + 0x18) = 1u << (pin - 32);
    else HARDY_REG(HARDY_GPIO_BASE + 0x0C) = 1u << pin;
}
/* RMT 初始化: 通道0 单发模式, 输出路由到 GPIO48 (信号81) */
static void hardy_rmt_init(void) {
    HARDY_PERIP_CLK_EN0 |= (1u << 9);             /* RMT 时钟使能 */
    HARDY_RMT_SYS_CONF = (1u << 0) | (1u << 1) | (1u << 3)
                       | (1u << 4) | (1u << 24) | (1u << 26) | (1u << 31);
    HARDY_RMT_CH0CONF0 = (1u << 8) | (1u << 16) | (1u << 6); /* div=1, 1块RAM, idle低 */
    HARDY_RMT_CH0CONF0 |= (1u << 24);             /* CONF_UPDATE */
    /* 数据脚 GPIO48 交给 RMT: IO_MUX 设 GPIO 功能, FUNC_OUT_SEL = 81 */
    HARDY_REG(HARDY_IOMUX_BASE + 48 * 4) = (1u << 12) | (3u << 10);
    HARDY_REG(HARDY_GPIO_BASE + 0x554 + 48 * 4) = 81;
}
/* WS2812: 经 RMT 硬件发送 24bit GRB (MSB first), 帧含 50us 锁存间隙。
 * 完成等待用 UART 锚定 (2 锚=174us > 帧最长 109us): 物理上保证发完
 * 才重写 RAM, 不依赖任何校准; TX_END 在部分板上不置位, 仅作快路径 */
static void hardy_ws2812_send(u32 grb) {
    volatile u32 *ram = (volatile u32 *)HARDY_RMT_RAM_CH0;
    const u32 BASE_CONF = (1u << 8) | (1u << 16) | (1u << 6);
    for (int i = 0; i < 24; i++) {
        u32 hi = HARDY_WS_T0H, lo = HARDY_WS_T0L;
        if ((grb >> (23 - i)) & 1) { hi = HARDY_WS_T1H; lo = HARDY_WS_T1L; }
        ram[i] = hi | (1u << 15) | (lo << 16);
    }
    ram[24] = 128;   /* ~10us 低电平 = WS2812 锁存间隙 (@12.71MHz) */
    ram[25] = 0;     /* 结束标记 */
    HARDY_RMT_INT_CLR = 1;
    HARDY_RMT_CH0CONF0 = BASE_CONF | (1u << 1) | (1u << 2);  /* 复位读指针 */
    HARDY_RMT_CH0CONF0 = BASE_CONF;
    HARDY_RMT_CH0CONF0 = BASE_CONF | (1u << 24);  /* CONF_UPDATE */
    HARDY_RMT_CH0CONF0 = BASE_CONF | (1u << 0);   /* TX_START */
    hardy_delay_us(400);  /* 帧长 ≈291us (@12.71MHz), 400us 余量充足;
                           * 校准经 UART 锚定实测自洽, 不再向串口发字符 */
}
static void hardy_rgb_set(int r, int g, int b) {
    if (r < 0) r = 0; if (r > 255) r = 255;
    if (g < 0) g = 0; if (g > 255) g = 255;
    if (b < 0) b = 0; if (b > 255) b = 255;
    hardy_ws2812_send(((u32)g << 16) | ((u32)r << 8) | (u32)b);
}
/* HSV 色轮: 765 步细粒度色环 (每段 255 步), 亮度 1/4。
 * 相比 256 步旧版, 相邻帧亮度增量从 ±3 降到 ±1,
 * 消除渐变时的阶梯跳变 (肉眼误判为闪烁的主要嫌疑) */
static void hardy_rgb_hue(int pos) {
    int r, g, b;
    pos = pos % 765;
    if (pos < 0) pos += 765;
    if (pos < 255) { r = 255 - pos; g = pos; b = 0; }
    else {
        if (pos < 510) { pos -= 255; r = 0; g = 255 - pos; b = pos; }
        else { pos -= 510; r = pos; g = 0; b = 255 - pos; }
    }
    hardy_ws2812_send(((u32)(g >> 2) << 16) | ((u32)(r >> 2) << 8) | (u32)(b >> 2));
}
static void hardy_boot_init(void) {
    /* 关三只看门狗 */
    HARDY_RTC_WDTWPROTECT = 0x50D83AA1;
    HARDY_RTC_WDTFEED = 1;
    HARDY_RTC_WDTCONFIG0 = 0;
    HARDY_RTC_WDTWPROTECT = 0;
    HARDY_RTC_SWD_WPROT = 0x8F1D312A;
    HARDY_RTC_SWD_CONF = (1u << 30);
    HARDY_RTC_SWD_WPROT = 0;
    HARDY_TIMG0_WDTWPROTECT = 0x50D83AA1;
    HARDY_TIMG0_WDTCONFIG0 = 0;
    HARDY_TIMG0_INT_CLR = 0xFFFFFFFF;
    HARDY_TIMG0_WDTWPROTECT = 0;
    /* TIMG0 Timer0: APB 源, 分频=1, 使能 */
    HARDY_TIMG0_T0CONFIG = (1u << 13) | (1u << 30) | (1u << 31);
    hardy_calibrate();
    /* WS2812 供电使能 GPIO47 (不使能则灯不亮) */
    hardy_pin_setup(47);
    hardy_pin_hi(47);
    hardy_delay_ms(10);
    /* 数据脚 GPIO48 由 RMT 外设驱动 (软件模拟在低 CPU 频率下波形畸变) */
    hardy_rmt_init();
    hardy_uart_puts("[DUANYAN-OK] hardyscript bare boot (RMT) tpu=");
    hardy_uart_num(_hardy_ticks_per_us);
    hardy_uart_puts("\r\n");
}
"#;

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
            promise_counter: 0,
            promise_helpers: Vec::new(),
            scheduler: Scheduler::new(),
            struct_defs: Vec::new(),
            object_vars: Vec::new(),
            array_vars: Vec::new(),
            needs_arr_helper: false,
            interrupt_fns: Vec::new(),
            needs_chip_info: false,
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
            self.output.push_str("/* HardyScript object structs */\n");
            for def in &self.struct_defs.clone() {
                self.output.push_str(def);
            }
            self.output.push('\n');
        }

        // Phase 2.4: 输出数组边界检查 helper
        if self.needs_arr_helper {
            self.output.push_str("/* HardyScript array bounds check */\n");
            self.output.push_str("static inline int32_t _hardy_arr_get(int32_t* arr, int32_t len, int32_t idx) {\n");
            self.output.push_str("    if (idx < 0 || idx >= len) { while(1); /* ARRAY_OOB */ }\n");
            self.output.push_str("    return arr[idx];\n");
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
            self.output.push_str("    hardy_boot_init();\n");
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
                    self.output.push_str("    for (;;) { _hardy_scheduler_poll(); }\n");
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
        self.output.push_str("/* Auto-generated by HardyScript v");
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
                    self.output.push_str("#include \"hardy_ai.h\"\n");
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

    /// 生成硬件 API 辅助函数
    fn generate_hw_helpers(&mut self) {
        match self.target {
            CTarget::Esp32S3Bare => {
                // 裸机 HAL 已在头部整体注入, 无需额外 helper
            }
            CTarget::Esp32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* HardyScript GPIO helpers */\n");
                    self.output.push_str("static inline void hardy_gpio_output(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_INPUT);\n");
                    self.output.push_str("    gpio_set_pull_mode(pin, GPIO_PULLUP_ONLY);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_high(int pin) { gpio_set_level(pin, 1); }\n");
                    self.output.push_str("static inline void hardy_gpio_low(int pin)  { gpio_set_level(pin, 0); }\n");
                    self.output.push_str("static inline int  hardy_gpio_read(int pin) { return gpio_get_level(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* HardyScript UART helpers */\n");
                    self.output.push_str("static inline void hardy_uart_init(int baud) {\n");
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
                    self.output.push_str("static inline void hardy_uart_write(const char* str) {\n");
                    self.output.push_str("    uart_write_bytes(UART_NUM_0, str, strlen(str));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* HardyScript PWM (LEDC) helpers */\n");
                    self.output.push_str("static int _hardy_pwm_channel = 0;\n");
                    self.output.push_str("static inline int hardy_pwm_output(int pin, int freq) {\n");
                    self.output.push_str("    int ch = _hardy_pwm_channel++;\n");
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
                    self.output.push_str("static inline void hardy_pwm_duty(int channel, int duty) {\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, channel, duty);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* HardyScript ADC helpers */\n");
                    self.output.push_str("static inline void hardy_adc_init(int channel) {\n");
                    self.output.push_str("    adc1_config_width(ADC_WIDTH_BIT_12);\n");
                    self.output.push_str("    adc1_config_channel_atten(channel, ADC_ATTEN_DB_11);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int hardy_adc_read(int channel) {\n");
                    self.output.push_str("    return adc1_get_raw(channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_wifi {
                    self.output.push_str("/* HardyScript WiFi helper */\n");
                    self.output.push_str("static void hardy_wifi_connect(const char* ssid, const char* pass) {\n");
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
                if self.needs_chip_info {
                    self.output.push_str("/* HardyScript Chip Info helpers */\n");
                    self.output.push_str("static const char* hardy_chip_model_name(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    switch (info.model) {\n");
                    self.output.push_str("        case CHIP_ESP32:   return \"ESP32\";\n");
                    self.output.push_str("        case CHIP_ESP32S2: return \"ESP32-S2\";\n");
                    self.output.push_str("        case CHIP_ESP32S3: return \"ESP32-S3\";\n");
                    self.output.push_str("        case CHIP_ESP32C3: return \"ESP32-C3\";\n");
                    self.output.push_str("        case CHIP_ESP32C6: return \"ESP32-C6\";\n");
                    self.output.push_str("        case CHIP_ESP32H2: return \"ESP32-H2\";\n");
                    self.output.push_str("        default:           return \"Unknown\";\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int hardy_chip_cores(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    return info.cores;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int hardy_chip_revision(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    return info.revision;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int hardy_chip_flash_size(void) {\n");
                    self.output.push_str("    uint32_t size = 0;\n");
                    self.output.push_str("    esp_flash_get_size(NULL, &size);\n");
                    self.output.push_str("    return (int)(size / (1024 * 1024));\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int hardy_chip_free_heap(void) {\n");
                    self.output.push_str("    return (int)esp_get_free_heap_size();\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int hardy_chip_cpu_freq(void) {\n");
                    self.output.push_str("    return CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static uint32_t hardy_chip_id(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    return ((uint32_t)mac[2] << 24) | ((uint32_t)mac[3] << 16) | ((uint32_t)mac[4] << 8) | mac[5];\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static const char* hardy_chip_sdk_version(void) {\n");
                    self.output.push_str("    return esp_get_idf_version();\n");
                    self.output.push_str("}\n");
                    // 设备唯一 ID: MAC → SHA-256 → "SNAR-XXXX-XXXX-XXXX"
                    self.output.push_str("static char _hardy_device_id[20];\n");
                    self.output.push_str("static const char* hardy_chip_device_id(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    uint8_t hash[32];\n");
                    self.output.push_str("    mbedtls_sha256(mac, 6, hash, 0);\n");
                    self.output.push_str("    snprintf(_hardy_device_id, sizeof(_hardy_device_id),\n");
                    self.output.push_str("        \"SNAR-%02X%02X-%02X%02X-%02X%02X\",\n");
                    self.output.push_str("        hash[0], hash[1], hash[2], hash[3], hash[4], hash[5]);\n");
                    self.output.push_str("    return _hardy_device_id;\n");
                    self.output.push_str("}\n");
                    // MAC 地址字符串
                    self.output.push_str("static char _hardy_mac_str[18];\n");
                    self.output.push_str("static const char* hardy_chip_mac(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    snprintf(_hardy_mac_str, sizeof(_hardy_mac_str),\n");
                    self.output.push_str("        \"%02X:%02X:%02X:%02X:%02X:%02X\",\n");
                    self.output.push_str("        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);\n");
                    self.output.push_str("    return _hardy_mac_str;\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_ai {
                    self.output.push_str("/* HardyScript 锻言Mini AI helpers */\n");
                    self.output.push_str("static hardy_brain_t _hardy_brain;\n");
                    self.output.push_str("static inline int hardy_brain_init(const char* model_path) {\n");
                    self.output.push_str("    return hardy_ai_load_model(&_hardy_brain, model_path);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int hardy_brain_classify(float* input, int input_len,\n");
                    self.output.push_str("                                       const char** labels, int n_labels) {\n");
                    self.output.push_str("    return hardy_ai_classify(&_hardy_brain, input, input_len, labels, n_labels);\n");
                    self.output.push_str("}\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (ESP32 LEDC)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* HardyScript Servo helpers (ESP32 LEDC 50Hz PWM) */\n");
                    self.output.push_str("#define HARDY_SERVO_MAX 16\n");
                    self.output.push_str("static int _hardy_servo_channel = 0;\n");
                    self.output.push_str("static int _hardy_servo_min[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_max[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_pin[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int ch = _hardy_servo_channel++;\n");
                    self.output.push_str("    _hardy_servo_min[ch] = min_us;\n");
                    self.output.push_str("    _hardy_servo_max[ch] = max_us;\n");
                    self.output.push_str("    _hardy_servo_pin[ch] = pin;\n");
                    self.output.push_str("    _hardy_servo_count = ch + 1;\n");
                    self.output.push_str("    ledc_timer_config_t tcfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .duty_resolution = LEDC_TIMER_16_BIT,\n");
                    self.output.push_str("        .timer_num = LEDC_TIMER_1,\n");
                    self.output.push_str("        .freq_hz = 50,\n");
                    self.output.push_str("        .clk_cfg = LEDC_AUTO_CLK,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_timer_config(&tcfg);\n");
                    self.output.push_str("    ledc_channel_config_t ccfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .channel = ch,\n");
                    self.output.push_str("        .timer_sel = LEDC_TIMER_1,\n");
                    self.output.push_str("        .intr_type = LEDC_INTR_DISABLE,\n");
                    self.output.push_str("        .gpio_num = pin,\n");
                    self.output.push_str("        .duty = 0,\n");
                    self.output.push_str("        .hpoint = 0,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_channel_config(&ccfg);\n");
                    self.output.push_str("    return ch;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    int ch = -1;\n");
                    self.output.push_str("    for (int i = 0; i < _hardy_servo_count; i++) {\n");
                    self.output.push_str("        if (_hardy_servo_pin[i] == pin) { ch = i; break; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (ch < 0) return;\n");
                    self.output.push_str("    if (angle < 0) angle = 0;\n");
                    self.output.push_str("    if (angle > 180) angle = 180;\n");
                    self.output.push_str("    int pulse_us = _hardy_servo_min[ch] + (angle * (_hardy_servo_max[ch] - _hardy_servo_min[ch])) / 180;\n");
                    self.output.push_str("    /* 16-bit duty: 65535 = 20000us period, pulse_us maps to duty */\n");
                    self.output.push_str("    uint32_t duty = (uint32_t)((uint64_t)pulse_us * 65535 / 20000);\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, ch, duty);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, ch);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* HardyScript Motor helpers (ESP32 LEDC + GPIO) */\n");
                    self.output.push_str("#define HARDY_MOTOR_MAX 4\n");
                    self.output.push_str("static int _hardy_motor_in1[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_in2[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_ch[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _hardy_motor_count++;\n");
                    self.output.push_str("    _hardy_motor_in1[id] = in1;\n");
                    self.output.push_str("    _hardy_motor_in2[id] = in2;\n");
                    self.output.push_str("    gpio_set_direction(in1, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("    gpio_set_direction(in2, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("    int ch = _hardy_servo_channel++;\n");
                    self.output.push_str("    _hardy_motor_ch[id] = ch;\n");
                    self.output.push_str("    ledc_timer_config_t tcfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .duty_resolution = LEDC_TIMER_8_BIT,\n");
                    self.output.push_str("        .timer_num = LEDC_TIMER_2,\n");
                    self.output.push_str("        .freq_hz = 1000,\n");
                    self.output.push_str("        .clk_cfg = LEDC_AUTO_CLK,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_timer_config(&tcfg);\n");
                    self.output.push_str("    ledc_channel_config_t ccfg = {\n");
                    self.output.push_str("        .speed_mode = LEDC_LOW_SPEED_MODE,\n");
                    self.output.push_str("        .channel = ch,\n");
                    self.output.push_str("        .timer_sel = LEDC_TIMER_2,\n");
                    self.output.push_str("        .intr_type = LEDC_INTR_DISABLE,\n");
                    self.output.push_str("        .gpio_num = ena,\n");
                    self.output.push_str("        .duty = 0,\n");
                    self.output.push_str("        .hpoint = 0,\n");
                    self.output.push_str("    };\n");
                    self.output.push_str("    ledc_channel_config(&ccfg);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        gpio_set_level(_hardy_motor_in1[id], 1);\n");
                    self.output.push_str("        gpio_set_level(_hardy_motor_in2[id], 0);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        gpio_set_level(_hardy_motor_in1[id], 0);\n");
                    self.output.push_str("        gpio_set_level(_hardy_motor_in2[id], 1);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, _hardy_motor_ch[id], speed);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, _hardy_motor_ch[id]);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    gpio_set_level(_hardy_motor_in1[id], 0);\n");
                    self.output.push_str("    gpio_set_level(_hardy_motor_in2[id], 0);\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, _hardy_motor_ch[id], 0);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, _hardy_motor_ch[id]);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::ArduinoAvr => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* HardyScript GPIO helpers (Arduino AVR) */\n");
                    self.output.push_str("static inline void hardy_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void hardy_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void hardy_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void hardy_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  hardy_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* HardyScript UART helpers (Arduino) */\n");
                    self.output.push_str("static inline void hardy_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void hardy_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* HardyScript Timer helpers (Arduino) */\n");
                    self.output.push_str("static inline void hardy_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* HardyScript PWM helpers (Arduino AVR, 8-bit duty 0-255) */\n");
                    self.output.push_str("static inline void hardy_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void hardy_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* HardyScript ADC helpers (Arduino AVR, 10-bit) */\n");
                    self.output.push_str("static inline int hardy_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (Arduino AVR)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* HardyScript Servo helpers (Arduino Servo.h) */\n");
                    self.output.push_str("#define HARDY_SERVO_MAX 12\n");
                    self.output.push_str("static Servo _hardy_servos[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_pins[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _hardy_servo_count++;\n");
                    self.output.push_str("    _hardy_servo_pins[id] = pin;\n");
                    self.output.push_str("    _hardy_servos[id].attach(pin, min_us, max_us);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    for (int i = 0; i < _hardy_servo_count; i++) {\n");
                    self.output.push_str("        if (_hardy_servo_pins[i] == pin) { _hardy_servos[i].write(angle); return; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* HardyScript Motor helpers (Arduino AVR) */\n");
                    self.output.push_str("#define HARDY_MOTOR_MAX 4\n");
                    self.output.push_str("static int _hardy_motor_in1[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_in2[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_ena[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _hardy_motor_count++;\n");
                    self.output.push_str("    _hardy_motor_in1[id] = in1;\n");
                    self.output.push_str("    _hardy_motor_in2[id] = in2;\n");
                    self.output.push_str("    _hardy_motor_ena[id] = ena;\n");
                    self.output.push_str("    pinMode(in1, OUTPUT);\n");
                    self.output.push_str("    pinMode(in2, OUTPUT);\n");
                    self.output.push_str("    pinMode(ena, OUTPUT);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in1[id], HIGH);\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in2[id], LOW);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in1[id], LOW);\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in2[id], HIGH);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    analogWrite(_hardy_motor_ena[id], speed);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    digitalWrite(_hardy_motor_in1[id], LOW);\n");
                    self.output.push_str("    digitalWrite(_hardy_motor_in2[id], LOW);\n");
                    self.output.push_str("    analogWrite(_hardy_motor_ena[id], 0);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::ArduinoArm => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* HardyScript GPIO helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void hardy_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void hardy_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void hardy_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void hardy_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  hardy_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* HardyScript UART helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void hardy_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void hardy_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* HardyScript Timer helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void hardy_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* HardyScript PWM helpers (Arduino ARM, 12-bit duty 0-4095) */\n");
                    self.output.push_str("static inline void hardy_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void hardy_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* HardyScript ADC helpers (Arduino ARM, 12-bit) */\n");
                    self.output.push_str("static inline int hardy_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (Arduino ARM)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* HardyScript Servo helpers (Arduino ARM Servo.h) */\n");
                    self.output.push_str("#define HARDY_SERVO_MAX 12\n");
                    self.output.push_str("static Servo _hardy_servos[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_pins[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _hardy_servo_count++;\n");
                    self.output.push_str("    _hardy_servo_pins[id] = pin;\n");
                    self.output.push_str("    _hardy_servos[id].attach(pin, min_us, max_us);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    for (int i = 0; i < _hardy_servo_count; i++) {\n");
                    self.output.push_str("        if (_hardy_servo_pins[i] == pin) { _hardy_servos[i].write(angle); return; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* HardyScript Motor helpers (Arduino ARM) */\n");
                    self.output.push_str("#define HARDY_MOTOR_MAX 4\n");
                    self.output.push_str("static int _hardy_motor_in1[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_in2[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_ena[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _hardy_motor_count++;\n");
                    self.output.push_str("    _hardy_motor_in1[id] = in1;\n");
                    self.output.push_str("    _hardy_motor_in2[id] = in2;\n");
                    self.output.push_str("    _hardy_motor_ena[id] = ena;\n");
                    self.output.push_str("    pinMode(in1, OUTPUT);\n");
                    self.output.push_str("    pinMode(in2, OUTPUT);\n");
                    self.output.push_str("    pinMode(ena, OUTPUT);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in1[id], HIGH);\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in2[id], LOW);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in1[id], LOW);\n");
                    self.output.push_str("        digitalWrite(_hardy_motor_in2[id], HIGH);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    analogWrite(_hardy_motor_ena[id], speed);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    digitalWrite(_hardy_motor_in1[id], LOW);\n");
                    self.output.push_str("    digitalWrite(_hardy_motor_in2[id], LOW);\n");
                    self.output.push_str("    analogWrite(_hardy_motor_ena[id], 0);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::Stm32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* HardyScript GPIO helpers (STM32 HAL) */\n");
                    self.output.push_str("static GPIO_TypeDef* hardy_gpio_port(int pin) {\n");
                    self.output.push_str("    if (pin >= 16) return GPIOB;\n");
                    self.output.push_str("    return GPIOA;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static uint16_t hardy_gpio_pin_mask(int pin) {\n");
                    self.output.push_str("    return (uint16_t)(1 << (pin & 0xF));\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_output(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = hardy_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_OUTPUT_PP;\n");
                    self.output.push_str("    gpio.Pull = GPIO_NOPULL;\n");
                    self.output.push_str("    gpio.Speed = GPIO_SPEED_FREQ_LOW;\n");
                    self.output.push_str("    HAL_GPIO_Init(hardy_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = hardy_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_INPUT;\n");
                    self.output.push_str("    gpio.Pull = GPIO_PULLUP;\n");
                    self.output.push_str("    HAL_GPIO_Init(hardy_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_high(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(hardy_gpio_port(pin), hardy_gpio_pin_mask(pin), GPIO_PIN_SET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_low(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(hardy_gpio_port(pin), hardy_gpio_pin_mask(pin), GPIO_PIN_RESET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int hardy_gpio_read(int pin) {\n");
                    self.output.push_str("    return HAL_GPIO_ReadPin(hardy_gpio_port(pin), hardy_gpio_pin_mask(pin));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* HardyScript Timer helpers (STM32 HAL) */\n");
                    self.output.push_str("static inline void hardy_timer_delay(int ms) { HAL_Delay(ms); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* HardyScript UART helpers (STM32 HAL) */\n");
                    self.output.push_str("static UART_HandleTypeDef _hardy_huart;\n");
                    self.output.push_str("static inline void hardy_uart_init(int baud) {\n");
                    self.output.push_str("    _hardy_huart.Instance = USART1;\n");
                    self.output.push_str("    _hardy_huart.Init.BaudRate = baud;\n");
                    self.output.push_str("    _hardy_huart.Init.WordLength = UART_WORDLENGTH_8B;\n");
                    self.output.push_str("    _hardy_huart.Init.StopBits = UART_STOPBITS_1;\n");
                    self.output.push_str("    _hardy_huart.Init.Parity = UART_PARITY_NONE;\n");
                    self.output.push_str("    _hardy_huart.Init.Mode = UART_MODE_TX_RX;\n");
                    self.output.push_str("    HAL_UART_Init(&_hardy_huart);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_uart_write(const char* str) {\n");
                    self.output.push_str("    HAL_UART_Transmit(&_hardy_huart, (uint8_t*)str, strlen(str), HAL_MAX_DELAY);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* HardyScript PWM helpers (STM32 TIM, 16-bit duty) */\n");
                    self.output.push_str("static inline void hardy_pwm_duty(int pin, int duty) {\n");
                    self.output.push_str("    /* User must configure TIM per pin; this is a placeholder */\n");
                    self.output.push_str("    (void)pin; (void)duty;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* HardyScript ADC helpers (STM32 HAL) */\n");
                    self.output.push_str("static ADC_HandleTypeDef _hardy_hadc;\n");
                    self.output.push_str("static inline int hardy_adc_read(int channel) {\n");
                    self.output.push_str("    ADC_ChannelConfTypeDef sConfig = {0};\n");
                    self.output.push_str("    sConfig.Channel = channel;\n");
                    self.output.push_str("    HAL_ADC_ConfigChannel(&_hardy_hadc, &sConfig);\n");
                    self.output.push_str("    HAL_ADC_Start(&_hardy_hadc);\n");
                    self.output.push_str("    HAL_ADC_PollForConversion(&_hardy_hadc, 100);\n");
                    self.output.push_str("    int val = HAL_ADC_GetValue(&_hardy_hadc);\n");
                    self.output.push_str("    HAL_ADC_Stop(&_hardy_hadc);\n");
                    self.output.push_str("    return val;\n");
                    self.output.push_str("}\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (STM32 TIM HAL)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* HardyScript Servo helpers (STM32 TIM2 PWM 50Hz) */\n");
                    self.output.push_str("static TIM_HandleTypeDef _hardy_servo_htim;\n");
                    self.output.push_str("#define HARDY_SERVO_MAX 4\n");
                    self.output.push_str("static int _hardy_servo_pin[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_min[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_max[HARDY_SERVO_MAX];\n");
                    self.output.push_str("static int _hardy_servo_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _hardy_servo_count++;\n");
                    self.output.push_str("    _hardy_servo_pin[id] = pin;\n");
                    self.output.push_str("    _hardy_servo_min[id] = min_us;\n");
                    self.output.push_str("    _hardy_servo_max[id] = max_us;\n");
                    self.output.push_str("    /* Configure TIM2 for 50Hz PWM (20ms period) */\n");
                    self.output.push_str("    _hardy_servo_htim.Instance = TIM2;\n");
                    self.output.push_str("    _hardy_servo_htim.Init.Prescaler = 72 - 1; /* 72MHz/72 = 1MHz tick */\n");
                    self.output.push_str("    _hardy_servo_htim.Init.Period = 20000 - 1; /* 1MHz/20000 = 50Hz */\n");
                    self.output.push_str("    _hardy_servo_htim.Init.CounterMode = TIM_COUNTERMODE_UP;\n");
                    self.output.push_str("    HAL_TIM_PWM_Init(&_hardy_servo_htim);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    int id = -1;\n");
                    self.output.push_str("    for (int i = 0; i < _hardy_servo_count; i++) {\n");
                    self.output.push_str("        if (_hardy_servo_pin[i] == pin) { id = i; break; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (id < 0) return;\n");
                    self.output.push_str("    if (angle < 0) angle = 0;\n");
                    self.output.push_str("    if (angle > 180) angle = 180;\n");
                    self.output.push_str("    int pulse_us = _hardy_servo_min[id] + (angle * (_hardy_servo_max[id] - _hardy_servo_min[id])) / 180;\n");
                    self.output.push_str("    TIM_OC_InitTypeDef oc = {0};\n");
                    self.output.push_str("    oc.OCMode = TIM_OCMODE_PWM1;\n");
                    self.output.push_str("    oc.Pulse = pulse_us;\n");
                    self.output.push_str("    HAL_TIM_PWM_ConfigChannel(&_hardy_servo_htim, &oc, TIM_CHANNEL_1);\n");
                    self.output.push_str("    HAL_TIM_PWM_Start(&_hardy_servo_htim, TIM_CHANNEL_1);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* HardyScript Motor helpers (STM32 HAL GPIO + TIM) */\n");
                    self.output.push_str("#define HARDY_MOTOR_MAX 4\n");
                    self.output.push_str("static int _hardy_motor_in1[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_in2[HARDY_MOTOR_MAX];\n");
                    self.output.push_str("static int _hardy_motor_count = 0;\n\n");
                    self.output.push_str("static inline int hardy_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _hardy_motor_count++;\n");
                    self.output.push_str("    _hardy_motor_in1[id] = in1;\n");
                    self.output.push_str("    _hardy_motor_in2[id] = in2;\n");
                    self.output.push_str("    hardy_gpio_output(in1);\n");
                    self.output.push_str("    hardy_gpio_output(in2);\n");
                    self.output.push_str("    (void)ena; /* PWM config via TIM */\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        hardy_gpio_high(_hardy_motor_in1[id]);\n");
                    self.output.push_str("        hardy_gpio_low(_hardy_motor_in2[id]);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        hardy_gpio_low(_hardy_motor_in1[id]);\n");
                    self.output.push_str("        hardy_gpio_high(_hardy_motor_in2[id]);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    /* TODO: set TIM duty cycle for speed */\n");
                    self.output.push_str("    (void)speed;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void hardy_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _hardy_motor_count) return;\n");
                    self.output.push_str("    hardy_gpio_low(_hardy_motor_in1[id]);\n");
                    self.output.push_str("    hardy_gpio_low(_hardy_motor_in2[id]);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::RaspberryPi => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* HardyScript GPIO helpers (Linux sysfs) */\n");
                    self.output.push_str("static inline void hardy_gpio_export(int pin) {\n");
                    self.output.push_str("    char buf[64];\n");
                    self.output.push_str("    int fd = open(\"/sys/class/gpio/export\", O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) {\n");
                    self.output.push_str("        int n = snprintf(buf, sizeof(buf), \"%d\", pin);\n");
                    self.output.push_str("        write(fd, buf, n);\n");
                    self.output.push_str("        close(fd);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_output(int pin) {\n");
                    self.output.push_str("    hardy_gpio_export(pin);\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/direction\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"out\", 3); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_high(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"1\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void hardy_gpio_low(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"0\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int hardy_gpio_read(int pin) {\n");
                    self.output.push_str("    char path[128], val;\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_RDONLY);\n");
                    self.output.push_str("    if (fd >= 0) { read(fd, &val, 1); close(fd); return val - '0'; }\n");
                    self.output.push_str("    return -1;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* HardyScript Timer helpers (Linux usleep) */\n");
                    self.output.push_str("static inline void hardy_timer_delay(int ms) { usleep(ms * 1000); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* HardyScript UART helpers (Linux serial) */\n");
                    self.output.push_str("static inline void hardy_uart_init(int baud) { (void)baud; /* TODO: termios */ }\n");
                    self.output.push_str("static inline void hardy_uart_write(const char* str) { printf(\"%s\", str); }\n\n");
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
        code.push_str(&format!("/* async function {}() — HardyScript v1.5 状态机 */\n", func.name));
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
        code.push_str(&format!("hardy_status_t {}_poll({}_ctx* ctx) {{\n", func.name, func.name));
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
                code.push_str("            return HARDY_PENDING;\n");
            } else if state.is_final {
                code.push_str("            return HARDY_READY;\n");
            }
            code.push_str("\n");
        }

        code.push_str("    }\n");
        code.push_str("    return HARDY_READY;\n");
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
        code.push_str(&format!("    hardy_spawn((hardy_status_t (*)(void*)){}_poll, &ctx);\n", func.name));
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
                        return Ok(format!("{}int32_t {} = {}; hardy_gpio_output({});\n", pad, name, pin_expr, pin_expr));
                    }
                    if call_name == "GPIO.input" {
                        let pull = if args.len() > 1 { "pullup" } else { "none" };
                        let pin_expr = self.generate_expr(&args[0])?;
                        self.gpio_pins.push((name.clone(), pin_expr.clone()));
                        return Ok(format!("{}int32_t {} = {}; hardy_gpio_input_{}({});\n", pad, name, pin_expr, pull, pin_expr));
                    }
                    if call_name == "PWM.output" {
                        let pin = self.generate_expr(&args[0])?;
                        let freq = if args.len() > 1 { self.generate_expr(&args[1])? } else { "1000".to_string() };
                        return Ok(format!("{}int32_t {} = hardy_pwm_output({}, {});\n", pad, name, pin, freq));
                    }
                    if call_name == "ADC.channel" {
                        let ch = self.generate_expr(&args[0])?;
                        return Ok(format!("{}int32_t {} = {}; hardy_adc_init({});\n", pad, name, ch, ch));
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
        // 裸机目标: 用 UART0 输出 (无 stdio)
        if self.target == CTarget::Esp32S3Bare {
            let mut code = String::new();
            for arg in args {
                match arg {
                    Expr::StringLit(s) => {
                        let escaped = s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                        code.push_str(&format!("{}hardy_uart_puts(\"{}\\r\\n\");\n", pad, escaped));
                    }
                    Expr::Number(n) => {
                        code.push_str(&format!("{}hardy_uart_num({});\n", pad, n));
                        code.push_str(&format!("{}hardy_uart_puts(\"\\r\\n\");\n", pad));
                    }
                    _ => {
                        let c_expr = self.generate_expr(arg)?;
                        code.push_str(&format!("{}hardy_uart_num((u32)({}));\n", pad, c_expr));
                        code.push_str(&format!("{}hardy_uart_puts(\"\\r\\n\");\n", pad));
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
            // led.high() → hardy_gpio_high(pin)
            Expr::Call { name, args } => {
                match name.as_str() {
                    // 对象方法形式: led.high() 被解析为 Call { name: "led.high" }
                    n if n.ends_with(".high") => {
                        let var = n.trim_end_matches(".high");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("hardy_gpio_high({});", pin)));
                        }
                    }
                    n if n.ends_with(".low") => {
                        let var = n.trim_end_matches(".low");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("hardy_gpio_low({});", pin)));
                        }
                    }
                    n if n.ends_with(".toggle") => {
                        let var = n.trim_end_matches(".toggle");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            // toggle: read current state and invert
                            return Ok(Some(format!("hardy_gpio_high({}); /* TODO: toggle */", pin)));
                        }
                    }
                    n if n.ends_with(".read") && args.is_empty() => {
                        let var = n.trim_end_matches(".read");
                        if let Some(pin) = self.find_gpio_pin(var) {
                            return Ok(Some(format!("hardy_gpio_read({})", pin)));
                        }
                    }
                    // Phase 4: Chip Info API (hardy:esp32) — 无参调用
                    "Chip.modelName" | "Chip.model" => {
                        return Ok(Some("hardy_chip_model_name()".to_string()));
                    }
                    "Chip.cores" => {
                        return Ok(Some("hardy_chip_cores()".to_string()));
                    }
                    "Chip.revision" => {
                        return Ok(Some("hardy_chip_revision()".to_string()));
                    }
                    "Chip.flashSize" => {
                        return Ok(Some("hardy_chip_flash_size()".to_string()));
                    }
                    "Chip.freeHeap" => {
                        return Ok(Some("hardy_chip_free_heap()".to_string()));
                    }
                    "Chip.cpuFreq" => {
                        return Ok(Some("hardy_chip_cpu_freq()".to_string()));
                    }
                    "Chip.id" => {
                        return Ok(Some("hardy_chip_id()".to_string()));
                    }
                    "Chip.sdkVersion" => {
                        return Ok(Some("hardy_chip_sdk_version()".to_string()));
                    }
                    "Chip.deviceId" => {
                        return Ok(Some("hardy_chip_device_id()".to_string()));
                    }
                    "Chip.mac" => {
                        return Ok(Some("hardy_chip_mac()".to_string()));
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
                            return Ok(Some(format!("hardy_pwm_duty({}, {});", first_arg, duty)));
                        }
                        n if n.ends_with(".read") && args.is_empty() => {
                            // already handled above
                        }
                        // RGB 灯 (ESP32-S3 板载 WS2812)
                        "RGB.setColor" if args.len() >= 3 => {
                            let g = self.generate_expr(&args[1])?;
                            let b = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("hardy_rgb_set({}, {}, {});", first_arg, g, b)));
                        }
                        "RGB.setHue" => {
                            return Ok(Some(format!("hardy_rgb_hue({});", first_arg)));
                        }
                        // UART / Timer 直接调用
                        "UART.init" => {
                            return Ok(Some(format!("hardy_uart_init({});", first_arg)));
                        }
                        "UART.write" => {
                            self.needs_string = true;
                            let s = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("hardy_uart_write({});", s)));
                        }
                        "Timer.delay" => {
                            match self.target {
                                CTarget::Esp32S3Bare => {
                                    return Ok(Some(format!("hardy_delay_ms({});", first_arg)));
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
                            return Ok(Some(format!("hardy_wifi_connect({}, {});", ssid, pass)));
                        }
                        // Phase F: AI
                        "Brain.init" => {
                            let path = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("hardy_brain_init({});", path)));
                        }
                        "Brain.classify" if args.len() >= 2 => {
                            let input = self.generate_expr(&args[0])?;
                            let labels = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("hardy_brain_classify((float*){}, {}, (const char**){}, {});",
                                input, "/* input_len */", labels, "/* n_labels */")));
                        }
                        // Phase 1.2: Servo/Motor API
                        "Servo.attach" => {
                            let pin = self.generate_expr(&args[0])?;
                            let min_us = if args.len() >= 2 { self.generate_expr(&args[1])? } else { "500".to_string() };
                            let max_us = if args.len() >= 3 { self.generate_expr(&args[2])? } else { "2500".to_string() };
                            return Ok(Some(format!("hardy_servo_attach({}, {}, {});", pin, min_us, max_us)));
                        }
                        "Servo.write" if args.len() >= 2 => {
                            let pin = self.generate_expr(&args[0])?;
                            let angle = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("hardy_servo_write({}, {});", pin, angle)));
                        }
                        "Motor.init" if args.len() >= 3 => {
                            let in1 = self.generate_expr(&args[0])?;
                            let in2 = self.generate_expr(&args[1])?;
                            let ena = self.generate_expr(&args[2])?;
                            return Ok(Some(format!("hardy_motor_init({}, {}, {});", in1, in2, ena)));
                        }
                        "Motor.setSpeed" if args.len() >= 2 => {
                            let id = self.generate_expr(&args[0])?;
                            let speed = self.generate_expr(&args[1])?;
                            return Ok(Some(format!("hardy_motor_set_speed({}, {});", id, speed)));
                        }
                        "Motor.brake" => {
                            let id = self.generate_expr(&args[0])?;
                            return Ok(Some(format!("hardy_motor_brake({});", id)));
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
                let func_name = format!("_hardy_task_fn_{}", self.arrow_counter);
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
                // Phase 2.4: 对已知长度的数组生成边界检查
                if let Expr::Ident(arr_name) = array.as_ref() {
                    if let Some((_, len)) = self.array_vars.iter().find(|(n, _)| n == arr_name) {
                        self.needs_arr_helper = true;
                        return Ok(format!("_hardy_arr_get({}, {}, {})", arr, len, idx));
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
                let func_name = format!("_hardy_arrow_{}", self.arrow_counter);
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
                    let _buf_name = format!("_hardy_tmpl_{}", self.arrow_counter);
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
                helper.push_str(&format!("static uint8_t _hardy_pall_{}_done = 0;\n", id));
                helper.push_str(&format!("static int32_t _hardy_pall_{}_results[{}];\n", id, n));
                helper.push_str(&format!("static uint8_t _hardy_promise_all_{}(void) {{\n", id));
                for (i, p) in promises.iter().enumerate() {
                    let expr_code = self.generate_expr(p)?;
                    helper.push_str(&format!("    if (!(_hardy_pall_{}_done & {})) {{\n", id, 1u32 << i));
                    helper.push_str(&format!("        int32_t _r = {};\n", expr_code));
                    helper.push_str(&format!("        _hardy_pall_{}_results[{}] = _r;\n", id, i));
                    helper.push_str(&format!("        _hardy_pall_{}_done |= {};\n", id, 1u32 << i));
                    helper.push_str("    }\n");
                }
                helper.push_str(&format!("    return (_hardy_pall_{}_done == 0x{:X}) ? 1 : 0;\n", id, mask));
                helper.push_str("}\n");
                self.promise_helpers.push(helper);
                Ok(format!("_hardy_promise_all_{}()", id))
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
                helper.push_str(&format!("static uint8_t _hardy_prace_{}_done = 0;\n", id));
                helper.push_str(&format!("static int32_t _hardy_prace_{}_result = 0;\n", id));
                helper.push_str(&format!("static uint8_t _hardy_promise_race_{}(void) {{\n", id));
                helper.push_str(&format!("    if (_hardy_prace_{}_done) return 1;\n", id));
                for (i, p) in promises.iter().enumerate() {
                    let expr_code = self.generate_expr(p)?;
                    helper.push_str(&format!("    {{ int32_t _r = {}; if (_r != 0) {{ _hardy_prace_{}_result = _r; _hardy_prace_{}_done = {}; return 1; }} }}\n",
                        expr_code, id, id, i + 1));
                }
                helper.push_str("    return 0;\n");
                helper.push_str("}\n");
                helper.push_str(&format!("static int32_t _hardy_promise_race_{}_value(void) {{ return _hardy_prace_{}_result; }}\n", id, id));
                self.promise_helpers.push(helper);
                Ok(format!("_hardy_promise_race_{}()", id))
            }
        }
    }

    /// Phase 3.3: 生成定时器中断初始化代码
    fn generate_timer_isr_init(&self) -> String {
        let mut code = String::new();
        code.push_str("    /* HardyScript timer ISR init */\n");

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
        let struct_name = format!("_hardy_obj_{}", name);

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
        let func_name = format!("_hardy_fn_{}_{}", name, self.arrow_counter);

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
            "import { GPIO, Timer } from \"hardy:hw\"; let led = GPIO.output(2);",
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
            "import { GPIO } from \"hardy:hw\"; let led = GPIO.output(2);",
            CTarget::Esp32
        );
        assert!(c.contains("hardy_gpio_output(2)"));
    }

    #[test]
    fn test_timer_delay_esp32() {
        let c = compile_to_c(
            "import { Timer } from \"hardy:hw\"; Timer.delay(500);",
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
        assert!(c.contains("_hardy_fn_add"));
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
            "import { GPIO } from \"hardy:hw\"; let led = GPIO.output(13);",
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
            "import { GPIO, Timer } from \"hardy:hw\"; let led = GPIO.output(13); while (true) { led.high(); Timer.delay(1000); led.low(); Timer.delay(1000); }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("void setup()"));
        assert!(c.contains("void loop()"));
        assert!(c.contains("delay(1000)"));
        // setup 中应有 GPIO 初始化
        assert!(c.contains("hardy_gpio_output(13)"));
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
            "import { GPIO } from \"hardy:hw\"; let led = GPIO.output(13); while (true) { led.high(); led.low(); }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("hardy_gpio_high(13)"));
        assert!(c.contains("hardy_gpio_low(13)"));
    }

    #[test]
    fn test_arduino_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"hardy:hw\"; Timer.delay(500);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("delay(500)"));
    }

    #[test]
    fn test_arduino_arm_target() {
        let c = compile_to_c(
            "import { GPIO, Timer } from \"hardy:hw\"; let led = GPIO.output(2); while (true) { led.high(); Timer.delay(100); led.low(); Timer.delay(100); }",
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
            "import { GPIO } from \"hardy:hw\"; let x = GPIO.output(5);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("HardyScript GPIO helpers (Arduino AVR)"));
    }

    #[test]
    fn test_arduino_arm_helpers_comment() {
        let c = compile_to_c(
            "import { GPIO } from \"hardy:hw\"; let x = GPIO.output(5);",
            CTarget::ArduinoArm
        );
        assert!(c.contains("HardyScript GPIO helpers (Arduino ARM)"));
    }

    // ========== STM32 后端测试 ==========

    #[test]
    fn test_stm32_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"hardy:hw\"; let led = GPIO.output(5);",
            CTarget::Stm32
        );
        assert!(c.contains("#include \"stm32f1xx_hal.h\""));
    }

    #[test]
    fn test_stm32_gpio_helpers() {
        let c = compile_to_c(
            "import { GPIO } from \"hardy:hw\"; let led = GPIO.output(5); while (true) { led.high(); led.low(); }",
            CTarget::Stm32
        );
        assert!(c.contains("HardyScript GPIO helpers (STM32 HAL)"));
        assert!(c.contains("HAL_GPIO_WritePin"));
        assert!(c.contains("hardy_gpio_high(5)"));
        assert!(c.contains("hardy_gpio_low(5)"));
    }

    #[test]
    fn test_stm32_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"hardy:hw\"; Timer.delay(500);",
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
            "import { UART } from \"hardy:hw\"; UART.init(115200);",
            CTarget::Stm32
        );
        assert!(c.contains("#include <string.h>"));
        assert!(c.contains("hardy_uart_init(115200)"));
    }

    // ========== 树莓派后端测试 ==========

    #[test]
    fn test_rpi_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"hardy:hw\"; let led = GPIO.output(17);",
            CTarget::RaspberryPi
        );
        assert!(c.contains("#include <fcntl.h>"));
        assert!(c.contains("#include <unistd.h>"));
    }

    #[test]
    fn test_rpi_gpio_sysfs() {
        let c = compile_to_c(
            "import { GPIO } from \"hardy:hw\"; let led = GPIO.output(17); while (true) { led.high(); led.low(); }",
            CTarget::RaspberryPi
        );
        assert!(c.contains("HardyScript GPIO helpers (Linux sysfs)"));
        assert!(c.contains("/sys/class/gpio/"));
        assert!(c.contains("hardy_gpio_high(17)"));
        assert!(c.contains("hardy_gpio_low(17)"));
    }

    #[test]
    fn test_rpi_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"hardy:hw\"; Timer.delay(1000);",
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
        assert!(c.contains("HARDY_PENDING"), "应返回 PENDING");
        assert!(c.contains("HARDY_READY"), "应返回 READY");
        assert!(c.contains("switch (ctx->state)"), "应包含状态分发");
    }

    #[test]
    fn test_cgen_async_spawn() {
        let c = compile_to_c("async function foo() { let x = await bar(); return x; }", CTarget::Esp32);
        assert!(c.contains("foo_spawn"), "应生成 spawn 函数");
        assert!(c.contains("hardy_spawn"), "应调用 hardy_spawn");
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

    // Phase 1.1: Promise.all/race 代码生成测试
    #[test]
    fn test_promise_all_codegen() {
        let c = compile_to_c(
            "async function multi() { let r = await Promise.all([readA(), readB()]); return r; }",
            CTarget::Esp32
        );
        assert!(c.contains("_hardy_promise_all_1"), "应生成 Promise.all 辅助函数");
        assert!(c.contains("_hardy_pall_1_done"), "应生成完成标志");
        assert!(c.contains("_hardy_pall_1_results"), "应生成结果数组");
        assert!(c.contains("0x3"), "应检查所有任务完成 (2任务 = 0x3)");
    }

    #[test]
    fn test_promise_race_codegen() {
        let c = compile_to_c(
            "async function race() { let r = await Promise.race([sensorA(), sensorB(), sensorC()]); return r; }",
            CTarget::Esp32
        );
        assert!(c.contains("_hardy_promise_race_1"), "应生成 Promise.race 辅助函数");
        assert!(c.contains("_hardy_prace_1_done"), "应生成完成标志");
        assert!(c.contains("_hardy_prace_1_result"), "应生成结果变量");
    }

    #[test]
    fn test_promise_all_empty() {
        let c = compile_to_c(
            "async function empty() { let r = await Promise.all([]); return r; }",
            CTarget::Esp32
        );
        // 空 Promise.all 应直接返回 1 (立即完成)
        assert!(c.contains("multi_ctx") || c.contains("empty_ctx"), "应生成状态机");
    }

    // Phase 1.2: Servo/Motor 代码生成测试
    #[test]
    fn test_servo_attach_esp32() {
        let c = compile_to_c(
            "import { Servo } from \"hardy:motor\"; Servo.attach(13, 500, 2500);",
            CTarget::Esp32
        );
        assert!(c.contains("#include \"driver/ledc.h\""), "应包含 LEDC 头文件");
        assert!(c.contains("HardyScript Servo helpers (ESP32 LEDC 50Hz PWM)"), "应生成 Servo 辅助函数");
        assert!(c.contains("hardy_servo_attach(13, 500, 2500);"), "应调用 hardy_servo_attach");
    }

    #[test]
    fn test_servo_write_esp32() {
        let c = compile_to_c(
            "import { Servo } from \"hardy:motor\"; Servo.attach(13, 500, 2500); Servo.write(13, 90);",
            CTarget::Esp32
        );
        assert!(c.contains("hardy_servo_write(13, 90);"), "应调用 hardy_servo_write");
        assert!(c.contains("hardy_servo_attach"), "应包含 attach 辅助函数");
    }

    #[test]
    fn test_servo_arduino() {
        let c = compile_to_c(
            "import { Servo } from \"hardy:motor\"; Servo.attach(9, 500, 2500); Servo.write(9, 45);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("#include <Servo.h>"), "应包含 Servo.h");
        assert!(c.contains("HardyScript Servo helpers (Arduino Servo.h)"), "应生成 Arduino Servo 辅助");
        assert!(c.contains("hardy_servo_attach(9, 500, 2500);"), "应调用 attach");
        assert!(c.contains("hardy_servo_write(9, 45);"), "应调用 write");
    }

    #[test]
    fn test_servo_stm32() {
        let c = compile_to_c(
            "import { Servo } from \"hardy:motor\"; Servo.attach(0, 500, 2500); Servo.write(0, 180);",
            CTarget::Stm32
        );
        assert!(c.contains("HardyScript Servo helpers (STM32 TIM2 PWM 50Hz)"), "应生成 STM32 Servo 辅助");
        assert!(c.contains("HAL_TIM_PWM_Init"), "应调用 HAL TIM PWM");
        assert!(c.contains("hardy_servo_write(0, 180);"), "应调用 write");
    }

    #[test]
    fn test_motor_init_esp32() {
        let c = compile_to_c(
            "import { Motor } from \"hardy:motor\"; Motor.init(2, 3, 5);",
            CTarget::Esp32
        );
        assert!(c.contains("HardyScript Motor helpers (ESP32 LEDC + GPIO)"), "应生成 Motor 辅助函数");
        assert!(c.contains("hardy_motor_init(2, 3, 5);"), "应调用 hardy_motor_init");
    }

    #[test]
    fn test_motor_set_speed_esp32() {
        let c = compile_to_c(
            "import { Motor } from \"hardy:motor\"; Motor.init(2, 3, 5); Motor.setSpeed(0, 200);",
            CTarget::Esp32
        );
        assert!(c.contains("hardy_motor_set_speed(0, 200);"), "应调用 hardy_motor_set_speed");
    }

    #[test]
    fn test_motor_brake_esp32() {
        let c = compile_to_c(
            "import { Motor } from \"hardy:motor\"; Motor.init(2, 3, 5); Motor.brake(0);",
            CTarget::Esp32
        );
        assert!(c.contains("hardy_motor_brake(0);"), "应调用 hardy_motor_brake");
    }

    #[test]
    fn test_motor_arduino() {
        let c = compile_to_c(
            "import { Motor } from \"hardy:motor\"; Motor.init(4, 5, 6); Motor.setSpeed(0, 128); Motor.brake(0);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("HardyScript Motor helpers (Arduino AVR)"), "应生成 Arduino Motor 辅助");
        assert!(c.contains("hardy_motor_init(4, 5, 6);"), "应调用 init");
        assert!(c.contains("hardy_motor_set_speed(0, 128);"), "应调用 setSpeed");
        assert!(c.contains("hardy_motor_brake(0);"), "应调用 brake");
    }

    #[test]
    fn test_servo_attach_defaults() {
        let c = compile_to_c(
            "import { Servo } from \"hardy:motor\"; Servo.attach(11);",
            CTarget::Esp32
        );
        // 默认 min=500, max=2500
        assert!(c.contains("hardy_servo_attach(11, 500, 2500);"), "应使用默认脉宽");
    }

    // Phase 1.3: 调度器代码生成测试
    #[test]
    fn test_set_interval_esp32() {
        let c = compile_to_c(
            "function tick() { let x = 1; } setInterval(tick, 100);",
            CTarget::Esp32
        );
        assert!(c.contains("_hardy_task_0"), "应生成任务包装函数");
        assert!(c.contains("xTaskCreate"), "应调用 xTaskCreate");
        assert!(c.contains("tick()"), "应调用回调函数");
        assert!(c.contains("vTaskDelay(pdMS_TO_TICKS(100))"), "应包含延时期望值");
    }

    #[test]
    fn test_set_interval_arduino() {
        let c = compile_to_c(
            "function blink() { let x = 1; } setInterval(blink, 500);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("_hardy_scheduler_poll"), "应生成调度器 poll");
        assert!(c.contains("millis()"), "应使用 millis()");
        assert!(c.contains("blink()"), "应调用回调");
        assert!(c.contains("_hardy_task_0_last"), "应生成时间戳变量");
    }

    #[test]
    fn test_set_timeout_esp32() {
        let c = compile_to_c(
            "function initOnce() { let x = 1; } setTimeout(initOnce, 3000);",
            CTarget::Esp32
        );
        assert!(c.contains("vTaskDelete(NULL)"), "单次任务应自删除");
        assert!(c.contains("vTaskDelay(pdMS_TO_TICKS(3000))"), "应延时 3000ms");
    }

    #[test]
    fn test_set_interval_arrow_fn() {
        let c = compile_to_c(
            "setInterval(() => { let x = 1; }, 50);",
            CTarget::Esp32
        );
        assert!(c.contains("_hardy_task_fn_"), "应提升箭头函数");
        assert!(c.contains("xTaskCreate"), "应创建任务");
    }

    #[test]
    fn test_multiple_intervals_esp32() {
        let c = compile_to_c(
            "function a() { let x = 1; } function b() { let y = 2; } setInterval(a, 10); setInterval(b, 100);",
            CTarget::Esp32
        );
        assert!(c.contains("_hardy_task_0"), "应生成任务0");
        assert!(c.contains("_hardy_task_1"), "应生成任务1");
        assert!(c.contains("a()"), "应调用 a");
        assert!(c.contains("b()"), "应调用 b");
    }

    #[test]
    fn test_set_interval_stm32() {
        let c = compile_to_c(
            "function ctrl() { let x = 1; } setInterval(ctrl, 1);",
            CTarget::Stm32
        );
        assert!(c.contains("HAL_GetTick()"), "应使用 HAL_GetTick");
        assert!(c.contains("_hardy_scheduler_poll"), "应生成 poll");
        assert!(c.contains("ctrl()"), "应调用回调");
    }

    // Phase 1.4: 端到端 Demo 测试
    #[test]
    fn test_robot_arm_esp32_end_to_end() {
        let source = r#"
import { Servo } from "hardy:motor";
import { ADC, Timer } from "hardy:hw";

const GRIP = 13;
const ELBOW = 12;
const SHOULDER = 14;

Servo.attach(SHOULDER, 500, 2500);
Servo.attach(ELBOW, 500, 2500);
Servo.attach(GRIP, 500, 2500);

async function grabAndPour() {
  Servo.write(SHOULDER, 45);
  await Timer.delay(500);
  Servo.write(GRIP, 90);
  await Timer.delay(200);
  Servo.write(ELBOW, 120);
  await Timer.delay(300);
  Servo.write(GRIP, 30);
}

function readSensors() {
  let pot = ADC.read(0);
  let angle = pot / 22;
  Servo.write(SHOULDER, angle);
}

setInterval(grabAndPour, 5000);
setInterval(readSensors, 10);
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        // 验证头文件
        assert!(c.contains("#include \"driver/ledc.h\""), "应包含 LEDC");
        assert!(c.contains("#include \"driver/gpio.h\""), "应包含 GPIO");
        assert!(c.contains("#include \"driver/adc.h\""), "应包含 ADC");
        assert!(c.contains("freertos/FreeRTOS.h"), "应包含 FreeRTOS");
        // 验证 Servo 辅助函数
        assert!(c.contains("hardy_servo_attach"), "应生成 servo attach");
        assert!(c.contains("hardy_servo_write"), "应生成 servo write");
        // 验证 ADC 辅助函数
        assert!(c.contains("hardy_adc_read"), "应生成 adc read");
        // 验证 async 状态机
        assert!(c.contains("grabAndPour_ctx"), "应生成 async 上下文");
        assert!(c.contains("grabAndPour_poll"), "应生成 poll 函数");
        // 验证调度器
        assert!(c.contains("xTaskCreate"), "应创建 FreeRTOS 任务");
        assert!(c.contains("_hardy_task_0"), "应生成任务0");
        assert!(c.contains("_hardy_task_1"), "应生成任务1");
        // 验证 app_main
        assert!(c.contains("void app_main(void)"), "应生成 app_main");
    }

    #[test]
    fn test_robot_arm_arduino_end_to_end() {
        let source = r#"
import { Servo } from "hardy:motor";
import { Timer } from "hardy:hw";

Servo.attach(9, 500, 2500);

function sweep() {
  Servo.write(9, 0);
  Servo.write(9, 180);
}

setInterval(sweep, 2000);
"#;
        let c = compile_to_c(source, CTarget::ArduinoAvr);
        assert!(c.contains("#include <Servo.h>"), "应包含 Servo.h");
        assert!(c.contains("void setup()"), "应生成 setup");
        assert!(c.contains("void loop()"), "应生成 loop");
        assert!(c.contains("_hardy_scheduler_poll()"), "loop 中应调用调度器");
        assert!(c.contains("hardy_servo_write(9, 0)"), "应调用 servo write");
    }

    // Phase 2.3: 对象 → C struct 测试
    #[test]
    fn test_object_to_struct_basic() {
        let source = r#"
function test() {
  let pos = { x: 10, y: 20, z: 30 };
  return pos.x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("struct _hardy_obj_pos"), "应生成 struct 定义");
        assert!(c.contains("int32_t x;"), "struct 应包含 x 字段");
        assert!(c.contains("int32_t y;"), "struct 应包含 y 字段");
        assert!(c.contains("int32_t z;"), "struct 应包含 z 字段");
        assert!(c.contains(".x = 10"), "应生成初始化");
        assert!(c.contains(".y = 20"), "应生成初始化");
        assert!(c.contains(".z = 30"), "应生成初始化");
    }

    #[test]
    fn test_object_struct_no_malloc() {
        let source = r#"
function make() {
  let obj = { a: 1, b: 2 };
  obj.a = 99;
  return obj.a;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(!c.contains("malloc"), "不应包含 malloc");
        assert!(!c.contains("calloc"), "不应包含 calloc");
        assert!(c.contains("struct _hardy_obj_obj"), "应生成 struct");
        assert!(c.contains("obj.a = 99"), "应生成属性赋值");
    }

    #[test]
    fn test_multiple_objects_different_structs() {
        let source = r#"
function test() {
  let pos = { x: 1, y: 2 };
  let vel = { vx: 3, vy: 4 };
  return pos.x + vel.vx;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("struct _hardy_obj_pos"), "应生成 pos struct");
        assert!(c.contains("struct _hardy_obj_vel"), "应生成 vel struct");
        assert!(c.contains("int32_t vx;"), "vel 应有 vx 字段");
    }

    // Phase 2.4: 数组边界检查测试
    #[test]
    fn test_array_fixed_length() {
        let source = r#"
function test() {
  let arr = [10, 20, 30, 40, 50];
  return arr[2];
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("int32_t arr[5]"), "应生成定长数组");
        assert!(c.contains("_hardy_arr_get(arr, 5, 2)"), "应生成边界检查访问");
        assert!(c.contains("_hardy_arr_get"), "应包含 helper 函数");
        assert!(c.contains("ARRAY_OOB"), "应包含越界提示");
    }

    #[test]
    fn test_array_bounds_no_malloc() {
        let source = r#"
function test() {
  let data = [1, 2, 3];
  let x = data[0];
  return x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(!c.contains("malloc"), "不应包含 malloc");
        assert!(c.contains("int32_t data[3]"), "应生成定长数组");
    }

    #[test]
    fn test_array_runtime_index_checked() {
        let source = r#"
function test(i) {
  let arr = [1, 2, 3, 4];
  return arr[i];
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("_hardy_arr_get(arr, 4, i)"), "运行时索引应边界检查");
    }

    // Phase 3.3: 硬件定时器中断映射测试
    #[test]
    fn test_realtime_annotation_in_output() {
        let source = r#"
@realtime
function motorCtrl() {
  let x = 1;
  return x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("/* @realtime */"), "应包含 @realtime 标记");
        assert!(c.contains("int32_t motorCtrl(void)"), "应生成函数");
    }

    #[test]
    fn test_interrupt_esp32() {
        let source = r#"
@interrupt("TIM2", period_us: 100)
function controlLoop() {
  let x = 1;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("timer ISR init"), "应包含定时器初始化");
        assert!(c.contains("TIM2"), "应包含定时器名");
        assert!(c.contains("100us"), "应包含周期");
        assert!(c.contains("controlLoop"), "应包含回调函数名");
    }

    #[test]
    fn test_interrupt_stm32() {
        let source = r#"
@interrupt("TIM3", period_us: 50)
function pid_loop() {
  let err = 0;
}
"#;
        let c = compile_to_c(source, CTarget::Stm32);
        assert!(c.contains("STM32 HAL"), "应包含 STM32 HAL 注释");
        assert!(c.contains("TIM3"), "应包含定时器名");
        assert!(c.contains("Prescaler=71"), "应包含预分频");
    }

    #[test]
    fn test_interrupt_arduino() {
        let source = r#"
@interrupt("TIMER2", period_us: 1000)
function blink_isr() {
  let x = 1;
}
"#;
        let c = compile_to_c(source, CTarget::ArduinoAvr);
        assert!(c.contains("ISR(TIMER2_COMPA_vect)"), "应包含 Arduino ISR");
        assert!(c.contains("OCR2A"), "应包含 OCR2A 设置");
    }

    #[test]
    fn test_priority_realtime_combined() {
        let source = r#"
@priority(0)
@realtime(500)
function highest() {
  let x = 1;
  return x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("/* @realtime */"), "应包含 @realtime 标记");
        assert!(c.contains("int32_t highest(void)"), "应生成函数");
    }

    // ========== ESP32-S3 裸机 RGB 后端 (examples/rgb_on.js + rgb_rainbow.js) ==========

    #[test]
    fn test_bare_rgb_on() {
        // 示例 1: 点亮 RGB 灯 (与 DUANYAN-IDE/examples/rgb_on.js 同源)
        let source = r#"
import { RGB, Timer } from "hardy:hw";

while (true) {
    RGB.setColor(255, 255, 255);
    Timer.delay(500);
}
"#;
        let c = compile_to_c(source, CTarget::Esp32S3Bare);
        // HAL 关键件: 入口/供电使能/WS2812 时序
        assert!(c.contains("void _start(void)"), "应生成裸机入口 _start");
        assert!(c.contains("hardy_boot_init();"), "应调用 boot 初始化");
        assert!(c.contains("hardy_pin_setup(47)"), "应使能 WS2812 供电脚 GPIO47");
        assert!(c.contains("hardy_rgb_set(255, 255, 255);"), "RGB.setColor 应映射到 hardy_rgb_set");
        assert!(c.contains("hardy_delay_ms(500);"), "Timer.delay 应映射到 hardy_delay_ms");
        assert!(c.contains("hardy_ws2812_send"), "应包含 WS2812 驱动");
        // 不得引入 ESP-IDF 依赖
        assert!(!c.contains("freertos"), "裸机目标不得依赖 FreeRTOS");
    }

    #[test]
    fn test_bare_rgb_rainbow() {
        // 示例 2: 彩虹渐变 (与 DUANYAN-IDE/examples/rgb_rainbow.js 同源)
        let source = r#"
import { RGB, Timer } from "hardy:hw";

let hue = 0;

while (true) {
    RGB.setHue(hue);
    hue = hue + 1;
    if (hue >= 256) {
        hue = 0;
    }
    Timer.delay(20);
}
"#;
        let c = compile_to_c(source, CTarget::Esp32S3Bare);
        assert!(c.contains("int32_t hue = 0;"), "应声明 hue 变量");
        assert!(c.contains("hardy_rgb_hue(hue);"), "RGB.setHue 应映射到色轮函数");
        assert!(c.contains("hue = (hue + 1);"), "应生成自增");
        assert!(c.contains("if ((hue >= 256))"), "应生成回绕判断");
        assert!(c.contains("hardy_delay_ms(20);"), "应生成 20ms 帧延时");
    }

    #[test]
    fn test_bare_console_log() {
        let source = r#"
console.log("boot ok");
console.log(42);
"#;
        let c = compile_to_c(source, CTarget::Esp32S3Bare);
        assert!(c.contains("hardy_uart_puts(\"boot ok\\r\\n\");"), "字符串应走 UART");
        assert!(c.contains("hardy_uart_num(42);"), "数字应走 UART");
        assert!(!c.contains("printf"), "裸机不得使用 printf");
    }
}
