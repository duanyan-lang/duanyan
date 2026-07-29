// HardyScript 硬件示例: ADC 传感器读取
// 运行: hardy run examples/sensor.js --hw

ADC.channel(0);
let value = ADC.read(0);
let temp = value / 4;
UART.init(9600);
