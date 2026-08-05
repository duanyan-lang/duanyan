// chip_info.js — ESP32-S3 自报家门 + 设备唯一 ID
// 让芯片介绍自己的硬件配置，并生成产品级设备标识
import { Chip } from "hardy:esp32";
import { Timer } from "hardy:hw";

console.log("================================");
console.log("  ESP32-S3 Self Introduction");
console.log("  Powered by HardyScript");
console.log("================================");
console.log("");

console.log("Chip Model:");
console.log(Chip.modelName());

console.log("CPU Cores:");
console.log(Chip.cores());

console.log("Chip Revision:");
console.log(Chip.revision());

console.log("Flash Size (MB):");
console.log(Chip.flashSize());

console.log("Free Heap (bytes):");
console.log(Chip.freeHeap());

console.log("SDK Version:");
console.log(Chip.sdkVersion());

console.log("");
console.log("--- Device Identity ---");

console.log("MAC Address:");
console.log(Chip.mac());

console.log("Device ID:");
console.log(Chip.deviceId());

console.log("");
console.log("================================");
console.log("  All systems nominal. Ready!");
console.log("================================");
