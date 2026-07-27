// SNARjs Phase C: ESP32 LED 闪烁示例
// 使用 snar:hw 硬件API
// 编译命令: snar build esp32_blink.js --target esp32

import { GPIO, Timer } from "snar:hw";

let led = GPIO.output(2);

while (true) {
    led.high();
    Timer.delay(500);
    led.low();
    Timer.delay(500);
}
