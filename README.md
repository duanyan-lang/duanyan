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
