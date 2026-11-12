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
}

/// ESP32-S3 裸机 HAL (源自 DUANYAN 实测跑通的 led_on.c / gradient.c)
/// 数据脚 GPIO48 (RGB_BUILTIN), 供电使能脚 GPIO47
const ESP32S3_BARE_HAL: &str = r#"
/* ===== DUANYAN ESP32-S3 bare-metal HAL (WS2812 onboard RGB) ===== */
typedef unsigned int u32;
#define DUANYAN_REG(addr) (*(volatile u32 *)(addr))

#define DUANYAN_UART0_FIFO   DUANYAN_REG(0x60000000)
#define DUANYAN_UART0_STATUS DUANYAN_REG(0x6000001C)
#define DUANYAN_RTC_WDTCONFIG0  DUANYAN_REG(0x60008098)
#define DUANYAN_RTC_WDTFEED     DUANYAN_REG(0x600080AC)
#define DUANYAN_RTC_WDTWPROTECT DUANYAN_REG(0x600080B0)
#define DUANYAN_RTC_SWD_CONF    DUANYAN_REG(0x600080B4)
#define DUANYAN_RTC_SWD_WPROT   DUANYAN_REG(0x600080B8)
#define DUANYAN_TIMG0_T0CONFIG  DUANYAN_REG(0x6001F000)
#define DUANYAN_TIMG0_T0LO      DUANYAN_REG(0x6001F004)
#define DUANYAN_TIMG0_T0UPDATE  DUANYAN_REG(0x6001F00C)
#define DUANYAN_TIMG0_WDTCONFIG0  DUANYAN_REG(0x6001F048)
#define DUANYAN_TIMG0_WDTWPROTECT DUANYAN_REG(0x6001F064)
#define DUANYAN_TIMG0_INT_CLR     DUANYAN_REG(0x6001F07C)
#define DUANYAN_GPIO_BASE  0x60004000UL
#define DUANYAN_IOMUX_BASE 0x60009000UL

/* RMT 外设 (硬件波形发生器, 时序由 80MHz 时钟树保证, 与 CPU 频率无关) */
#define DUANYAN_PERIP_CLK_EN0  DUANYAN_REG(0x600C0018)
#define DUANYAN_RMT_BASE       0x60016000UL
#define DUANYAN_RMT_RAM_CH0    0x60016800UL
#define DUANYAN_RMT_CH0CONF0   DUANYAN_REG(DUANYAN_RMT_BASE + 0x20)
/* S3 真实偏移 (核对 esp-idf rmt_struct.h): 早期用的 0x74/0x78/0x7C
 * 落在载波占空比寄存器上, 导致中断从未被正确清除/轮询 → 帧撕裂闪烁 */
#define DUANYAN_RMT_INT_RAW    DUANYAN_REG(DUANYAN_RMT_BASE + 0x60)
#define DUANYAN_RMT_INT_ST     DUANYAN_REG(DUANYAN_RMT_BASE + 0x64)
#define DUANYAN_RMT_INT_ENA    DUANYAN_REG(DUANYAN_RMT_BASE + 0x68)
#define DUANYAN_RMT_INT_CLR    DUANYAN_REG(DUANYAN_RMT_BASE + 0x6C)
#define DUANYAN_RMT_SYS_CONF   DUANYAN_REG(DUANYAN_RMT_BASE + 0xC0)
/* WS2812 时序 @ RMT sclk 实测 ≈11.7MHz (tick≈85ns)。
 * 测量方法: 9400 tick 长帧(50%占空) + CPU 轮询 GPIO48 输入电平采样,
 * 高电平样本数经 UART 字符锚定(LPC)换算帧时长=804us -> sclk=9400/804us。
 * 判决验证: 红蓝交替纯色无杂光 + 白->红 64 级渐变丝滑无闪。
 * 此前误设 T0H=5(0.39~0.43us) 越过灯珠判决门限 → "0"码全读成 1 → 白灯;
 * T0H=1(85ns) 又短到灯珠不认 → 同样全白。
 * 现 T0H=0.26us/T0L=1.11us/T1H=0.94us/T1L=0.43us, 周期 16tick≈1.37us */
#define DUANYAN_WS_T0H 3u
#define DUANYAN_WS_T0L 13u
#define DUANYAN_WS_T1H 11u
#define DUANYAN_WS_T1L 5u

/* SAR ADC1 (RTC 控制器 oneshot)。寄存器布局逐项对照 ESP-IDF v5.2 sens_reg.h /
 * adc_ll.h 核实。★实测根因: 不开 SENS+0x104 bit30 (saradc_clk_en) 时钟门,
 * start 置位后 done 永不置位 (YL-69 探针 yl69.c 实测验证)。
 * 干湿实测: 干探头 RAW=4095, 湿探头 RAW≈3300, 干燥曲线 3301→3838→4095 */
#define DUANYAN_SENS_MEAS1_CTRL1 DUANYAN_REG(0x60008808)
#define DUANYAN_SENS_MEAS1_CTRL2 DUANYAN_REG(0x6000880C)
#define DUANYAN_SENS_MEAS1_MUX   DUANYAN_REG(0x60008810)
#define DUANYAN_SENS_ATTEN1      DUANYAN_REG(0x60008814)
#define DUANYAN_SENS_POWER_XPD   DUANYAN_REG(0x6000883C)
#define DUANYAN_SENS_SLAVE_ADDR1 DUANYAN_REG(0x60008840)
#define DUANYAN_SENS_CLK_GATE    DUANYAN_REG(0x60008904)  /* bit30 saradc_clk_en */
#define DUANYAN_SENS_RESET       DUANYAN_REG(0x60008908)  /* bit30 saradc_reset */
#define DUANYAN_SARADC_CTRL      DUANYAN_REG(0x60040000)  /* APB_SARADC 数字侧 */
#define DUANYAN_SARADC_CTRL2     DUANYAN_REG(0x60040004)

static u32 _duanyan_ticks_per_us = 10;

static void duanyan_uart_putc(char c) {
    while (((DUANYAN_UART0_STATUS >> 16) & 0x3FF) >= 127) { }
    DUANYAN_UART0_FIFO = (u32)c;
}
static void duanyan_uart_puts(const char *s) { while (*s) duanyan_uart_putc(*s++); }
static void duanyan_uart_num(u32 v) {
    char tmp[10]; int t = 0;
    do { tmp[t++] = (char)('0' + (v % 10)); v /= 10; } while (v > 0);
    while (t > 0) duanyan_uart_putc(tmp[--t]);
}
static u32 duanyan_timer_now(void) { DUANYAN_TIMG0_T0UPDATE = 1; return DUANYAN_TIMG0_T0LO; }
static void duanyan_wait_ticks(u32 start, u32 ticks) {
    u32 cap = (ticks + 10) * 100 + 100000;
    while ((duanyan_timer_now() - start) < ticks && cap--) { }
}
static void duanyan_delay_us(u32 us) { duanyan_wait_ticks(duanyan_timer_now(), us * _duanyan_ticks_per_us); }
static void duanyan_delay_ms(u32 ms) { duanyan_wait_ticks(duanyan_timer_now(), ms * 1000 * _duanyan_ticks_per_us); }
/* UART 锚定等待: 1 字符 = 86.8us 绝对基准 (115200 波特), 完全不依赖
 * timer/校准/时钟树——这块板 timer 与校准均不可信, UART 是
 * 全系统唯一被实测验证精确的时间基准 */
static void duanyan_uart_drain(void) {
    while (((DUANYAN_UART0_STATUS >> 16) & 0x3FF) != 0) { }
}
/* 开机自校准: 用 UART 115200 绝对基准实测 ticks/us。
 * 长采样 1000 字符 (~87ms): 不同板卡 timer 频率差异巨大 (实测过
 * 10MHz 与 155MHz 两种), 短采样 10 字符的量化误差会把 tpu 测错
 * 数倍, 导致所有延时/帧等待失真 → WS2812 帧尾撕裂闪烁 */
static void duanyan_calibrate(void) {
    u32 t0 = duanyan_timer_now();
    for (int i = 0; i < 1000; i++) duanyan_uart_putc('~');
    while (((DUANYAN_UART0_STATUS >> 16) & 0x3FF) != 0) { }
    u32 ticks = duanyan_timer_now() - t0;
    _duanyan_ticks_per_us = ticks / 86800;
    if (_duanyan_ticks_per_us == 0) _duanyan_ticks_per_us = 10;
}
static void duanyan_pin_setup(u32 pin) {
    DUANYAN_REG(DUANYAN_IOMUX_BASE + pin * 4) = (1u << 12) | (3u << 10);
    DUANYAN_REG(DUANYAN_GPIO_BASE + 0x554 + pin * 4) = pin;
    if (pin >= 32) DUANYAN_REG(DUANYAN_GPIO_BASE + 0x30) = 1u << (pin - 32);
    else DUANYAN_REG(DUANYAN_GPIO_BASE + 0x24) = 1u << pin;
}
static void duanyan_pin_hi(u32 pin) {
    if (pin >= 32) DUANYAN_REG(DUANYAN_GPIO_BASE + 0x14) = 1u << (pin - 32);
    else DUANYAN_REG(DUANYAN_GPIO_BASE + 0x08) = 1u << pin;
}
static void duanyan_pin_lo(u32 pin) {
    if (pin >= 32) DUANYAN_REG(DUANYAN_GPIO_BASE + 0x18) = 1u << (pin - 32);
    else DUANYAN_REG(DUANYAN_GPIO_BASE + 0x0C) = 1u << pin;
}
/* GPIO 统一 API 包装: JS 侧 GPIO.output/high/low/read 生成的是 duanyan_gpio_*
 * 调用（与其它后端同名），bare HAL 内部用 pin_setup/hi/lo 实现。
 * 缺少这层包装时 blink 类程序直接 gcc 报 implicit declaration (2026-10 网站阶段 0 实测发现) */
static inline void duanyan_gpio_output(int pin) { duanyan_pin_setup((u32)pin); }
static inline void duanyan_gpio_high(int pin) { duanyan_pin_hi((u32)pin); }
static inline void duanyan_gpio_low(int pin)  { duanyan_pin_lo((u32)pin); }
static inline void duanyan_gpio_input_pullup(int pin) {
    u32 p = (u32)pin;
    /* FUN_IE(bit12) + FUN_WPU(bit11) 上拉输入；清输出使能 (ENABLE_W1TC 0x28/0x34) */
    DUANYAN_REG(DUANYAN_IOMUX_BASE + p * 4) = (1u << 12) | (1u << 11);
    DUANYAN_REG(DUANYAN_GPIO_BASE + 0x554 + p * 4) = p;
    if (pin >= 32) DUANYAN_REG(DUANYAN_GPIO_BASE + 0x34) = 1u << (pin - 32);
    else DUANYAN_REG(DUANYAN_GPIO_BASE + 0x28) = 1u << pin;
}
static inline int duanyan_gpio_read(int pin) {
    /* GPIO_IN_REG=0x3C (pin<32), GPIO_IN1_REG=0x40 */
    if (pin >= 32) return (int)((DUANYAN_REG(DUANYAN_GPIO_BASE + 0x40) >> (pin - 32)) & 1u);
    return (int)((DUANYAN_REG(DUANYAN_GPIO_BASE + 0x3C) >> pin) & 1u);
}
/* RMT 初始化: 通道0 单发模式, 输出路由到 GPIO48 (信号81) */
static void duanyan_rmt_init(void) {
    DUANYAN_PERIP_CLK_EN0 |= (1u << 9);             /* RMT 时钟使能 */
    DUANYAN_RMT_SYS_CONF = (1u << 0) | (1u << 1) | (1u << 3)
                       | (1u << 4) | (1u << 24) | (1u << 26) | (1u << 31);
    DUANYAN_RMT_CH0CONF0 = (1u << 8) | (1u << 16) | (1u << 6); /* div=1, 1块RAM, idle低 */
    DUANYAN_RMT_CH0CONF0 |= (1u << 24);             /* CONF_UPDATE */
    /* 数据脚 GPIO48 交给 RMT: IO_MUX 设 GPIO 功能, FUNC_OUT_SEL = 81 */
    DUANYAN_REG(DUANYAN_IOMUX_BASE + 48 * 4) = (1u << 12) | (3u << 10);
    DUANYAN_REG(DUANYAN_GPIO_BASE + 0x554 + 48 * 4) = 81;
}
/* WS2812: 经 RMT 硬件发送 24bit GRB (MSB first), 帧含 50us 锁存间隙。
 * 完成等待用 UART 锚定 (2 锚=174us > 帧最长 44us): 物理上保证发完
 * 才重写 RAM, 不依赖任何校准; TX_END 在部分板上不置位, 仅作快路径 */
static void duanyan_ws2812_send(u32 grb) {
    volatile u32 *ram = (volatile u32 *)DUANYAN_RMT_RAM_CH0;
    const u32 BASE_CONF = (1u << 8) | (1u << 16) | (1u << 6);
    for (int i = 0; i < 24; i++) {
        u32 hi = DUANYAN_WS_T0H, lo = DUANYAN_WS_T0L;
        if ((grb >> (23 - i)) & 1) { hi = DUANYAN_WS_T1H; lo = DUANYAN_WS_T1L; }
        ram[i] = hi | (1u << 15) | (lo << 16);
    }
    ram[24] = 128;   /* ~11us 低电平帧尾 (@11.7MHz), 后续空闲低电平提供锁存间隙 */
    ram[25] = 0;     /* 结束标记 */
    DUANYAN_RMT_INT_CLR = 1;
    DUANYAN_RMT_CH0CONF0 = BASE_CONF | (1u << 1) | (1u << 2);  /* 复位读指针 */
    DUANYAN_RMT_CH0CONF0 = BASE_CONF;
    DUANYAN_RMT_CH0CONF0 = BASE_CONF | (1u << 24);  /* CONF_UPDATE */
    DUANYAN_RMT_CH0CONF0 = BASE_CONF | (1u << 0);   /* TX_START */
    duanyan_delay_us(400);  /* 帧长 ≈44us (@11.7MHz), 400us 余量充足;
                           * 校准经 UART 锚定实测自洽, 不再向串口发字符 */
}
static void duanyan_rgb_set(int r, int g, int b) {
    if (r < 0) r = 0; if (r > 255) r = 255;
    if (g < 0) g = 0; if (g > 255) g = 255;
    if (b < 0) b = 0; if (b > 255) b = 255;
    duanyan_ws2812_send(((u32)g << 16) | ((u32)r << 8) | (u32)b);
}
/* HSV 色轮: 765 步细粒度色环 (每段 255 步), 亮度 1/4。
 * 相比 256 步旧版, 相邻帧亮度增量从 ±3 降到 ±1,
 * 消除渐变时的阶梯跳变 (肉眼误判为闪烁的主要嫌疑) */
static void duanyan_rgb_hue(int pos) {
    int r, g, b;
    pos = pos % 765;
    if (pos < 0) pos += 765;
    if (pos < 255) { r = 255 - pos; g = pos; b = 0; }
    else {
        if (pos < 510) { pos -= 255; r = 0; g = 255 - pos; b = pos; }
        else { pos -= 510; r = pos; g = 0; b = 255 - pos; }
    }
    duanyan_ws2812_send(((u32)(g >> 2) << 16) | ((u32)(r >> 2) << 8) | (u32)(b >> 2));
}
/* ADC1 初始化 (实测验证序列, 源自 yl69.c 探针)。由 boot_init 无条件调用:
 * ★实测教训: 不能用 static 标志做懒初始化——零初值 static 会进 .bss,
 * 裸机镜像若不清 .bss 则标志为随机值, init 被静默跳过 (YL-69 首烧实证) */
static void duanyan_adc_init(void) {
    DUANYAN_PERIP_CLK_EN0 |= (1u << 29);        /* APB 总线时钟 (保险) */
    DUANYAN_REG(0x600C0038) |= (1u << 29);      /* 探针 yl69.c 实测用的保险地址, 两处都写 */
    DUANYAN_SENS_CLK_GATE |= (1u << 30);        /* ★ saradc_clk_en: 不开则 done 永不置位 */
    DUANYAN_SENS_RESET |= (1u << 30);           /* saradc 复位脉冲 */
    DUANYAN_SENS_RESET &= ~(1u << 30);
    duanyan_delay_us(100);                      /* 时钟门开启后稳定窗口 (探针里由 UART 输出自然提供) */
    DUANYAN_SENS_MEAS1_CTRL1 |= (0xFFu << 24);  /* amp FSM 全部强制关 (纯数字 oneshot) */
    DUANYAN_SENS_POWER_XPD |= (3u << 29) | (1u << 31); /* force_xpd_sar=常开 + sarclk_en */
    duanyan_delay_us(500);                      /* 模拟上电稳定 */
    DUANYAN_SENS_MEAS1_MUX &= ~(1u << 31);      /* RTC 控制器路径 (非数字 DMA) */
    DUANYAN_SENS_MEAS1_CTRL2 |= (1u << 18) | (1u << 31); /* start_force + en_pad_force */
    /* APB_SARADC 数字侧默认值 (探针 yl69.c 实测序列的一部分) */
    DUANYAN_SARADC_CTRL |= (1u << 0);           /* start_force=1 */
    DUANYAN_SARADC_CTRL &= ~(1u << 1);          /* start=0 */
    DUANYAN_SARADC_CTRL2 |= (1u << 11);         /* timer_sel=1 */
    DUANYAN_SARADC_CTRL2 &= ~(1u << 24);        /* timer_en=0 (不用定时触发) */
}
/* 读 ADC1: 参数为 GPIO 号 (1~10 → ADC1_CH0~CH9), 12bit, 11dB 全量程 0~3.3V。
 * YL-69 实测: 干=4095, 湿≈3300 */
static u32 duanyan_adc_read(u32 gpio) {
    if (gpio < 1 || gpio > 10) return 0;
    u32 ch = gpio - 1;
    DUANYAN_SENS_ATTEN1 = (DUANYAN_SENS_ATTEN1 & ~(3u << (ch * 2))) | (3u << (ch * 2));
    DUANYAN_SENS_MEAS1_CTRL2 = (DUANYAN_SENS_MEAS1_CTRL2 & ~(0xFFFu << 19)) | ((1u << ch) << 19);
    u32 w = 0;
    while ((DUANYAN_SENS_SLAVE_ADDR1 >> 22) & 0xFFu) { if (++w > 100000u) break; } /* 等 FSM 空闲 */
    DUANYAN_SENS_MEAS1_CTRL2 &= ~(1u << 17);    /* start_sar 清 0 */
    duanyan_delay_us(20);
    DUANYAN_SENS_MEAS1_CTRL2 |= (1u << 17);     /* 启动转换 */
    u32 t = 0;
    while (!(DUANYAN_SENS_MEAS1_CTRL2 & (1u << 16))) {
        if (++t > 100000u) return 0;          /* 超时 */
        duanyan_delay_us(1);
    }
    return DUANYAN_SENS_MEAS1_CTRL2 & 0xFFFFu;
}
static void duanyan_boot_init(void) {
    /* 清零 .bss: 裸机无 crt0, 零初值 static 全靠手动清 (链接脚本提供符号) */
    extern char __bss_start[], __bss_end[];
    for (char *p = __bss_start; p < __bss_end; p++) *p = 0;
    /* 关三只看门狗 */
    DUANYAN_RTC_WDTWPROTECT = 0x50D83AA1;
    DUANYAN_RTC_WDTFEED = 1;
    DUANYAN_RTC_WDTCONFIG0 = 0;
    DUANYAN_RTC_WDTWPROTECT = 0;
    DUANYAN_RTC_SWD_WPROT = 0x8F1D312A;
    DUANYAN_RTC_SWD_CONF = (1u << 30);
    DUANYAN_RTC_SWD_WPROT = 0;
    DUANYAN_TIMG0_WDTWPROTECT = 0x50D83AA1;
    DUANYAN_TIMG0_WDTCONFIG0 = 0;
    DUANYAN_TIMG0_INT_CLR = 0xFFFFFFFF;
    DUANYAN_TIMG0_WDTWPROTECT = 0;
    /* TIMG0 Timer0: APB 源, 分频=1, 使能 */
    DUANYAN_TIMG0_T0CONFIG = (1u << 13) | (1u << 30) | (1u << 31);
    duanyan_calibrate();
    /* WS2812 供电使能 GPIO47 (不使能则灯不亮) */
    duanyan_pin_setup(47);
    duanyan_pin_hi(47);
    duanyan_delay_ms(10);
    /* 数据脚 GPIO48 由 RMT 外设驱动 (软件模拟在低 CPU 频率下波形畸变) */
    duanyan_rmt_init();
    /* ADC1 控制器初始化 (RTC oneshot, 不用不影响) */
    duanyan_adc_init();
    duanyan_uart_puts("[DUANYAN-OK] duanyan bare boot (RMT) tpu=");
    duanyan_uart_num(_duanyan_ticks_per_us);
    duanyan_uart_puts("\r\n");
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
    generate_c_source_with_widths(program, target, &HashMap::new())
}

/// 将 AST 生成 C 源码（带旁路宽度/定点类型表，扩展 1/3）
pub fn generate_c_source_with_widths(
    program: &Program,
    target: &CTarget,
    width_types: &HashMap<String, String>,
) -> Result<CGenerateResult, String> {
    let mut gen = CCodeGenerator::new(target.clone());
    gen.width_types = width_types.clone();
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
        }
    }

    /// 扩展 1: 变量/参数的 C 类型（按旁路宽度表；缺省 int32_t）
    fn c_width_type(&self, name: &str) -> &'static str {
        match self.width_types.get(name).map(|s| s.as_str()) {
            Some("u8") => "uint8_t",
            Some("u16") => "uint16_t",
            Some("u32") => "uint32_t",
            Some("i8") => "int8_t",
            Some("i16") => "int16_t",
            _ => "int32_t",
        }
    }

    /// 扩展 1: 宽度变量的截断转换前缀（缺省 int32_t 无需截断 → None）
    fn width_cast(&self, name: &str) -> Option<&'static str> {
        match self.width_types.get(name).map(|s| s.as_str()) {
            Some("u8") => Some("(uint8_t)"),
            Some("u16") => Some("(uint16_t)"),
            Some("u32") => Some("(uint32_t)"),
            Some("i8") => Some("(int8_t)"),
            Some("i16") => Some("(int16_t)"),
            _ => None,
        }
    }

    /// 扩展 1: 字面量越界编译期 lint（超宽度范围 → 编译错误）
    fn lint_width_literal(&self, name: &str, n: i64) -> Result<(), String> {
        let (lo, hi, tn) = match self.width_types.get(name).map(|s| s.as_str()) {
            Some("u8") => (0i128, 255i128, "u8"),
            Some("u16") => (0, 65535, "u16"),
            Some("u32") => (0, 4294967295, "u32"),
            Some("i8") => (-128, 127, "i8"),
            Some("i16") => (-32768, 32767, "i16"),
            Some("i32") => (-2147483648, 2147483647, "i32"),
            _ => return Ok(()),
        };
        let v = n as i128;
        if v < lo || v > hi {
            return Err(format!(
                "宽度越界: 变量 '{}' 类型为 {}, 字面量 {} 超出范围 [{}, {}]",
                name, tn, n, lo, hi
            ));
        }
        Ok(())
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

    /// 生成硬件 API 辅助函数
    fn generate_hw_helpers(&mut self) {
        match self.target {
            CTarget::Esp32S3Bare => {
                // 裸机 HAL 已在头部整体注入, 无需额外 helper
                // v1.6 P3: --target duanyan-spine 的 Mode-B 应答端骨架 (走裸机 UART0 FIFO 直发)
                if self.hw_imports.iter().any(|s| s.starts_with("modeb:")) {
                    self.output.push_str("/* DUANYAN Mode-B spine skeleton (v1.6 P3, --target duanyan-spine) — 组帧端骨架;\n");
                    self.output.push_str("   ⚠ 只走 UART0 FIFO 直发, 不替换真机 uart_linkm6 位啮时序/应答端 */\n");
                    self.output.push_str("static inline uint8_t _duanyan_modeb_crc8(const uint8_t *d, int n) {\n");
                    self.output.push_str("    uint8_t crc = 0;\n");
                    self.output.push_str("    for (int i = 0; i < n; i++) {\n");
                    self.output.push_str("        crc ^= d[i];\n");
                    self.output.push_str("        for (int b = 0; b < 8; b++) crc = (crc & 0x80) ? (uint8_t)((crc << 1) ^ 0x07) : (uint8_t)(crc << 1);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    return crc;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_send(uint8_t cmd, const uint8_t *pl, uint8_t len) {\n");
                    self.output.push_str("    uint8_t buf[5 + 64];\n");
                    self.output.push_str("    if (len > 64) len = 64;\n");
                    self.output.push_str("    buf[0] = 0xAA; buf[1] = 0x55; buf[2] = len; buf[3] = cmd;\n");
                    self.output.push_str("    for (int i = 0; i < len; i++) buf[4 + i] = pl[i];\n");
                    self.output.push_str("    buf[4 + len] = _duanyan_modeb_crc8(&buf[2], 2 + len);\n");
                    self.output.push_str("    for (int i = 0; i < 8; i++) duanyan_uart_putc('U');   /* 位啮自校准突发 */\n");
                    self.output.push_str("    for (int i = 0; i < 5 + len; i++) duanyan_uart_putc((char)buf[i]);\n");
                    self.output.push_str("    return 5 + len;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_joint_set(uint8_t joint, uint16_t ang, uint16_t lo, uint16_t hi) {\n");
                    self.output.push_str("    uint8_t pl[7] = { joint, (uint8_t)(ang>>8), (uint8_t)(ang&0xFF), (uint8_t)(lo>>8), (uint8_t)(lo&0xFF), (uint8_t)(hi>>8), (uint8_t)(hi&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x20, pl, 7);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_home(void) {\n");
                    self.output.push_str("    uint8_t pl[13] = { 4, 0,0x03,0x84, 1,0x03,0x84, 2,0x03,0x84, 3,0x03,0x84 };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x22, pl, 13);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_sync(const uint8_t *items, uint8_t n) {\n");
                    self.output.push_str("    uint8_t pl[1 + 8*5];\n");
                    self.output.push_str("    if (n == 0 || n > 8) return 0;\n");
                    self.output.push_str("    pl[0] = n;\n");
                    self.output.push_str("    for (int i = 0; i < n*5; i++) pl[1+i] = items[i];\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x30, pl, 1 + n*5);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int _duanyan_modeb_heartbeat(uint32_t uptime) {\n");
                    self.output.push_str("    uint8_t pl[4] = { (uint8_t)(uptime&0xFF), (uint8_t)((uptime>>8)&0xFF), (uint8_t)((uptime>>16)&0xFF), (uint8_t)((uptime>>24)&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x01, pl, 4);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::Esp32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers */\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    gpio_set_direction(pin, GPIO_MODE_INPUT);\n");
                    self.output.push_str("    gpio_set_pull_mode(pin, GPIO_PULLUP_ONLY);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) { gpio_set_level(pin, 1); }\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin)  { gpio_set_level(pin, 0); }\n");
                    self.output.push_str("static inline int  duanyan_gpio_read(int pin) { return gpio_get_level(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) {\n");
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
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) {\n");
                    self.output.push_str("    uart_write_bytes(UART_NUM_0, str, strlen(str));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM (LEDC) helpers */\n");
                    self.output.push_str("static int _duanyan_pwm_channel = 0;\n");
                    self.output.push_str("static inline int duanyan_pwm_output(int pin, int freq) {\n");
                    self.output.push_str("    int ch = _duanyan_pwm_channel++;\n");
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
                    self.output.push_str("static inline void duanyan_pwm_duty(int channel, int duty) {\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, channel, duty);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers */\n");
                    self.output.push_str("static inline void duanyan_adc_init(int channel) {\n");
                    self.output.push_str("    adc1_config_width(ADC_WIDTH_BIT_12);\n");
                    self.output.push_str("    adc1_config_channel_atten(channel, ADC_ATTEN_DB_11);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) {\n");
                    self.output.push_str("    return adc1_get_raw(channel);\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_wifi {
                    self.output.push_str("/* DUANYAN WiFi helper */\n");
                    self.output.push_str("static void duanyan_wifi_connect(const char* ssid, const char* pass) {\n");
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
                    self.output.push_str("/* DUANYAN Chip Info helpers */\n");
                    self.output.push_str("static const char* duanyan_chip_model_name(void) {\n");
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
                    self.output.push_str("static int duanyan_chip_cores(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    return info.cores;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_revision(void) {\n");
                    self.output.push_str("    esp_chip_info_t info;\n");
                    self.output.push_str("    esp_chip_info(&info);\n");
                    self.output.push_str("    return info.revision;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_flash_size(void) {\n");
                    self.output.push_str("    uint32_t size = 0;\n");
                    self.output.push_str("    esp_flash_get_size(NULL, &size);\n");
                    self.output.push_str("    return (int)(size / (1024 * 1024));\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_free_heap(void) {\n");
                    self.output.push_str("    return (int)esp_get_free_heap_size();\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static int duanyan_chip_cpu_freq(void) {\n");
                    self.output.push_str("    return CONFIG_ESP_DEFAULT_CPU_FREQ_MHZ;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static uint32_t duanyan_chip_id(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    return ((uint32_t)mac[2] << 24) | ((uint32_t)mac[3] << 16) | ((uint32_t)mac[4] << 8) | mac[5];\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static const char* duanyan_chip_sdk_version(void) {\n");
                    self.output.push_str("    return esp_get_idf_version();\n");
                    self.output.push_str("}\n");
                    // 设备唯一 ID: MAC → SHA-256 → "DUANYAN-XXXX-XXXX-XXXX"
                    // 缓冲 28 = 前缀 12 + 12 hex + 2 连字符 + NUL（旧 SNAR- 时代为 20）
                    self.output.push_str("static char _duanyan_device_id[28];\n");
                    self.output.push_str("static const char* duanyan_chip_device_id(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    uint8_t hash[32];\n");
                    self.output.push_str("    mbedtls_sha256(mac, 6, hash, 0);\n");
                    self.output.push_str("    snprintf(_duanyan_device_id, sizeof(_duanyan_device_id),\n");
                    self.output.push_str("        \"DUANYAN-%02X%02X-%02X%02X-%02X%02X\",\n");
                    self.output.push_str("        hash[0], hash[1], hash[2], hash[3], hash[4], hash[5]);\n");
                    self.output.push_str("    return _duanyan_device_id;\n");
                    self.output.push_str("}\n");
                    // MAC 地址字符串
                    self.output.push_str("static char _duanyan_mac_str[18];\n");
                    self.output.push_str("static const char* duanyan_chip_mac(void) {\n");
                    self.output.push_str("    uint8_t mac[6] = {0};\n");
                    self.output.push_str("    esp_efuse_mac_get_default(mac);\n");
                    self.output.push_str("    snprintf(_duanyan_mac_str, sizeof(_duanyan_mac_str),\n");
                    self.output.push_str("        \"%02X:%02X:%02X:%02X:%02X:%02X\",\n");
                    self.output.push_str("        mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);\n");
                    self.output.push_str("    return _duanyan_mac_str;\n");
                    self.output.push_str("}\n\n");
                }
                if self.uses_ai {
                    self.output.push_str("/* DUANYAN 锻言Mini AI helpers */\n");
                    self.output.push_str("static duanyan_brain_t _duanyan_brain;\n");
                    self.output.push_str("static inline int duanyan_brain_init(const char* model_path) {\n");
                    self.output.push_str("    return duanyan_ai_load_model(&_duanyan_brain, model_path);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_brain_classify(float* input, int input_len,\n");
                    self.output.push_str("                                       const char** labels, int n_labels) {\n");
                    self.output.push_str("    return duanyan_ai_classify(&_duanyan_brain, input, input_len, labels, n_labels);\n");
                    self.output.push_str("}\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (ESP32 LEDC)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (ESP32 LEDC 50Hz PWM) */\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 16\n");
                    self.output.push_str("static int _duanyan_servo_channel = 0;\n");
                    self.output.push_str("static int _duanyan_servo_min[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_max[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_pin[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int ch = _duanyan_servo_channel++;\n");
                    self.output.push_str("    _duanyan_servo_min[ch] = min_us;\n");
                    self.output.push_str("    _duanyan_servo_max[ch] = max_us;\n");
                    self.output.push_str("    _duanyan_servo_pin[ch] = pin;\n");
                    self.output.push_str("    _duanyan_servo_count = ch + 1;\n");
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
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    int ch = -1;\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pin[i] == pin) { ch = i; break; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (ch < 0) return;\n");
                    self.output.push_str("    if (angle < 0) angle = 0;\n");
                    self.output.push_str("    if (angle > 180) angle = 180;\n");
                    self.output.push_str("    int pulse_us = _duanyan_servo_min[ch] + (angle * (_duanyan_servo_max[ch] - _duanyan_servo_min[ch])) / 180;\n");
                    self.output.push_str("    /* 16-bit duty: 65535 = 20000us period, pulse_us maps to duty */\n");
                    self.output.push_str("    uint32_t duty = (uint32_t)((uint64_t)pulse_us * 65535 / 20000);\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, ch, duty);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, ch);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (ESP32 LEDC + GPIO) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_ch[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    gpio_set_direction(in1, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("    gpio_set_direction(in2, GPIO_MODE_OUTPUT);\n");
                    self.output.push_str("    int ch = _duanyan_servo_channel++;\n");
                    self.output.push_str("    _duanyan_motor_ch[id] = ch;\n");
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
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in1[id], 1);\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in2[id], 0);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in1[id], 0);\n");
                    self.output.push_str("        gpio_set_level(_duanyan_motor_in2[id], 1);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id], speed);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id]);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    gpio_set_level(_duanyan_motor_in1[id], 0);\n");
                    self.output.push_str("    gpio_set_level(_duanyan_motor_in2[id], 0);\n");
                    self.output.push_str("    ledc_set_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id], 0);\n");
                    self.output.push_str("    ledc_update_duty(LEDC_LOW_SPEED_MODE, _duanyan_motor_ch[id]);\n");
                    self.output.push_str("}\n\n");
                }
                // v1.6 P0: duanyan:servo 关节层 helpers (PCA9685 I²C + ZX361 UART)
                // 语义对齐 DUANYAN joint_api: deg 先钳包络→线性映射 500..2500→按后端路由
                // (Esp32S3Bare 寄存器级目标的关节驱动走 mode-B 帧, 属 P3 duanyan-spine)
                if self.hw_imports.iter().any(|s| s.starts_with("servo:") || s.starts_with("robot:")) {
                    self.output.push_str("/* DUANYAN Joint/Servo helpers (v1.6 P0: PCA9685 I2C + ZX361 UART bus) */\n");
                    self.output.push_str("#define DUANYAN_JOINT_MAX 16\n");
                    self.output.push_str("#define DUANYAN_SERVO_US_MIN 500\n");
                    self.output.push_str("#define DUANYAN_SERVO_US_MAX 2500\n");
                    self.output.push_str("#ifndef DUANYAN_PCA9685_ADDR\n#define DUANYAN_PCA9685_ADDR 0x40\n#endif\n");
                    self.output.push_str("#ifndef DUANYAN_ZX361_UART\n#define DUANYAN_ZX361_UART UART_NUM_1\n#endif\n\n");
                    self.output.push_str("/* 关节绑定表: 后端(0=pca9685,1=zx361) + 通道/ID + 包络 */\n");
                    self.output.push_str("static uint8_t _hj_backend[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _hj_chan_id[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static float   _hj_min[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static float   _hj_max[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static float   _hj_home[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _hj_bound[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _duanyan_pca_addr = DUANYAN_PCA9685_ADDR;\n\n");
                    self.output.push_str("/* --- PCA9685 (16-ch PWM over I2C) --- */\n");
                    self.output.push_str("static void duanyan_pca9685_write_reg(uint8_t reg, uint8_t val) {\n");
                    self.output.push_str("    uint8_t buf[2] = { reg, val };\n");
                    self.output.push_str("    i2c_master_write_to_device(I2C_NUM_0, _duanyan_pca_addr, buf, sizeof(buf), 100 / portTICK_PERIOD_MS);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_pca9685_init(uint8_t addr) {\n");
                    self.output.push_str("    _duanyan_pca_addr = addr;\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0x00, 0x10);   /* MODE1: SLEEP */\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0xFE, 121);    /* PRE_SCALE: 25MHz/4096/50Hz-1 */\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0x00, 0x80);   /* MODE1: RESTART, wake */\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(0x00, 0xA0);   /* MODE1: auto-increment */\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_pca9685_write_us(uint8_t channel, uint32_t us) {\n");
                    self.output.push_str("    if (us > DUANYAN_SERVO_US_MAX) us = DUANYAN_SERVO_US_MAX;\n");
                    self.output.push_str("    uint16_t off = (uint16_t)(us * 4096 / 20000);  /* 50Hz: 20000us period, 12-bit */\n");
                    self.output.push_str("    uint8_t base = 0x06 + 4 * channel;\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 0, 0);\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 1, 0);\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 2, (uint8_t)(off & 0xFF));\n");
                    self.output.push_str("    duanyan_pca9685_write_reg(base + 3, (uint8_t)((off >> 8) & 0x0F));\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("/* --- ZX361 bus servo (ASCII over UART) --- */\n");
                    self.output.push_str("static inline void duanyan_zx361_move(uint8_t id, uint16_t pwm, uint16_t ms) {\n");
                    self.output.push_str("    if (pwm < DUANYAN_SERVO_US_MIN) pwm = DUANYAN_SERVO_US_MIN;\n");
                    self.output.push_str("    if (pwm > DUANYAN_SERVO_US_MAX) pwm = DUANYAN_SERVO_US_MAX;\n");
                    self.output.push_str("    char buf[20];\n");
                    self.output.push_str("    int n = snprintf(buf, sizeof(buf), \"#%03uP%04uT%04u!\", id, pwm, ms);\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_ZX361_UART, buf, n);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_zx361_read_pos(uint8_t id) {\n");
                    self.output.push_str("    char buf[12];\n");
                    self.output.push_str("    int n = snprintf(buf, sizeof(buf), \"#%03uPRAD!\", id);\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_ZX361_UART, buf, n);\n");
                    self.output.push_str("    uint8_t rx[16] = {0};\n");
                    self.output.push_str("    int len = uart_read_bytes(DUANYAN_ZX361_UART, rx, sizeof(rx) - 1, 50 / portTICK_PERIOD_MS);\n");
                    self.output.push_str("    if (len < 10) return -1;\n");
                    self.output.push_str("    rx[len] = 0;\n");
                    self.output.push_str("    return atoi((char*)&rx[5]);   /* #IDP<pwm>! */\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_zx361_read_temp(uint8_t id) {\n");
                    self.output.push_str("    char buf[12];\n");
                    self.output.push_str("    int n = snprintf(buf, sizeof(buf), \"#%03uPRTE!\", id);\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_ZX361_UART, buf, n);\n");
                    self.output.push_str("    uint8_t rx[24] = {0};\n");
                    self.output.push_str("    int len = uart_read_bytes(DUANYAN_ZX361_UART, rx, sizeof(rx) - 1, 50 / portTICK_PERIOD_MS);\n");
                    self.output.push_str("    if (len < 15) return -1;\n");
                    self.output.push_str("    rx[len] = 0;\n");
                    self.output.push_str("    char ads[5] = { (char)rx[5], (char)rx[6], (char)rx[7], (char)rx[8], 0 };\n");
                    self.output.push_str("    float r = 10.0f * (float)atoi(ads) / 2048.0f;\n");
                    self.output.push_str("    return (int)(0.633916f * r * r - 13.2675f * r + 94.28351f);  /* 手册 V3.1 温度公式 */\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("/* --- Joint layer: bind + envelope-clamped write + read + home --- */\n");
                    self.output.push_str("static inline int duanyan_joint_bind(uint8_t joint, uint8_t backend, uint8_t chan_id,\n");
                    self.output.push_str("                                   float min_deg, float max_deg, float home_deg) {\n");
                    self.output.push_str("    if (joint >= DUANYAN_JOINT_MAX || min_deg >= max_deg) return 0;  /* 越界/倒置包络拒绑 */\n");
                    self.output.push_str("    _hj_backend[joint] = backend;\n");
                    self.output.push_str("    _hj_chan_id[joint] = chan_id;\n");
                    self.output.push_str("    _hj_min[joint] = min_deg;\n");
                    self.output.push_str("    _hj_max[joint] = max_deg;\n");
                    self.output.push_str("    _hj_home[joint] = home_deg;\n");
                    self.output.push_str("    _hj_bound[joint] = 1;\n");
                    self.output.push_str("    return 1;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_joint_write(uint8_t joint, float deg, uint16_t ms) {\n");
                    self.output.push_str("    if (joint >= DUANYAN_JOINT_MAX || !_hj_bound[joint]) return;  /* 未绑定跳过 */\n");
                    self.output.push_str("    if (deg < _hj_min[joint]) deg = _hj_min[joint];   /* 运行期包络钳位 */\n");
                    self.output.push_str("    if (deg > _hj_max[joint]) deg = _hj_max[joint];\n");
                    self.output.push_str("    float f = (deg - _hj_min[joint]) / (_hj_max[joint] - _hj_min[joint]);\n");
                    self.output.push_str("    uint16_t pwm = (uint16_t)(DUANYAN_SERVO_US_MIN + f * (DUANYAN_SERVO_US_MAX - DUANYAN_SERVO_US_MIN) + 0.5f);\n");
                    self.output.push_str("    if (_hj_backend[joint] == 1) duanyan_zx361_move(_hj_chan_id[joint], pwm, ms);\n");
                    self.output.push_str("    else duanyan_pca9685_write_us(_hj_chan_id[joint], pwm);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_joint_read(uint8_t joint) {\n");
                    self.output.push_str("    if (joint >= DUANYAN_JOINT_MAX || !_hj_bound[joint]) return -1;\n");
                    self.output.push_str("    if (_hj_backend[joint] == 1) return duanyan_zx361_read_pos(_hj_chan_id[joint]);\n");
                    self.output.push_str("    return -1;  /* PWM 后端无位置回读 (hw 脉冲真值走 mode-B 0x21) */\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_joint_home(void) {\n");
                    self.output.push_str("    for (uint8_t j = 0; j < DUANYAN_JOINT_MAX; j++)\n");
                    self.output.push_str("        if (_hj_bound[j]) duanyan_joint_write(j, _hj_home[j], 500);\n");
                    self.output.push_str("}\n\n");
                    // v1.6 P1: 执行器安全护栏 (对齐 DUANYAN safety_policy: temp>stall>load + 迟滞)
                    self.output.push_str("/* DUANYAN actuator safety guard (v1.6 P1: load/temp/stall, 对齐 DUANYAN) */\n");
                    self.output.push_str("#ifndef DUANYAN_LOAD_MAX\n#define DUANYAN_LOAD_MAX 900\n#endif\n");
                    self.output.push_str("#ifndef DUANYAN_TEMP_MAX\n#define DUANYAN_TEMP_MAX 65.0f\n#endif\n");
                    self.output.push_str("#ifndef DUANYAN_STALL_MS\n#define DUANYAN_STALL_MS 3000\n#endif\n");
                    self.output.push_str("#define DUANYAN_NORMAL_RECOVER 3\n");
                    self.output.push_str("/* 动作码: 0=None 1=TorqueOff 2=Stop 3=AlarmLed 4=EscalateHost */\n");
                    self.output.push_str("static uint16_t _duanyan_load_max = DUANYAN_LOAD_MAX;\n");
                    self.output.push_str("static float    _duanyan_temp_max = DUANYAN_TEMP_MAX;\n");
                    self.output.push_str("static uint32_t _duanyan_stall_ms = DUANYAN_STALL_MS;\n");
                    self.output.push_str("static uint8_t _hs_faulted[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static uint8_t _hs_normal_run[DUANYAN_JOINT_MAX];\n");
                    self.output.push_str("static inline void duanyan_safety_limits(uint16_t load_max, float temp_max_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    _duanyan_load_max = load_max; _duanyan_temp_max = temp_max_c; _duanyan_stall_ms = stall_ms;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 无状态优先级判定: temp > stall > load (stall==0||0xFFFFFFFF 视为无堵转证据) */\n");
                    self.output.push_str("static inline int _duanyan_safety_eval(uint16_t load, float temp_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    if (temp_c > _duanyan_temp_max) return 1;\n");
                    self.output.push_str("    if (stall_ms != 0 && stall_ms != 0xFFFFFFFFu && stall_ms > _duanyan_stall_ms) return 2;\n");
                    self.output.push_str("    if (load > _duanyan_load_max) return 2;\n");
                    self.output.push_str("    return 0;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 无状态一次性判定 (对应 Safety.check) */\n");
                    self.output.push_str("static inline int _duanyan_safety_check(uint16_t load, float temp_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    return _duanyan_safety_eval(load, temp_c, stall_ms);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 带迟滞的按 id 状态化判定: 恢复需连续 DUANYAN_NORMAL_RECOVER 次正常 */\n");
                    self.output.push_str("static inline int _duanyan_safety_check_id(uint8_t id, uint16_t load, float temp_c, uint32_t stall_ms) {\n");
                    self.output.push_str("    int act = _duanyan_safety_eval(load, temp_c, stall_ms);\n");
                    self.output.push_str("    if (id >= DUANYAN_JOINT_MAX) return act;\n");
                    self.output.push_str("    if (act == 0) {\n");
                    self.output.push_str("        if (_hs_faulted[id]) {\n");
                    self.output.push_str("            if (_hs_normal_run[id] < 255) _hs_normal_run[id]++;\n");
                    self.output.push_str("            if (_hs_normal_run[id] >= DUANYAN_NORMAL_RECOVER) { _hs_faulted[id] = 0; _hs_normal_run[id] = 0; return 0; }\n");
                    self.output.push_str("            return 4;\n");
                    self.output.push_str("        }\n");
                    self.output.push_str("        _hs_normal_run[id] = 0; return 0;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    _hs_faulted[id] = 1; _hs_normal_run[id] = 0; return act;\n");
                    self.output.push_str("}\n\n");
                }
                // v1.6 P2: duanyan:robot 轨迹数学 (下沉自 robotics.rs 纯数学, 与 robot_motion.rs 一致)
                if self.hw_imports.iter().any(|s| s.starts_with("robot:")) {
                    self.output.push_str("/* DUANYAN robot motion helpers (v1.6 P2: quintic/trapezoid + joint glide) */\n");
                    self.output.push_str("typedef struct { float c[6]; float T; } _duanyan_quintic_t;\n");
                    self.output.push_str("typedef struct { float pos; float vel; float acc; } _duanyan_traj_point_t;\n");
                    self.output.push_str("static inline _duanyan_quintic_t _duanyan_quintic_plan(float q0, float qf, float T) {\n");
                    self.output.push_str("    _duanyan_quintic_t h; float T2 = T*T, T3 = T2*T, T4 = T3*T, T5 = T4*T; float dq = qf - q0;\n");
                    self.output.push_str("    h.c[0] = q0; h.c[1] = 0.0f; h.c[2] = 0.0f;\n");
                    self.output.push_str("    h.c[3] = 10.0f*dq/T3; h.c[4] = -15.0f*dq/T4; h.c[5] = 6.0f*dq/T5; h.T = T;\n");
                    self.output.push_str("    return h;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline _duanyan_traj_point_t _duanyan_quintic_sample(_duanyan_quintic_t h, float t) {\n");
                    self.output.push_str("    _duanyan_traj_point_t p; float t2=t*t, t3=t2*t, t4=t3*t, t5=t4*t;\n");
                    self.output.push_str("    p.pos = h.c[0]+h.c[1]*t+h.c[2]*t2+h.c[3]*t3+h.c[4]*t4+h.c[5]*t5;\n");
                    self.output.push_str("    p.vel = h.c[1]+2*h.c[2]*t+3*h.c[3]*t2+4*h.c[4]*t3+5*h.c[5]*t4;\n");
                    self.output.push_str("    p.acc = 2*h.c[2]+6*h.c[3]*t+12*h.c[4]*t2+20*h.c[5]*t3;\n");
                    self.output.push_str("    return p;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("typedef struct { float t_accel, t_const, t_decel, v_max, accel, distance; } _duanyan_trapezoid_t;\n");
                    self.output.push_str("static inline _duanyan_trapezoid_t _duanyan_trapezoid_plan(float distance, float v_max, float a_max) {\n");
                    self.output.push_str("    _duanyan_trapezoid_t p; float t_accel = v_max/a_max; float d_accel = 0.5f*a_max*t_accel*t_accel;\n");
                    self.output.push_str("    if (2.0f*d_accel >= distance) {\n");
                    self.output.push_str("        float t = sqrtf(distance/a_max);\n");
                    self.output.push_str("        p.t_accel=t; p.t_const=0.0f; p.t_decel=t; p.v_max=a_max*t; p.accel=a_max; p.distance=distance;\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        float d_const = distance - 2.0f*d_accel;\n");
                    self.output.push_str("        p.t_accel=t_accel; p.t_const=d_const/v_max; p.t_decel=t_accel; p.v_max=v_max; p.accel=a_max; p.distance=distance;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    return p;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 五次采样 steps 点平滑移动关节 idx: from→to, 总时长 T 秒 (呼应真机 0x23 track glide) */\n");
                    self.output.push_str("static inline void _duanyan_joint_glide(uint8_t idx, float from, float to, float T, int steps) {\n");
                    self.output.push_str("    if (steps < 2) steps = 2;\n");
                    self.output.push_str("    _duanyan_quintic_t h = _duanyan_quintic_plan(from, to, T);\n");
                    self.output.push_str("    uint32_t dt_ms = (uint32_t)(T * 1000.0f / (steps - 1));\n");
                    self.output.push_str("    for (int i = 0; i < steps; i++) {\n");
                    self.output.push_str("        float t = T * (float)i / (float)(steps - 1);\n");
                    self.output.push_str("        _duanyan_traj_point_t p = _duanyan_quintic_sample(h, t);\n");
                    self.output.push_str("        duanyan_joint_write(idx, p.pos, (uint16_t)dt_ms);\n");
                    self.output.push_str("        vTaskDelay(pdMS_TO_TICKS(dt_ms));\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                // v1.6 P3: duanyan:modeb Mode-B 线协议直发 (镜像 DuanYanX mode_b_proto, 帧 AA 55 LEN CMD PL CRC8)
                if self.hw_imports.iter().any(|s| s.starts_with("modeb:")) {
                    self.output.push_str("/* DUANYAN Mode-B wire protocol (v1.6 P3: 镜像 DuanYanX mode_b_proto; 骨架不替换真机 uart_linkm6 位啮时序) */\n");
                    self.output.push_str("#ifndef DUANYAN_MODEB_UART\n#define DUANYAN_MODEB_UART UART_NUM_0\n#endif\n");
                    self.output.push_str("#define DUANYAN_MODEB_HEAD0 0xAA\n#define DUANYAN_MODEB_HEAD1 0x55\n#define DUANYAN_MODEB_MAXPL 64\n");
                    self.output.push_str("/* CRC8 poly 0x07 init 0 — 与真机固件 crc8() 逐位等价 */\n");
                    self.output.push_str("static inline uint8_t _duanyan_modeb_crc8(const uint8_t *d, int n) {\n");
                    self.output.push_str("    uint8_t crc = 0;\n");
                    self.output.push_str("    for (int i = 0; i < n; i++) {\n");
                    self.output.push_str("        crc ^= d[i];\n");
                    self.output.push_str("        for (int b = 0; b < 8; b++) crc = (crc & 0x80) ? (uint8_t)((crc << 1) ^ 0x07) : (uint8_t)(crc << 1);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    return crc;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 组帧 + 8×'U' 校准突发 + UART 写: 返回帧总字节数 (5+len, 不含前导) */\n");
                    self.output.push_str("static inline int _duanyan_modeb_send(uint8_t cmd, const uint8_t *pl, uint8_t len) {\n");
                    self.output.push_str("    uint8_t buf[5 + DUANYAN_MODEB_MAXPL];\n");
                    self.output.push_str("    if (len > DUANYAN_MODEB_MAXPL) len = DUANYAN_MODEB_MAXPL;\n");
                    self.output.push_str("    buf[0] = DUANYAN_MODEB_HEAD0; buf[1] = DUANYAN_MODEB_HEAD1; buf[2] = len; buf[3] = cmd;\n");
                    self.output.push_str("    for (int i = 0; i < len; i++) buf[4 + i] = pl[i];\n");
                    self.output.push_str("    buf[4 + len] = _duanyan_modeb_crc8(&buf[2], 2 + len);   /* CRC 覆盖 LEN,CMD,PAYLOAD */\n");
                    self.output.push_str("    for (int i = 0; i < 8; i++) uart_write_bytes(DUANYAN_MODEB_UART, \"U\", 1);  /* 位啮自校准突发 */\n");
                    self.output.push_str("    uart_write_bytes(DUANYAN_MODEB_UART, (const char*)buf, 5 + len);\n");
                    self.output.push_str("    return 5 + len;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x20 关节设置: payload [joint][ang_hi][ang_lo][lo_hi][lo_lo][hi_hi][hi_lo] */\n");
                    self.output.push_str("static inline int _duanyan_modeb_joint_set(uint8_t joint, uint16_t ang, uint16_t lo, uint16_t hi) {\n");
                    self.output.push_str("    uint8_t pl[7] = { joint, (uint8_t)(ang>>8), (uint8_t)(ang&0xFF), (uint8_t)(lo>>8), (uint8_t)(lo&0xFF), (uint8_t)(hi>>8), (uint8_t)(hi&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x20, pl, 7);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x22 四关节归中: ARM_HOME_ANGLE=900(0x0384) — 黄金帧字节固定 */\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_home(void) {\n");
                    self.output.push_str("    uint8_t pl[13] = { 4, 0,0x03,0x84, 1,0x03,0x84, 2,0x03,0x84, 3,0x03,0x84 };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x22, pl, 13);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x30 臂总线同步写: items = n×[id,pw_hi,pw_lo,t_hi,t_lo] 已排布, n≤8 */\n");
                    self.output.push_str("static inline int _duanyan_modeb_arm_sync(const uint8_t *items, uint8_t n) {\n");
                    self.output.push_str("    uint8_t pl[1 + 8*5];\n");
                    self.output.push_str("    if (n == 0 || n > 8) return 0;\n");
                    self.output.push_str("    pl[0] = n;\n");
                    self.output.push_str("    for (int i = 0; i < n*5; i++) pl[1+i] = items[i];\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x30, pl, 1 + n*5);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("/* 0x01 心跳: payload = uptime 秒 LE 4 字节 */\n");
                    self.output.push_str("static inline int _duanyan_modeb_heartbeat(uint32_t uptime) {\n");
                    self.output.push_str("    uint8_t pl[4] = { (uint8_t)(uptime&0xFF), (uint8_t)((uptime>>8)&0xFF), (uint8_t)((uptime>>16)&0xFF), (uint8_t)((uptime>>24)&0xFF) };\n");
                    self.output.push_str("    return _duanyan_modeb_send(0x01, pl, 4);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::ArduinoAvr => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (Arduino AVR) */\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  duanyan_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (Arduino) */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (Arduino) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM helpers (Arduino AVR, 8-bit duty 0-255) */\n");
                    self.output.push_str("static inline void duanyan_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers (Arduino AVR, 10-bit) */\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (Arduino AVR)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (Arduino Servo.h) */\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 12\n");
                    self.output.push_str("static Servo _duanyan_servos[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_pins[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _duanyan_servo_count++;\n");
                    self.output.push_str("    _duanyan_servo_pins[id] = pin;\n");
                    self.output.push_str("    _duanyan_servos[id].attach(pin, min_us, max_us);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pins[i] == pin) { _duanyan_servos[i].write(angle); return; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (Arduino AVR) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_ena[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    _duanyan_motor_ena[id] = ena;\n");
                    self.output.push_str("    pinMode(in1, OUTPUT);\n");
                    self.output.push_str("    pinMode(in2, OUTPUT);\n");
                    self.output.push_str("    pinMode(ena, OUTPUT);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], HIGH);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], HIGH);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], speed);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], 0);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::ArduinoArm => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) { pinMode(pin, INPUT_PULLUP); }\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) { digitalWrite(pin, HIGH); }\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin)  { digitalWrite(pin, LOW); }\n");
                    self.output.push_str("static inline int  duanyan_gpio_read(int pin) { return digitalRead(pin); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) { Serial.begin(baud); }\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) { Serial.print(str); }\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (Arduino ARM) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { delay(ms); }\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM helpers (Arduino ARM, 12-bit duty 0-4095) */\n");
                    self.output.push_str("static inline void duanyan_pwm_output(int pin) { pinMode(pin, OUTPUT); }\n");
                    self.output.push_str("static inline void duanyan_pwm_duty(int pin, int duty) { analogWrite(pin, duty); }\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers (Arduino ARM, 12-bit) */\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) { return analogRead(channel); }\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (Arduino ARM)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (Arduino ARM Servo.h) */\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 12\n");
                    self.output.push_str("static Servo _duanyan_servos[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_pins[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _duanyan_servo_count++;\n");
                    self.output.push_str("    _duanyan_servo_pins[id] = pin;\n");
                    self.output.push_str("    _duanyan_servos[id].attach(pin, min_us, max_us);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pins[i] == pin) { _duanyan_servos[i].write(angle); return; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (Arduino ARM) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_ena[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    _duanyan_motor_ena[id] = ena;\n");
                    self.output.push_str("    pinMode(in1, OUTPUT);\n");
                    self.output.push_str("    pinMode(in2, OUTPUT);\n");
                    self.output.push_str("    pinMode(ena, OUTPUT);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], HIGH);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("        digitalWrite(_duanyan_motor_in2[id], HIGH);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (speed > 255) speed = 255;\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], speed);\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in1[id], LOW);\n");
                    self.output.push_str("    digitalWrite(_duanyan_motor_in2[id], LOW);\n");
                    self.output.push_str("    analogWrite(_duanyan_motor_ena[id], 0);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::Stm32 => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (STM32 HAL) */\n");
                    self.output.push_str("static GPIO_TypeDef* duanyan_gpio_port(int pin) {\n");
                    self.output.push_str("    if (pin >= 16) return GPIOB;\n");
                    self.output.push_str("    return GPIOA;\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static uint16_t duanyan_gpio_pin_mask(int pin) {\n");
                    self.output.push_str("    return (uint16_t)(1 << (pin & 0xF));\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = duanyan_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_OUTPUT_PP;\n");
                    self.output.push_str("    gpio.Pull = GPIO_NOPULL;\n");
                    self.output.push_str("    gpio.Speed = GPIO_SPEED_FREQ_LOW;\n");
                    self.output.push_str("    HAL_GPIO_Init(duanyan_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_input_pullup(int pin) {\n");
                    self.output.push_str("    GPIO_InitTypeDef gpio = {0};\n");
                    self.output.push_str("    gpio.Pin = duanyan_gpio_pin_mask(pin);\n");
                    self.output.push_str("    gpio.Mode = GPIO_MODE_INPUT;\n");
                    self.output.push_str("    gpio.Pull = GPIO_PULLUP;\n");
                    self.output.push_str("    HAL_GPIO_Init(duanyan_gpio_port(pin), &gpio);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(duanyan_gpio_port(pin), duanyan_gpio_pin_mask(pin), GPIO_PIN_SET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin) {\n");
                    self.output.push_str("    HAL_GPIO_WritePin(duanyan_gpio_port(pin), duanyan_gpio_pin_mask(pin), GPIO_PIN_RESET);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_gpio_read(int pin) {\n");
                    self.output.push_str("    return HAL_GPIO_ReadPin(duanyan_gpio_port(pin), duanyan_gpio_pin_mask(pin));\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (STM32 HAL) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { HAL_Delay(ms); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (STM32 HAL) */\n");
                    self.output.push_str("static UART_HandleTypeDef _duanyan_huart;\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) {\n");
                    self.output.push_str("    _duanyan_huart.Instance = USART1;\n");
                    self.output.push_str("    _duanyan_huart.Init.BaudRate = baud;\n");
                    self.output.push_str("    _duanyan_huart.Init.WordLength = UART_WORDLENGTH_8B;\n");
                    self.output.push_str("    _duanyan_huart.Init.StopBits = UART_STOPBITS_1;\n");
                    self.output.push_str("    _duanyan_huart.Init.Parity = UART_PARITY_NONE;\n");
                    self.output.push_str("    _duanyan_huart.Init.Mode = UART_MODE_TX_RX;\n");
                    self.output.push_str("    HAL_UART_Init(&_duanyan_huart);\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) {\n");
                    self.output.push_str("    HAL_UART_Transmit(&_duanyan_huart, (uint8_t*)str, strlen(str), HAL_MAX_DELAY);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("PWM") {
                    self.output.push_str("/* DUANYAN PWM helpers (STM32 TIM, 16-bit duty) */\n");
                    self.output.push_str("static inline void duanyan_pwm_duty(int pin, int duty) {\n");
                    self.output.push_str("    /* User must configure TIM per pin; this is a placeholder */\n");
                    self.output.push_str("    (void)pin; (void)duty;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("ADC") {
                    self.output.push_str("/* DUANYAN ADC helpers (STM32 HAL) */\n");
                    self.output.push_str("static ADC_HandleTypeDef _duanyan_hadc;\n");
                    self.output.push_str("static inline int duanyan_adc_read(int channel) {\n");
                    self.output.push_str("    ADC_ChannelConfTypeDef sConfig = {0};\n");
                    self.output.push_str("    sConfig.Channel = channel;\n");
                    self.output.push_str("    HAL_ADC_ConfigChannel(&_duanyan_hadc, &sConfig);\n");
                    self.output.push_str("    HAL_ADC_Start(&_duanyan_hadc);\n");
                    self.output.push_str("    HAL_ADC_PollForConversion(&_duanyan_hadc, 100);\n");
                    self.output.push_str("    int val = HAL_ADC_GetValue(&_duanyan_hadc);\n");
                    self.output.push_str("    HAL_ADC_Stop(&_duanyan_hadc);\n");
                    self.output.push_str("    return val;\n");
                    self.output.push_str("}\n\n");
                }
                // Phase 1.2: Servo/Motor helpers (STM32 TIM HAL)
                if self.hw_imports.contains("motor:Servo") {
                    self.output.push_str("/* DUANYAN Servo helpers (STM32 TIM2 PWM 50Hz) */\n");
                    self.output.push_str("static TIM_HandleTypeDef _duanyan_servo_htim;\n");
                    self.output.push_str("#define DUANYAN_SERVO_MAX 4\n");
                    self.output.push_str("static int _duanyan_servo_pin[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_min[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_max[DUANYAN_SERVO_MAX];\n");
                    self.output.push_str("static int _duanyan_servo_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_servo_attach(int pin, int min_us, int max_us) {\n");
                    self.output.push_str("    int id = _duanyan_servo_count++;\n");
                    self.output.push_str("    _duanyan_servo_pin[id] = pin;\n");
                    self.output.push_str("    _duanyan_servo_min[id] = min_us;\n");
                    self.output.push_str("    _duanyan_servo_max[id] = max_us;\n");
                    self.output.push_str("    /* Configure TIM2 for 50Hz PWM (20ms period) */\n");
                    self.output.push_str("    _duanyan_servo_htim.Instance = TIM2;\n");
                    self.output.push_str("    _duanyan_servo_htim.Init.Prescaler = 72 - 1; /* 72MHz/72 = 1MHz tick */\n");
                    self.output.push_str("    _duanyan_servo_htim.Init.Period = 20000 - 1; /* 1MHz/20000 = 50Hz */\n");
                    self.output.push_str("    _duanyan_servo_htim.Init.CounterMode = TIM_COUNTERMODE_UP;\n");
                    self.output.push_str("    HAL_TIM_PWM_Init(&_duanyan_servo_htim);\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_servo_write(int pin, int angle) {\n");
                    self.output.push_str("    int id = -1;\n");
                    self.output.push_str("    for (int i = 0; i < _duanyan_servo_count; i++) {\n");
                    self.output.push_str("        if (_duanyan_servo_pin[i] == pin) { id = i; break; }\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    if (id < 0) return;\n");
                    self.output.push_str("    if (angle < 0) angle = 0;\n");
                    self.output.push_str("    if (angle > 180) angle = 180;\n");
                    self.output.push_str("    int pulse_us = _duanyan_servo_min[id] + (angle * (_duanyan_servo_max[id] - _duanyan_servo_min[id])) / 180;\n");
                    self.output.push_str("    TIM_OC_InitTypeDef oc = {0};\n");
                    self.output.push_str("    oc.OCMode = TIM_OCMODE_PWM1;\n");
                    self.output.push_str("    oc.Pulse = pulse_us;\n");
                    self.output.push_str("    HAL_TIM_PWM_ConfigChannel(&_duanyan_servo_htim, &oc, TIM_CHANNEL_1);\n");
                    self.output.push_str("    HAL_TIM_PWM_Start(&_duanyan_servo_htim, TIM_CHANNEL_1);\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("motor:Motor") {
                    self.output.push_str("/* DUANYAN Motor helpers (STM32 HAL GPIO + TIM) */\n");
                    self.output.push_str("#define DUANYAN_MOTOR_MAX 4\n");
                    self.output.push_str("static int _duanyan_motor_in1[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_in2[DUANYAN_MOTOR_MAX];\n");
                    self.output.push_str("static int _duanyan_motor_count = 0;\n\n");
                    self.output.push_str("static inline int duanyan_motor_init(int in1, int in2, int ena) {\n");
                    self.output.push_str("    int id = _duanyan_motor_count++;\n");
                    self.output.push_str("    _duanyan_motor_in1[id] = in1;\n");
                    self.output.push_str("    _duanyan_motor_in2[id] = in2;\n");
                    self.output.push_str("    duanyan_gpio_output(in1);\n");
                    self.output.push_str("    duanyan_gpio_output(in2);\n");
                    self.output.push_str("    (void)ena; /* PWM config via TIM */\n");
                    self.output.push_str("    return id;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_set_speed(int id, int speed) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    if (speed >= 0) {\n");
                    self.output.push_str("        duanyan_gpio_high(_duanyan_motor_in1[id]);\n");
                    self.output.push_str("        duanyan_gpio_low(_duanyan_motor_in2[id]);\n");
                    self.output.push_str("    } else {\n");
                    self.output.push_str("        duanyan_gpio_low(_duanyan_motor_in1[id]);\n");
                    self.output.push_str("        duanyan_gpio_high(_duanyan_motor_in2[id]);\n");
                    self.output.push_str("        speed = -speed;\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("    /* TODO: set TIM duty cycle for speed */\n");
                    self.output.push_str("    (void)speed;\n");
                    self.output.push_str("}\n\n");
                    self.output.push_str("static inline void duanyan_motor_brake(int id) {\n");
                    self.output.push_str("    if (id < 0 || id >= _duanyan_motor_count) return;\n");
                    self.output.push_str("    duanyan_gpio_low(_duanyan_motor_in1[id]);\n");
                    self.output.push_str("    duanyan_gpio_low(_duanyan_motor_in2[id]);\n");
                    self.output.push_str("}\n\n");
                }
            }
            CTarget::RaspberryPi => {
                if self.hw_imports.contains("GPIO") {
                    self.output.push_str("/* DUANYAN GPIO helpers (Linux sysfs) */\n");
                    self.output.push_str("static inline void duanyan_gpio_export(int pin) {\n");
                    self.output.push_str("    char buf[64];\n");
                    self.output.push_str("    int fd = open(\"/sys/class/gpio/export\", O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) {\n");
                    self.output.push_str("        int n = snprintf(buf, sizeof(buf), \"%d\", pin);\n");
                    self.output.push_str("        write(fd, buf, n);\n");
                    self.output.push_str("        close(fd);\n");
                    self.output.push_str("    }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_output(int pin) {\n");
                    self.output.push_str("    duanyan_gpio_export(pin);\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/direction\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"out\", 3); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_high(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"1\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline void duanyan_gpio_low(int pin) {\n");
                    self.output.push_str("    char path[128];\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_WRONLY);\n");
                    self.output.push_str("    if (fd >= 0) { write(fd, \"0\", 1); close(fd); }\n");
                    self.output.push_str("}\n");
                    self.output.push_str("static inline int duanyan_gpio_read(int pin) {\n");
                    self.output.push_str("    char path[128], val;\n");
                    self.output.push_str("    snprintf(path, sizeof(path), \"/sys/class/gpio/gpio%d/value\", pin);\n");
                    self.output.push_str("    int fd = open(path, O_RDONLY);\n");
                    self.output.push_str("    if (fd >= 0) { read(fd, &val, 1); close(fd); return val - '0'; }\n");
                    self.output.push_str("    return -1;\n");
                    self.output.push_str("}\n\n");
                }
                if self.hw_imports.contains("Timer") {
                    self.output.push_str("/* DUANYAN Timer helpers (Linux usleep) */\n");
                    self.output.push_str("static inline void duanyan_timer_delay(int ms) { usleep(ms * 1000); }\n\n");
                }
                if self.hw_imports.contains("UART") {
                    self.output.push_str("/* DUANYAN UART helpers (Linux serial) */\n");
                    self.output.push_str("static inline void duanyan_uart_init(int baud) { (void)baud; /* TODO: termios */ }\n");
                    self.output.push_str("static inline void duanyan_uart_write(const char* str) { printf(\"%s\", str); }\n\n");
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
                let cty = self.c_width_type(name);
                let expr_out = match self.width_cast(name) {
                    Some(cast) => format!("{}({})", cast, c_expr),
                    None => c_expr.clone(),
                };
                Ok(format!("{}{} {} = {};\n", pad, cty, name, expr_out))
            }
            Statement::Const { name, value } => {
                let c_expr = self.generate_expr(value)?;
                if let Expr::Number(n) = value {
                    self.lint_width_literal(name, *n)?;
                }
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
#[path = "c_codegen_tests.rs"]
mod tests;
