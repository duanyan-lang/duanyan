# HardyScript

**An AOT compiler that forges JavaScript into machine code.**

**把 JavaScript 锻造成机器码的 AOT 编译器。**

[![npm](https://img.shields.io/npm/v/hardyscript)](https://www.npmjs.com/package/hardyscript)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

---

Write standard JavaScript/TypeScript. Compile to bare-metal firmware. Flash to real hardware.

写标准 JavaScript/TypeScript，编译为裸机固件，烧录到真实硬件。

## Supported Targets

| Platform | Output | Status |
|----------|--------|--------|
| ESP32 / ESP32-S3 | ESP-IDF C project | ✅ |
| ESP32-S3 bare-metal (`--target esp32s3` / `duanyan-spine`) | Register-level C image + Mode-B spine skeleton | ✅ |
| Arduino (Uno/Nano/Mega) | Arduino `.ino` | ✅ |
| STM32 (F103) | Makefile + OpenOCD | ✅ |
| Raspberry Pi | Native C + Makefile | ✅ |
| COUNPRE64 | Custom 64-bit ISA machine code | ✅ |

## Quick Start

### Install

```bash
npm install -g hardyscript
```

Or build from source (requires [Rust](https://rustup.rs)):

```bash
git clone https://github.com/hardyscript-cpu/HardyScript.git
cd HardyScript
cargo install --path .
```

### Usage

```bash
# Compile JS to firmware
hardy build blink.js --target esp32

# DUANYAN spine (脑体分离: Pi=脑, S3=脊髄) — 关节经 Mode-B 帧驱动
hardy build spine.js --target duanyan-spine

# Flash to board
hardy flash output.hex --port COM4

# Serial monitor
hardy monitor --port COM4

# AI model generation (gesture recognition)
hardy model generate --output gesture.duanyan

# Package manager
hardy pack init
hardy pack install math-utils
```

### Robotics Hardware Modules (v1.6)

机器人对齐升级（以 DUANYAN 真机为单一真值口径镜像/移植，带字节级一致性测试）：

| Import | API | 说明 |
|--------|-----|------|
| `hardy:servo` | `Joint.bind/write/read/home` · `Safety.check/limits` | 关节抽象（PCA9685 通道 + ZX361 总线双后端，包络钳位 + 回读）与负载/温度/堵转安全护栏 |
| `hardy:robot` | `Traj.quintic/sample/trapezoid` · `Joint.glide` | 五次/梯形轨迹规划 + 多点平滑 glide（呼应真机 0x23 track） |
| `hardy:modeb` | `ModeB.send/frame/jointSet/armSync/armHome/heartbeat` | Mode-B 线协议直发（帧 `AA 55 LEN CMD PAYLOAD CRC8` + 8×'U' 位啮校准突发） |

> **Target 口径**：`hardy:servo`/`hardy:robot` 的 `Joint.*`/`Traj.*` helper 落在 **ESP-IDF 工程**（`--target esp32`，直驱 PCA9685 I²C + ZX361 UART）；`hardy:modeb` 同时支持 `--target esp32` 与裸机 `--target duanyan-spine`。脊髄端（spine）不发射 ESP-IDF 关节 helper，关节一律经 Mode-B 帧下发（呼应真机 `uart_linkm6` 链路）。

```javascript
// arm.js — ESP-IDF 全栈 (--target esp32): 关节绑定 + 轨迹平滑 + Mode-B 归中
import { Joint } from "hardy:servo";
import { Traj } from "hardy:robot";
import { ModeB } from "hardy:modeb";

Joint.bind(0, 0, 0, 30, 150, 90);   // J1 pan: PCA9685 CH0, 包络 [30,150], home 90
Joint.glide(0, 30, 120, 2, 20);     // 五次采样 20 点平滑移动
ModeB.armHome();                     // 0x22 四关节同帧归中 (黄金帧)
```

```javascript
// spine.js — 脊髄端骨架 (--target duanyan-spine): 关节全部走 Mode-B 帧
import { ModeB } from "hardy:modeb";

ModeB.armHome();                 // 0x22 四关节同帧归中
ModeB.jointSet(2, 900, 0, 0);    // 0x20 J3 → 90.0° 静态定位
ModeB.heartbeat(0);              // 0x01 心跳保活
```

## Example

```javascript
// blink.js — ESP32 LED blink
const LED = 2;

pinMode(LED, OUTPUT);

setInterval(() => {
  digitalWrite(LED, !digitalRead(LED));
}, 500);
```

```bash
hardy build blink.js --target esp32
hardy flash blink.hex --port COM4
```

That's it. No C. No toolchain setup. No SDK.

## Features

- **Zero learning curve** — Write standard JS/TS, no proprietary syntax
- **AOT compilation** — No interpreter overhead, native performance
- **Multi-platform** — One codebase, five hardware targets
- **TypeScript support** — Full type annotation stripping
- **async/await** — Event-driven embedded programming
- **AI runtime** — On-device gesture recognition (Duanyan engine)
- **Package manager** — `hardy pack` for embedded libraries
- **Serial tooling** — Built-in flasher + monitor
- **CI/CD ready** — `hardy ci` for automated build pipelines

## Architecture

```
JavaScript / TypeScript
        │
        ▼
   ┌─────────┐
   │  Lexer  │
   └────┬────┘
        ▼
   ┌─────────┐
   │ Parser  │  → AST
   └────┬────┘
        ▼
   ┌───────────┐
   │ Optimizer │  → Constant folding, DCE, inlining
   └────┬──────┘
        ▼
   ┌──────────────┐
   │ Code Generator│
   └──────┬───────┘
          ▼
  ┌───────────────────────────────┐
  │ ESP32 │ Arduino │ STM32 │ RPi │ COUNPRE64 │
  └───────────────────────────────┘
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `hardy build <file>` | Compile JS/TS to firmware |
| `hardy flash <hex>` | Flash firmware to board |
| `hardy monitor` | Open serial monitor |
| `hardy model generate` | Generate AI gesture model |
| `hardy model convert` | Convert GGUF to Duanyan format |
| `hardy model benchmark` | Generate ESP32 benchmark harness |
| `hardy pack init/install/publish` | Package manager |
| `hardy ci` | Run CI/CD pipeline |
| `hardy lint <file>` | Static analysis |

## Building from Source

```bash
# Prerequisites: Rust nightly
rustup toolchain install nightly

# Build
cargo build --release

# Test (419 tests · 481 with --features domains)
cargo test

# Install locally
cargo install --path .
```

## Tech Stack

- **Compiler**: Rust
- **Custom ISA**: COUNPRE64 (16 registers, 32-bit instructions, 64-bit data)
- **AI Engine**: Duanyan (INT4 quantized, on-device inference)
- **Package Format**: `.hardy` archives

## License

MIT © HardyScript Team

## Links

- [npm](https://www.npmjs.com/package/hardyscript)
- [Issues](https://github.com/hardyscript-cpu/HardyScript/issues)
