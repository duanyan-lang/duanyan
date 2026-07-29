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

# Flash to board
hardy flash output.hex --port COM4

# Serial monitor
hardy monitor --port COM4

# AI model generation (gesture recognition)
hardy model generate --output gesture.duanyan

# Package manager
hardy pack init
hardy pack add math-utils
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
| `hardy pack init/add/publish` | Package manager |
| `hardy ci` | Run CI/CD pipeline |
| `hardy lint <file>` | Static analysis |

## Building from Source

```bash
# Prerequisites: Rust nightly
rustup toolchain install nightly

# Build
cargo build --release

# Test (438 tests)
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
