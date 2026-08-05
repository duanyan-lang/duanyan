// robot_arm.js — HardyScript 机械臂抓取 Demo
// 编译: hardy build robot_arm.js --target esp32
//
// 演示: Servo 控制 + async/await + setInterval 调度
// 硬件: ESP32-S3 + 3x SG90 舵机 + 电位器 (ADC)

import { Servo } from "hardy:motor";
import { ADC, Timer } from "hardy:hw";

const GRIP = 13;
const ELBOW = 12;
const SHOULDER = 14;

// 初始化舵机 (500-2500us 脉宽)
Servo.attach(SHOULDER, 500, 2500);
Servo.attach(ELBOW, 500, 2500);
Servo.attach(GRIP, 500, 2500);

// 异步抓取-倒水序列
async function grabAndPour() {
  Servo.write(SHOULDER, 45);     // 肩部转到目标位置
  await Timer.delay(500);
  Servo.write(GRIP, 90);         // 抓住
  await Timer.delay(200);
  Servo.write(ELBOW, 120);       // 抬起
  await Timer.delay(300);
  Servo.write(SHOULDER, 135);    // 转到倒水位置
  await Timer.delay(400);
  Servo.write(GRIP, 30);         // 松开倒水
  await Timer.delay(200);
  Servo.write(ELBOW, 60);        // 复位
  await Timer.delay(300);
  Servo.write(SHOULDER, 45);     // 回到起始位置
}

// 传感器监控 (100Hz)
function readSensors() {
  let pot = ADC.read(0);
  let angle = pot / 22;  // 0-4095 → 0-180
  Servo.write(SHOULDER, angle);
}

// 调度: 每5秒执行一次抓取, 每10ms读传感器
setInterval(grabAndPour, 5000);
setInterval(readSensors, 10);
