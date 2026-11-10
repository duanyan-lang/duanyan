// examples/ai_gesture.js — 锻言Mini 手势识别示例
// 使用 duanyan:ai 模块在 ESP32 上进行端侧AI推理
// duanyan build examples/ai_gesture.js --target esp32

import { GPIO, Timer } from "duanyan:hw";
import { Brain } from "duanyan:ai";

Brain.init("/sd/duanyan/gesture-10m-int4.duanyan");

let led_r = GPIO.output(2);
let labels = ["wave", "fist", "point", "none"];

Timer.interval(200, () => {
    let input = [0, 0, 0, 0, 0, 0];
    let gesture = Brain.classify(input, labels);
    if (gesture == "wave") { led_r.high(); }
});
