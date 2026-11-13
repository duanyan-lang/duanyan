# Duanyan / 锻言

> A JavaScript compiler for hardware and edge decision engine.
> 锻言：面向硬件与端侧决策引擎的 JavaScript 编译器

[![npm](https://img.shields.io/npm/v/@duanyan-lang/duanyan)](https://www.npmjs.com/package/@duanyan-lang/duanyan)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

## 🔗 Links

- Homepage: https://duanyan-lang.com
- Documentation: https://duanyan.dev
- Community: https://duanyan.org

## ✨ What is Duanyan

Duanyan (锻言) compiles JavaScript down to runnable code for embedded hardware, FPGA and edge decision engines.
It connects JS ecosystem directly to your hardware, for robotics, IoT and local-side intelligent decision.

锻言打通 JavaScript 生态与底层硬件，面向机器人、物联网、本地端侧智能决策场景，把 JS 编译成可在硬件上运行的目标代码。

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
npm install -g @duanyan-lang/duanyan
```

Or build from source (requires [Rust](https://rustup.rs)):

```bash
git clone https://github.com/duanyan-lang/duanyan.git
cd DUANYAN
cargo install --path .
```

### Usage

```bash
# Compile JS to firmware
duanyan build blink.js --target esp32

# DuanYanX spine (脑体分离: Pi=脑, S3=脊髄) — 关节经 Mode-B 帧驱动
duanyan build spine.js --target duanyan-spine

# Flash to board
duanyan flash output.hex --port COM4

# Serial monitor
duanyan monitor --port COM4

# AI model generation (gesture recognition)
duanyan model generate --output gesture.duanyan

# Package manager
duanyan pack init
duanyan pack install math-utils
```

### Robotics Hardware Modules (v1.6)

机器人对齐升级（以 DuanYanX 真机为单一真值口径镜像/移植，带字节级一致性测试）：

| Import | API | 说明 |
|--------|-----|------|
| `duanyan:servo` | `Joint.bind/write/read/home` · `Safety.check/limits` | 关节抽象（PCA9685 通道 + ZX361 总线双后端，包络钳位 + 回读）与负载/温度/堵转安全护栏 |
| `duanyan:robot` | `Traj.quintic/sample/trapezoid` · `Joint.glide` | 五次/梯形轨迹规划 + 多点平滑 glide（呼应真机 0x23 track） |
| `duanyan:modeb` | `ModeB.send/frame/jointSet/armSync/armHome/heartbeat` | Mode-B 线协议直发（帧 `AA 55 LEN CMD PAYLOAD CRC8` + 8×'U' 位啮校准突发） |

> **Target 口径**：`duanyan:servo`/`duanyan:robot` 的 `Joint.*`/`Traj.*` helper 落在 **ESP-IDF 工程**（`--target esp32`，直驱 PCA9685 I²C + ZX361 UART）；`duanyan:modeb` 同时支持 `--target esp32` 与裸机 `--target duanyan-spine`。脊髄端（spine）不发射 ESP-IDF 关节 helper，关节一律经 Mode-B 帧下发（呼应真机 `uart_linkm6` 链路）。

```javascript
// arm.js — ESP-IDF 全栈 (--target esp32): 关节绑定 + 轨迹平滑 + Mode-B 归中
import { Joint } from "duanyan:servo";
import { Traj } from "duanyan:robot";
import { ModeB } from "duanyan:modeb";

Joint.bind(0, 0, 0, 30, 150, 90);   // J1 pan: PCA9685 CH0, 包络 [30,150], home 90
Joint.glide(0, 30, 120, 2, 20);     // 五次采样 20 点平滑移动
ModeB.armHome();                     // 0x22 四关节同帧归中 (黄金帧)
```

```javascript
// spine.js — 脊髄端骨架 (--target duanyan-spine): 关节全部走 Mode-B 帧
import { ModeB } from "duanyan:modeb";

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
duanyan build blink.js --target esp32
duanyan flash blink.hex --port COM4
```

That's it. No C. No toolchain setup. No SDK.

## Features

- **Zero learning curve** — Write standard JS/TS, no proprietary syntax
- **Not a JS VM — true AOT to machine code** — Compiles JS/TS source directly to native hardware machine code at build time; no on-device interpreter, VM, or GC. Unlike Espruino / Moddable (XS), which interpret JS on-chip, DUANYAN ships deterministic native firmware (memory & timing known at compile time)
- **Multi-platform** — One codebase, six hardware targets (incl. a custom ISA)
- **TypeScript support** — Full type annotation stripping
- **async/await** — Event-driven embedded programming
- **AI runtime** — On-device gesture recognition (Duanyan engine)
- **Package manager** — `duanyan pack` for embedded libraries
- **Serial tooling** — Built-in flasher + monitor
- **CI/CD ready** — `duanyan ci` for automated build pipelines

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
| `duanyan build <file>` | Compile JS/TS to firmware |
| `duanyan flash <hex>` | Flash firmware to board |
| `duanyan monitor` | Open serial monitor |
| `duanyan model generate` | Generate AI gesture model |
| `duanyan model convert` | Convert GGUF to Duanyan format |
| `duanyan model benchmark` | Generate ESP32 benchmark harness |
| `duanyan pack init/install/publish` | Package manager |
| `duanyan ci` | Run CI/CD pipeline |
| `duanyan lint <file>` | Static analysis |

## Building from Source

```bash
# Prerequisites: Rust nightly
rustup toolchain install nightly

# Build
cargo build --release

# Test (default: 500 lib + 485 bin + 30 exec · --features domains: 562 lib + 552 bin + 30 exec)
cargo test
cargo test --features domains

# Install locally
cargo install --path .
```

## Tech Stack

- **Compiler**: Rust
- **Custom ISA**: COUNPRE64 (16 registers, 32-bit instructions, 64-bit data)
- **AI Engine**: Duanyan (INT4 quantized, on-device inference)
- **Package Format**: `.duanyan` archives

## License

MIT © duanyan-lang

**商标声明 / Trademark Notice**：「Duanyan / 锻言」为青岛岩画智能科技有限公司注册商标。MIT 许可仅授予代码版权，不授予任何商标权。

## Links

- [npm](https://www.npmjs.com/package/@duanyan-lang/duanyan)
- [Issues](https://github.com/duanyan-lang/duanyan/issues)
- [Homepage](https://duanyan-lang.com) · [Documentation](https://duanyan.dev) · [Community](https://duanyan.org)
