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
# HardyScript - JavaScript 编译器

## 项目概述

HardyScript 是 OPENSNAR 生态系统的 JavaScript 编译器，为智能数卡提供 JS 运行时支持。

**核心特性**：
- ✅ 编译 JavaScript/TypeScript 为 **COUNPRE64 机器码**
- ✅ 完全自主指令集（独立于 RISC-V/ARM/MIPS）
- ✅ 与 SNARcore 运行时集成
- ✅ 与 tianwu AI 大模型协同
- ✅ COUNPRE64 FPGA 优化

## 目录结构

```
HardyScript/
├── src/              
│   ├── compiler/     # 编译器核心
│   ├── parser/       # JS/TS 解析器
│   ├── optimizer/    # 代码优化
│   └── backend/      # COUNPRE64 代码生成 ✨
│       └── counpre64_isa.rs  # COUNPRE64 指令集定义
├── tests/            # 测试用例
├── examples/         # 示例代码
└── docs/             # 文档
```

## 快速开始

### 编译
```bash
cd c:\OPENSNAR\HardyScript
cargo +nightly-x86_64-pc-windows-gnu build --release -Zbuild-std
```

### 运行测试
```bash
cargo test
```

## COUNPRE64 指令集

HardyScript 生成 **COUNPRE64 自主指令集**机器码：

### 特性
- **16 个通用寄存器** (x0-x15)
- **4-bit FUNC 操作码**
- **32 位定长指令**
- **64 位数据处理**

### 示例

JavaScript 代码：
```javascript
let a = 10;
let b = 20;
let c = a + b;
```

编译为 COUNPRE64 机器码：
```assembly
ADDI x1, x0, 10      # a = 10
ADDI x2, x0, 20      # b = 20
ADD  x3, x1, x2      # c = a + b
```

编码为 32 位机器码：
```
0x800A1000  # ADDI x1, x0, 10
0x80142000  # ADDI x2, x0, 20
0x00123000  # ADD x3, x1, x2
```

## 与 OPENSNAR 集成

```
OPENSNAR/
├── SNARcore/    ← 核心运行时（执行 COUNPRE64 机器码）
├── tianwu/      ← AI 大模型
└── HardyScript/      ← JS 编译器（生成 COUNPRE64 机器码）✨
```

## 编译流程

```
JavaScript/TypeScript 源码
    ↓
词法分析 (Lexer)
    ↓
语法分析 (Parser)
    ↓
抽象语法树 (AST)
    ↓
优化 (Optimizer)
    ↓
代码生成 (Code Generator)
    ↓
COUNPRE64 机器码 ✨
    ↓
在 COUNPRE64 FPGA CPU 上运行
```

## 开发路线图

### Phase 1: 基础编译器 ✅
- [x] COUNPRE64 指令集定义
- [ ] JS 词法分析器
- [ ] JS 语法分析器
- [ ] AST 生成
- [ ] 基础代码生成

### Phase 2: 优化与集成
- [ ] 死代码消除
- [ ] 内联优化
- [ ] SNARcore 集成
- [ ] tianwu API 绑定

### Phase 3: 完整功能
- [ ] TypeScript 支持
- [ ] async/await 支持
- [ ] Web API 兼容层
- [ ] 性能分析工具

## 技术栈

- **语言**: Rust
- **目标架构**: COUNPRE64 (自主 64 位指令集) ✨
- **运行时**: SNARcore
- **AI 引擎**: tianwu

## 与 RISC-V 的区别

| 特性 | COUNPRE64 | RISC-V |
|------|-----------|--------|
| 指令编码 | 4-bit FUNC | funct3 + funct7 |
| 寄存器数 | 16 个 | 32 个 |
| 设计来源 | 自主设计 | Berkeley 开源 |
| 专利风险 | 无 | 有 |

## 许可证

遵循 OPENSNAR 项目许可证

## 联系方式

- 开发者: Zhujun
- 创建日期: 2026-06-02
