// DUANYAN STM32 示例 — LED 闪烁 (BluePill PC13)
// 编译: duanyan build examples/stm32_blink.js --target stm32

import { GPIO, Timer } from "duanyan:hw";

let led = GPIO.output(13); // PC13 = pin 16+13 = 29 in our model

while (true) {
    led.high();
    Timer.delay(500);
    led.low();
    Timer.delay(500);
}
