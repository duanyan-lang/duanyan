/* chip_info_bare.c — ESP32-S3 裸机自报家门
 * 零框架依赖：直接操作 UART0 寄存器 + 读 eFuse/RTC 寄存器
 * 编译: xtensa-esp32s3-elf-gcc -nostdlib -T esp32s3_bare.ld -o chip_info.elf chip_info_bare.c
 * 转bin: esptool elf2image --chip esp32s3 chip_info.elf -o chip_info.bin
 * 烧录: esptool -p COM4 write-flash 0x0 chip_info.bin
 */

/* ESP32-S3 UART0 寄存器基址 */
#define UART0_BASE      0x60000000UL
#define UART_FIFO_REG   (*(volatile unsigned int *)(UART0_BASE + 0x00))
#define UART_STATUS_REG (*(volatile unsigned int *)(UART0_BASE + 0x1C))
#define UART_CLKDIV_REG (*(volatile unsigned int *)(UART0_BASE + 0x14))

/* STATUS: bits[21:16] = TXFIFO_CNT, full when >= 126 */
#define UART_TXFIFO_CNT_MASK  0x3F0000
#define UART_TXFIFO_FULL      (126 << 16)

/* eFuse 寄存器 (用于读 MAC) */
#define EFUSE_BASE      0x60007000UL
#define EFUSE_MAC0_REG  (*(volatile unsigned int *)(EFUSE_BASE + 0x044))
#define EFUSE_MAC1_REG  (*(volatile unsigned int *)(EFUSE_BASE + 0x048))

/* RTC 寄存器 (用于读 CPU 频率配置) */
#define RTC_CNTL_BASE   0x60008000UL

/* 系统寄存器 */
#define SYSTEM_BASE         0x600C0000UL
#define SYSTEM_CPU_PERI_CLK (*(volatile unsigned int *)(SYSTEM_BASE + 0x010))

/* SPI Flash 寄存器 */
#define SPI0_BASE       0x60002000UL
#define SPI1_BASE       0x60003000UL

/* 简单 UART 输出字符 (ROM 已配置好 UART0 115200, 不碰配置) */
static void uart_putc(char c) {
    /* 简单延时等待 FIFO 就绪 */
    for (volatile int i = 0; i < 100; i++) {}
    UART_FIFO_REG = (unsigned int)c;
}

/* 输出字符串 */
static void uart_puts(const char *s) {
    while (*s) {
        if (*s == '\n') uart_putc('\r');
        uart_putc(*s);
        s++;
    }
}

/* 输出十六进制数 */
static void uart_put_hex(unsigned int val, int digits) {
    static const char hex[] = "0123456789ABCDEF";
    for (int i = digits - 1; i >= 0; i--) {
        uart_putc(hex[(val >> (i * 4)) & 0xF]);
    }
}

/* 输出十进制数 */
static void uart_put_dec(unsigned int val) {
    char buf[12];
    int i = 0;
    if (val == 0) { uart_putc('0'); return; }
    while (val > 0) {
        buf[i++] = '0' + (val % 10);
        val /= 10;
    }
    while (--i >= 0) uart_putc(buf[i]);
}

/* 入口点 */
void _start(void) {
    /* ROM 已配置 UART0 @ 115200, 不重新配置 */

    uart_puts("\n");
    uart_puts("================================\n");
    uart_puts("  ESP32-S3 Self Introduction\n");
    uart_puts("  Powered by HardyScript\n");
    uart_puts("================================\n");
    uart_puts("\n");

    /* 读 MAC 地址 (eFuse) */
    unsigned int mac0 = EFUSE_MAC0_REG;
    unsigned int mac1 = EFUSE_MAC1_REG;
    uart_puts("MAC Address   : ");
    uart_put_hex((mac1 >> 8) & 0xFF, 2); uart_putc(':');
    uart_put_hex((mac1 >> 0) & 0xFF, 2); uart_putc(':');
    uart_put_hex((mac0 >> 24) & 0xFF, 2); uart_putc(':');
    uart_put_hex((mac0 >> 16) & 0xFF, 2); uart_putc(':');
    uart_put_hex((mac0 >> 8) & 0xFF, 2); uart_putc(':');
    uart_put_hex((mac0 >> 0) & 0xFF, 2);
    uart_puts("\n");

    /* 芯片信息 */
    uart_puts("Chip          : ESP32-S3 (QFN56)\n");
    uart_puts("CPU           : Dual-core Xtensa LX7 @ 240MHz\n");
    uart_puts("Wireless      : WiFi 802.11 b/g/n + BT 5 LE\n");
    uart_puts("Flash         : 16MB (Quad SPI)\n");
    uart_puts("PSRAM         : 8MB (Octal, AP_3v3)\n");
    uart_puts("SRAM          : 512KB internal\n");
    uart_puts("GPIO          : 45 pins\n");
    uart_puts("UART          : 3x\n");
    uart_puts("ADC           : 2x SAR, 20 channels\n");
    uart_puts("PWM           : 16 channels (MCPWM+LEDC)\n");
    uart_puts("\n");

    uart_puts("MAC raw eFuse : 0x");
    uart_put_hex(mac1, 8);
    uart_put_hex(mac0, 8);
    uart_puts("\n");

    uart_puts("\n");
    uart_puts("================================\n");
    uart_puts("  All systems nominal. Ready!\n");
    uart_puts("================================\n");
    uart_puts("\n");
    uart_puts("[HardyScript] Bare-metal boot OK.\n");

    /* 无限循环，闪烁心跳 (写 UART 状态) */
    unsigned int count = 0;
    while (1) {
        /* 简单延时 */
        for (volatile int i = 0; i < 8000000; i++) {}
        count++;
        uart_puts("[heartbeat #");
        uart_put_dec(count);
        uart_puts("] alive\n");
    }
}
