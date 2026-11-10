// DUANYAN Raspberry Pi 示例 — LED 闪烁 (GPIO 17)
// 编译: duanyan build examples/rpi_blink.js --target rpi

import { GPIO, Timer } from "duanyan:hw";

let led = GPIO.output(17);

while (true) {
    led.high();
    Timer.delay(1000);
    led.low();
    Timer.delay(1000);
}
