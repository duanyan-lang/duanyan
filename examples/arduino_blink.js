// HardyScript Arduino 示例 — LED 闪烁
// 编译: hardy build examples/arduino_blink.js --target arduino-avr --board arduino-uno
// 对应 Arduino Uno 板载 LED (pin 13)

import { GPIO, Timer } from "hardy:hw";

let led = GPIO.output(13);

while (true) {
    led.high();
    Timer.delay(1000);
    led.low();
    Timer.delay(1000);
}
