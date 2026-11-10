// DUANYAN 硬件示例: LED 闪烁
// 使用 GPIO API 控制 Pin 13 (板载 LED)
// 运行: duanyan run examples/blink.js --hw

let LED = 13;
GPIO.output(LED);

let state = 0;
let count = 0;

while (count < 5) {
    if (state == 0) {
        GPIO.high(LED);
        state = 1;
    } else {
        GPIO.low(LED);
        state = 0;
    }
    count = count + 1;
}
