//! ESP32-S3 裸机 HAL C 源码常量（任务 B1 从 c_codegen.rs 拆出，纯搬运）

/// ESP32-S3 裸机 HAL (源自 DUANYAN 实测跑通的 led_on.c / gradient.c)
/// 数据脚 GPIO48 (RGB_BUILTIN), 供电使能脚 GPIO47
pub(super) const ESP32S3_BARE_HAL: &str = r#"
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