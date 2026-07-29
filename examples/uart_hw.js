// HardyScript 硬件示例: UART 串口输出
// 运行: hardy run examples/uart_hw.js --hw

UART.init(9600);
UART.write(72);
UART.write(101);
UART.write(108);
UART.write(108);
UART.write(111);
UART.write(10);
