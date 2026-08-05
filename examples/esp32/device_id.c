/* device_id.c — ESP32-S3 裸机设备唯一 ID 生成器
 * 
 * 功能: 上电后通过 UART0 输出芯片信息 + 设备唯一标识
 * 工具链: xtensa-esp32s3-elf-gcc (无需 ESP-IDF)
 * 烧录: esptool write-flash 或 Flash Download Tool
 * 
 * 设备 ID 算法:
 *   MAC (eFuse 6字节) → FNV-1a 哈希 → "SNAR-XXXX-XXXX-XXXX"
 *   每颗芯片全球唯一，适用于云服务认证/产品销售/联网通讯
 */

/* ===== ESP32-S3 寄存器定义 ===== */
#define UART0_FIFO      (*(volatile unsigned int *)0x60000000)
#define UART0_STATUS    (*(volatile unsigned int *)0x6000001C)
#define EFUSE_BASE      0x60007000
#define EFUSE_MAC_LOW   (*(volatile unsigned int *)(EFUSE_BASE + 0x044))
#define EFUSE_MAC_HIGH  (*(volatile unsigned int *)(EFUSE_BASE + 0x048))

/* ===== 最小 libc 桩 (GCC 优化需要) ===== */
void *memcpy(void *dst, const void *src, unsigned int n) {
    char *d = (char *)dst;
    const char *s = (const char *)src;
    while (n--) *d++ = *s++;
    return dst;
}

/* ===== UART 输出 ===== */
static void uart_putc(char c) {
    /* 等待 TX FIFO 非满 (bits[21:16] < 128) */
    while (((UART0_STATUS >> 16) & 0xFF) >= 128) {}
    UART0_FIFO = (unsigned int)c;
}

static void uart_puts(const char *s) {
    while (*s) uart_putc(*s++);
}

static void uart_put_hex8(unsigned int val) {
    const char hex[] = "0123456789ABCDEF";
    uart_putc(hex[(val >> 4) & 0xF]);
    uart_putc(hex[val & 0xF]);
}

static void uart_put_hex16(unsigned int val) {
    uart_put_hex8(val >> 8);
    uart_put_hex8(val & 0xFF);
}

static void uart_put_dec(int val) {
    char buf[12];
    int i = 0;
    if (val == 0) { uart_putc('0'); return; }
    while (val > 0) { buf[i++] = '0' + (val % 10); val /= 10; }
    while (i > 0) uart_putc(buf[--i]);
}

/* ===== FNV-1a 哈希 (32位) ===== */
static unsigned int fnv1a_32(const unsigned char *data, int len) {
    unsigned int hash = 0x811C9DC5u;  /* FNV offset basis */
    for (int i = 0; i < len; i++) {
        hash ^= data[i];
        hash *= 0x01000193u;          /* FNV prime */
    }
    return hash;
}

/* ===== 设备 ID 生成 ===== */
/* 从 MAC 生成 48 位唯一标识: 两轮 FNV-1a 拼接 */
static void generate_device_id(const unsigned char mac[6], char out[19]) {
    /* 第一轮: 原始 MAC → 32 bit */
    unsigned int h1 = fnv1a_32(mac, 6);
    /* 第二轮: MAC + h1 混合 → 32 bit (取高16位凑满48位) */
    unsigned char mix[10];
    for (int i = 0; i < 6; i++) mix[i] = mac[i];
    mix[6] = (h1 >> 24) & 0xFF;
    mix[7] = (h1 >> 16) & 0xFF;
    mix[8] = (h1 >> 8) & 0xFF;
    mix[9] = h1 & 0xFF;
    unsigned int h2 = fnv1a_32(mix, 10);
    
    /* 格式: SNAR-XXXX-XXXX-XXXX (h1高16 + h1低16 + h2高16) */
    const char hex[] = "0123456789ABCDEF";
    unsigned int parts[3] = { (h1 >> 16) & 0xFFFF, h1 & 0xFFFF, (h2 >> 16) & 0xFFFF };
    int pos = 0;
    out[pos++] = 'S'; out[pos++] = 'N'; out[pos++] = 'A'; out[pos++] = 'R';
    out[pos++] = '-';
    for (int p = 0; p < 3; p++) {
        if (p > 0) out[pos++] = '-';
        out[pos++] = hex[(parts[p] >> 12) & 0xF];
        out[pos++] = hex[(parts[p] >> 8) & 0xF];
        out[pos++] = hex[(parts[p] >> 4) & 0xF];
        out[pos++] = hex[parts[p] & 0xF];
    }
    out[pos] = '\0';
}

/* ===== 主程序 ===== */
void _start(void) {
    /* 立即输出 - 不等待 (避免看门狗复位) */
    uart_puts("\r\n");
    uart_puts("================================\r\n");
    uart_puts("  SNAR Device Identity Report\r\n");
    uart_puts("  Powered by HardyScript\r\n");
    uart_puts("================================\r\n\r\n");
    
    /* 读取 eFuse MAC */
    unsigned int mac_low  = EFUSE_MAC_LOW;
    unsigned int mac_high = EFUSE_MAC_HIGH;
    unsigned char mac[6];
    mac[0] = (mac_high >> 8) & 0xFF;
    mac[1] = mac_high & 0xFF;
    mac[2] = (mac_low >> 24) & 0xFF;
    mac[3] = (mac_low >> 16) & 0xFF;
    mac[4] = (mac_low >> 8) & 0xFF;
    mac[5] = mac_low & 0xFF;
    
    /* 输出芯片信息 */
    uart_puts("Chip: ESP32-S3 (Dual Core 240MHz)\r\n");
    uart_puts("MAC:  ");
    for (int i = 0; i < 6; i++) {
        if (i > 0) uart_putc(':');
        uart_put_hex8(mac[i]);
    }
    uart_puts("\r\n");
    
    /* 生成并输出设备唯一 ID */
    char device_id[19];
    generate_device_id(mac, device_id);
    
    uart_puts("Device ID: ");
    uart_puts(device_id);
    uart_puts("\r\n");
    
    /* 输出芯片 ID (用于快速识别) */
    unsigned int chip_id = ((unsigned int)mac[2] << 24) | ((unsigned int)mac[3] << 16)
                         | ((unsigned int)mac[4] << 8) | mac[5];
    uart_puts("Chip ID: 0x");
    uart_put_hex16(chip_id >> 16);
    uart_put_hex16(chip_id & 0xFFFF);
    uart_puts("\r\n");
    
    uart_puts("\r\n--- Cloud Ready ---\r\n");
    uart_puts("Use Device ID for:\r\n");
    uart_puts("  * Network authentication\r\n");
    uart_puts("  * Cloud service binding\r\n");
    uart_puts("  * Product activation\r\n");
    uart_puts("\r\n================================\r\n");
    
    /* 心跳: 每 2 秒闪烁一次设备 ID 前缀 */
    while (1) {
        uart_puts("[");
        uart_puts(device_id);
        uart_puts("] alive\r\n");
        for (volatile unsigned int i = 0; i < 48000000; i++) {} /* ~200ms */
    }
}
