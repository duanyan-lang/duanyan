/* test_uart.c — 极简 UART 测试: 先等待再发 "HELLO\n" */
#define UART0_FIFO (*(volatile unsigned int *)0x60000000)

void _start(void) {
    /* 等待 ~2秒 (240MHz CPU) */
    for (volatile unsigned int i = 0; i < 480000000; i++) {}
    const char *msg = "HELLO FROM ESP32-S3!\n";
    while (*msg) {
        UART0_FIFO = (unsigned int)*msg++;
        for (volatile int i = 0; i < 2000; i++) {}
    }
    while (1) {
        UART0_FIFO = '.';
        for (volatile unsigned int i = 0; i < 240000000; i++) {}
    }
}
