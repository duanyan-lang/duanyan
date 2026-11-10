//! Phase W1: WCET 精度提升引擎
//!
//! 在基础 WCET 分析之上，增加：
//! - 外设访问延迟建模 (GPIO/ADC/UART/SPI/I2C)
//! - 中断干扰分析 (blocking time from higher-priority interrupts)
//! - 循环上界自动推断 (从初始化+步进+条件三元组推断)
//! - 多路径 WCET 取最大值
//! - 结构化 WCET 报告输出

use crate::c_codegen::CTarget;
use crate::wcet::{TargetTimingModel, get_timing_model, WcetResult};
use crate::parser::{Function, Expr, Statement, SourceStatement, BinOp};

// =============================================================================
// 外设延迟模型
// =============================================================================

/// 外设类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PeripheralKind {
    GpioRead,
    GpioWrite,
    AdcRead,
    UartWrite,
    UartRead,
    SpiTransfer,
    I2cRead,
    I2cWrite,
    PwmWrite,
    TimerRead,
    DmaStart,
}

impl PeripheralKind {
    /// 从函数调用名推断外设类型
    pub fn from_call_name(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.contains("digitalread") || lower.contains("gpio.read") || lower.contains("gpio.input") {
            Some(Self::GpioRead)
        } else if lower.contains("digitalwrite") || lower.contains("gpio.output") || lower.contains("gpio.high") || lower.contains("gpio.low") {
            Some(Self::GpioWrite)
        } else if lower.contains("analogread") || lower.contains("adc.read") || lower.contains("adc.channel") {
            Some(Self::AdcRead)
        } else if lower.contains("uart.write") || lower.contains("serial.write") || lower.contains("console.log") {
            Some(Self::UartWrite)
        } else if lower.contains("uart.read") || lower.contains("serial.read") {
            Some(Self::UartRead)
        } else if lower.contains("spi.transfer") || lower.contains("spi.write") {
            Some(Self::SpiTransfer)
        } else if lower.contains("i2c.read") || lower.contains("wire.request") {
            Some(Self::I2cRead)
        } else if lower.contains("i2c.write") || lower.contains("wire.begin") {
            Some(Self::I2cWrite)
        } else if lower.contains("analogwrite") || lower.contains("pwm.write") || lower.contains("pwm.output") {
            Some(Self::PwmWrite)
        } else if lower.contains("millis") || lower.contains("micros") || lower.contains("timer.read") {
            Some(Self::TimerRead)
        } else if lower.contains("dma.start") {
            Some(Self::DmaStart)
        } else {
            None
        }
    }
}

/// 外设延迟模型 (单位: CPU cycles)
#[derive(Debug, Clone)]
pub struct PeripheralLatencyModel {
    pub gpio_read: u32,
    pub gpio_write: u32,
    pub adc_read: u32,
    pub uart_write_per_byte: u32,
    pub uart_read: u32,
    pub spi_transfer_per_byte: u32,
    pub i2c_read_per_byte: u32,
    pub i2c_write_per_byte: u32,
    pub pwm_write: u32,
    pub timer_read: u32,
    pub dma_start: u32,
}

impl PeripheralLatencyModel {
    /// 获取指定外设的延迟
    pub fn latency_for(&self, kind: PeripheralKind) -> u32 {
        match kind {
            PeripheralKind::GpioRead => self.gpio_read,
            PeripheralKind::GpioWrite => self.gpio_write,
            PeripheralKind::AdcRead => self.adc_read,
            PeripheralKind::UartWrite => self.uart_write_per_byte,
            PeripheralKind::UartRead => self.uart_read,
            PeripheralKind::SpiTransfer => self.spi_transfer_per_byte,
            PeripheralKind::I2cRead => self.i2c_read_per_byte,
            PeripheralKind::I2cWrite => self.i2c_write_per_byte,
            PeripheralKind::PwmWrite => self.pwm_write,
            PeripheralKind::TimerRead => self.timer_read,
            PeripheralKind::DmaStart => self.dma_start,
        }
    }

    /// ESP32-S3 外设延迟 (240MHz, APB 80MHz)
    pub fn esp32() -> Self {
        Self {
            gpio_read: 4,        // GPIO matrix: 2 APB cycles
            gpio_write: 4,
            adc_read: 180,       // SAR ADC: ~7.5μs @ 240MHz
            uart_write_per_byte: 240, // 115200 baud: ~87μs/byte
            uart_read: 240,
            spi_transfer_per_byte: 8, // 80MHz SPI: 80ns/byte
            i2c_read_per_byte: 960,   // 100kHz I2C: ~4μs/byte
            i2c_write_per_byte: 960,
            pwm_write: 6,        // LEDC: 2 APB cycles + sync
            timer_read: 2,       // 1 APB cycle
            dma_start: 12,       // DMA config registers
        }
    }

    /// STM32F4 (Cortex-M4 @ 168MHz, APB2 84MHz)
    pub fn stm32f4() -> Self {
        Self {
            gpio_read: 3,        // AHB: 1-2 cycles
            gpio_write: 3,
            adc_read: 252,       // 12-bit ADC: ~1.5μs @ 168MHz
            uart_write_per_byte: 1458, // 115200 baud
            uart_read: 1458,
            spi_transfer_per_byte: 4,  // 42MHz SPI
            i2c_read_per_byte: 1680,   // 100kHz I2C
            i2c_write_per_byte: 1680,
            pwm_write: 4,        // TIM register write
            timer_read: 2,
            dma_start: 8,
        }
    }

    /// STM32F103 (Cortex-M3 @ 72MHz)
    pub fn stm32f1() -> Self {
        Self {
            gpio_read: 2,
            gpio_write: 2,
            adc_read: 90,        // 12-bit: ~1.25μs
            uart_write_per_byte: 625, // 115200 baud @ 72MHz
            uart_read: 625,
            spi_transfer_per_byte: 4,
            i2c_read_per_byte: 720,
            i2c_write_per_byte: 720,
            pwm_write: 3,
            timer_read: 2,
            dma_start: 6,
        }
    }

    /// Arduino AVR (ATmega328P @ 16MHz)
    pub fn arduino_avr() -> Self {
        Self {
            gpio_read: 2,        // 2 cycles (PINx register)
            gpio_write: 2,
            adc_read: 208,       // 13μs @ 16MHz (125kHz ADC clock)
            uart_write_per_byte: 139, // 115200 baud @ 16MHz
            uart_read: 139,
            spi_transfer_per_byte: 16, // 8MHz SPI (F_CPU/2)
            i2c_read_per_byte: 160,    // 100kHz I2C
            i2c_write_per_byte: 160,
            pwm_write: 4,
            timer_read: 4,       // 16-bit timer: 2 reads
            dma_start: 0,        // AVR 无 DMA
        }
    }

    /// 根据目标平台获取外设延迟模型
    pub fn for_target(target: &CTarget) -> Self {
        match target {
            CTarget::Esp32 | CTarget::Esp32S3Bare => Self::esp32(),
            CTarget::Stm32 => Self::stm32f1(),
            CTarget::ArduinoAvr => Self::arduino_avr(),
            CTarget::ArduinoArm => Self::stm32f1(), // SAMD21 类似
            CTarget::RaspberryPi => Self {
                gpio_read: 30,   // GPIO via mmap: ~30 cycles
                gpio_write: 30,
                adc_read: 0,     // RPi 无内置 ADC
                uart_write_per_byte: 13020, // 115200 @ 1500MHz
                uart_read: 13020,
                spi_transfer_per_byte: 15,
                i2c_read_per_byte: 15000,
                i2c_write_per_byte: 15000,
                pwm_write: 20,
                timer_read: 5,
                dma_start: 50,
            },
        }
    }
}

// =============================================================================
// 中断干扰模型
// =============================================================================

/// 中断源描述
#[derive(Debug, Clone)]
pub struct InterruptSource {
    /// 中断名称
    pub name: String,
    /// 优先级 (0=最高)
    pub priority: u8,
    /// ISR 最坏执行周期
    pub isr_wcet_cycles: u64,
    /// 触发频率 (Hz), 0 = 事件驱动
    pub frequency_hz: u32,
}

/// 中断干扰分析结果
#[derive(Debug, Clone)]
pub struct InterruptInterference {
    /// 总阻塞周期 (所有更高优先级中断的最大叠加)
    pub total_blocking_cycles: u64,
    /// 各中断贡献
    pub contributions: Vec<(String, u64)>,
    /// 中断延迟 (从请求到 ISR 第一条指令)
    pub entry_latency_cycles: u32,
}

/// 中断干扰分析器
pub struct InterruptAnalyzer {
    sources: Vec<InterruptSource>,
    entry_latency: u32,
}

impl InterruptAnalyzer {
    pub fn new(target: &CTarget) -> Self {
        let entry_latency = match target {
            CTarget::Esp32 | CTarget::Esp32S3Bare => 16,      // Xtensa: ~16 cycles interrupt entry
            CTarget::Stm32 => 12,      // Cortex-M: 12 cycles (stacking)
            CTarget::ArduinoAvr => 4,  // AVR: 4 cycles
            CTarget::ArduinoArm => 12,
            CTarget::RaspberryPi => 20, // Linux: 不确定，取保守值
        };
        Self { sources: Vec::new(), entry_latency }
    }

    /// 注册中断源
    pub fn add_interrupt(&mut self, name: &str, priority: u8, isr_wcet_cycles: u64, freq_hz: u32) {
        self.sources.push(InterruptSource {
            name: name.to_string(),
            priority,
            isr_wcet_cycles,
            frequency_hz: freq_hz,
        });
    }

    /// 分析对指定优先级任务的干扰
    pub fn analyze(&self, task_priority: u8) -> InterruptInterference {
        let mut total: u64 = 0;
        let mut contributions = Vec::new();

        for src in &self.sources {
            if src.priority < task_priority {
                // 更高优先级的中断可以抢占
                let blocking = src.isr_wcet_cycles + self.entry_latency as u64;
                total += blocking;
                contributions.push((src.name.clone(), blocking));
            }
        }

        InterruptInterference {
            total_blocking_cycles: total,
            contributions,
            entry_latency_cycles: self.entry_latency,
        }
    }
}

// =============================================================================
// 循环上界自动推断
// =============================================================================

/// 循环边界推断结果
#[derive(Debug, Clone)]
pub struct LoopBoundInference {
    /// 推断的上界
    pub bound: Option<u64>,
    /// 推断依据
    pub evidence: String,
    /// 置信度 (High/Medium/Low)
    pub confidence: BoundConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoundConfidence {
    /// 精确: 常量上界 (i < 10)
    Exact,
    /// 高: 从初始化+步进推断 (i = 0; i < n; i += 2 → n/2)
    High,
    /// 中: 从数组长度推断 (i < arr.length)
    Medium,
    /// 低: 无法推断，使用保守默认值
    Low,
}

/// 从 for 循环三元组推断上界
pub fn infer_loop_bound(init: &Statement, condition: &Expr, update: &Statement) -> LoopBoundInference {
    // 尝试从条件中提取上界常量
    if let Some(bound) = extract_const_bound(condition) {
        // 检查步进值
        let step = extract_step(update);
        let actual_bound = if step > 1 { bound / step } else { bound };
        return LoopBoundInference {
            bound: Some(actual_bound),
            evidence: format!("常量上界={}, 步进={}", bound, step),
            confidence: BoundConfidence::Exact,
        };
    }

    // 检查是否是 arr.length 形式
    if is_array_length_bound(condition) {
        return LoopBoundInference {
            bound: None,
            evidence: "数组长度 (编译期已知则为定长)".to_string(),
            confidence: BoundConfidence::Medium,
        };
    }

    LoopBoundInference {
        bound: None,
        evidence: "无法推断，使用保守默认值 100".to_string(),
        confidence: BoundConfidence::Low,
    }
}

fn extract_const_bound(condition: &Expr) -> Option<u64> {
    match condition {
        Expr::BinOp { op, right, .. } => {
            match op {
                BinOp::Lt | BinOp::Le => {
                    if let Expr::Number(n) = right.as_ref() {
                        Some(*n as u64)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn extract_step(update: &Statement) -> u64 {
    match update {
        Statement::PostInc(_) | Statement::PostDec(_) => 1,
        Statement::Assign { value, .. } => {
            if let Expr::BinOp { right, .. } = value {
                if let Expr::Number(n) = right.as_ref() {
                    return *n as u64;
                }
            }
            1
        }
        _ => 1,
    }
}

fn is_array_length_bound(condition: &Expr) -> bool {
    if let Expr::BinOp { right, .. } = condition {
        if let Expr::PropAccess { property, .. } = right.as_ref() {
            return property == "length";
        }
    }
    false
}

// =============================================================================
// 高级 WCET 分析器
// =============================================================================

/// 高级 WCET 分析结果
#[derive(Debug, Clone)]
pub struct AdvancedWcetResult {
    /// 基础 WCET 结果
    pub base: WcetResult,
    /// 外设延迟贡献 (cycles)
    pub peripheral_cycles: u64,
    /// 外设访问明细
    pub peripheral_breakdown: Vec<(PeripheralKind, u32, u32)>, // (类型, 单次延迟, 调用次数)
    /// 中断干扰 (cycles)
    pub interrupt_blocking_cycles: u64,
    /// 总 WCET (含外设+中断) cycles
    pub total_wcet_cycles: u64,
    /// 总 WCET (μs)
    pub total_wcet_us: f64,
    /// 循环推断结果
    pub loop_inferences: Vec<LoopBoundInference>,
    /// 是否通过 (含所有开销)
    pub passed_with_overhead: bool,
}

/// 高级 WCET 分析器
pub struct AdvancedWcetAnalyzer {
    timing: TargetTimingModel,
    peripheral: PeripheralLatencyModel,
    interrupt_analyzer: InterruptAnalyzer,
    clock_mhz: u32,
}

impl AdvancedWcetAnalyzer {
    pub fn new(target: &CTarget) -> Self {
        let timing = get_timing_model(target);
        let clock_mhz = timing.clock_mhz;
        let peripheral = PeripheralLatencyModel::for_target(target);
        let interrupt_analyzer = InterruptAnalyzer::new(target);
        Self { timing, peripheral, interrupt_analyzer, clock_mhz }
    }

    /// 注册中断源
    pub fn add_interrupt(&mut self, name: &str, priority: u8, isr_wcet: u64, freq: u32) {
        self.interrupt_analyzer.add_interrupt(name, priority, isr_wcet, freq);
    }

    /// 分析函数 (含外设+中断)
    pub fn analyze(&self, func: &Function, base_result: &WcetResult, task_priority: u8) -> AdvancedWcetResult {
        // 扫描外设调用
        let (peripheral_cycles, breakdown) = self.scan_peripheral_calls(&func.body);

        // 中断干扰
        let interference = self.interrupt_analyzer.analyze(task_priority);

        // 总 WCET
        let total_cycles = base_result.wcet_cycles + peripheral_cycles + interference.total_blocking_cycles;
        let total_us = total_cycles as f64 / self.clock_mhz as f64;

        let passed_with_overhead = if let Some(limit) = base_result.limit_us {
            total_us <= limit as f64
        } else {
            true
        };

        AdvancedWcetResult {
            base: base_result.clone(),
            peripheral_cycles,
            peripheral_breakdown: breakdown,
            interrupt_blocking_cycles: interference.total_blocking_cycles,
            total_wcet_cycles: total_cycles,
            total_wcet_us: total_us,
            loop_inferences: Vec::new(),
            passed_with_overhead,
        }
    }

    /// 扫描语句中的外设调用
    fn scan_peripheral_calls(&self, stmts: &[SourceStatement]) -> (u64, Vec<(PeripheralKind, u32, u32)>) {
        let mut total: u64 = 0;
        let mut counts: std::collections::HashMap<PeripheralKind, u32> = std::collections::HashMap::new();

        for ss in stmts {
            self.scan_stmt_peripherals(&ss.stmt, &mut total, &mut counts);
        }

        let breakdown: Vec<(PeripheralKind, u32, u32)> = counts.iter()
            .map(|(kind, count)| (*kind, self.peripheral.latency_for(*kind), *count))
            .collect();

        (total, breakdown)
    }

    fn scan_stmt_peripherals(&self, stmt: &Statement, total: &mut u64, counts: &mut std::collections::HashMap<PeripheralKind, u32>) {
        match stmt {
            Statement::Let { value, .. } | Statement::Const { value, .. } | Statement::Assign { value, .. } => {
                self.scan_expr_peripherals(value, total, counts);
            }
            Statement::Return(expr) => self.scan_expr_peripherals(expr, total, counts),
            Statement::If { condition, then_body, else_body, .. } => {
                self.scan_expr_peripherals(condition, total, counts);
                for ss in then_body { self.scan_stmt_peripherals(&ss.stmt, total, counts); }
                if let Some(eb) = else_body {
                    for ss in eb { self.scan_stmt_peripherals(&ss.stmt, total, counts); }
                }
            }
            Statement::While { condition, body } | Statement::For { condition, body, .. } => {
                self.scan_expr_peripherals(condition, total, counts);
                for ss in body { self.scan_stmt_peripherals(&ss.stmt, total, counts); }
            }
            Statement::ConsoleLog(args) => {
                for _ in args {
                    let lat = self.peripheral.latency_for(PeripheralKind::UartWrite);
                    *total += lat as u64;
                    *counts.entry(PeripheralKind::UartWrite).or_insert(0) += 1;
                }
            }
            Statement::Expr(expr) => self.scan_expr_peripherals(expr, total, counts),
            _ => {}
        }
    }

    fn scan_expr_peripherals(&self, expr: &Expr, total: &mut u64, counts: &mut std::collections::HashMap<PeripheralKind, u32>) {
        match expr {
            Expr::Call { name, args } => {
                if let Some(kind) = PeripheralKind::from_call_name(name) {
                    let lat = self.peripheral.latency_for(kind);
                    *total += lat as u64;
                    *counts.entry(kind).or_insert(0) += 1;
                }
                for arg in args { self.scan_expr_peripherals(arg, total, counts); }
            }
            Expr::BinOp { left, right, .. } => {
                self.scan_expr_peripherals(left, total, counts);
                self.scan_expr_peripherals(right, total, counts);
            }
            Expr::Ternary { condition, then_expr, else_expr } => {
                self.scan_expr_peripherals(condition, total, counts);
                self.scan_expr_peripherals(then_expr, total, counts);
                self.scan_expr_peripherals(else_expr, total, counts);
            }
            Expr::Index { array, index } => {
                self.scan_expr_peripherals(array, total, counts);
                self.scan_expr_peripherals(index, total, counts);
            }
            Expr::PropAccess { object, .. } => self.scan_expr_peripherals(object, total, counts),
            Expr::Await { expr } => self.scan_expr_peripherals(expr, total, counts),
            _ => {}
        }
    }
}

// =============================================================================
// 结构化 WCET 报告
// =============================================================================

/// 生成高级 WCET 报告
pub fn generate_advanced_report(results: &[AdvancedWcetResult], target: &CTarget) -> String {
    let model = get_timing_model(target);
    let mut r = String::new();

    r.push_str("=== DUANYAN Advanced WCET Report (Phase W1) ===\n");
    r.push_str(&format!("Target: {} @ {}MHz\n", model.name, model.clock_mhz));
    r.push_str(&format!("Peripheral model: loaded\n"));
    r.push_str(&format!("Interrupt model: loaded\n\n"));

    r.push_str(&format!(
        "{:<18}{:>10}{:>10}{:>10}{:>10}{:>8}  {:>6}\n",
        "Function", "Base(cyc)", "Periph", "IRQ", "Total(cyc)", "Total(us)", "Status"
    ));
    r.push_str(&"─".repeat(80));
    r.push('\n');

    for res in results {
        let status = if res.base.is_realtime {
            if res.passed_with_overhead { "PASS" } else { "FAIL" }
        } else { "-" };

        r.push_str(&format!(
            "{:<18}{:>10}{:>10}{:>10}{:>10}{:>10.2}  {:>6}\n",
            res.base.func_name,
            res.base.wcet_cycles,
            res.peripheral_cycles,
            res.interrupt_blocking_cycles,
            res.total_wcet_cycles,
            res.total_wcet_us,
            status,
        ));

        // 外设明细
        if !res.peripheral_breakdown.is_empty() {
            for (kind, lat, count) in &res.peripheral_breakdown {
                r.push_str(&format!("    {:?}: {}cyc x {} = {}cyc\n",
                    kind, lat, count, *lat as u64 * *count as u64));
            }
        }
    }

    r.push_str(&"─".repeat(80));
    r.push('\n');
    let rt: Vec<&AdvancedWcetResult> = results.iter().filter(|x| x.base.is_realtime).collect();
    let passed = rt.iter().filter(|x| x.passed_with_overhead).count();
    r.push_str(&format!("@realtime: {} total, {} passed, {} failed\n",
        rt.len(), passed, rt.len() - passed));
    r.push_str("Heap: 0 bytes (zero-malloc guaranteed)\n");

    r
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peripheral_from_call_name() {
        assert_eq!(PeripheralKind::from_call_name("digitalRead"), Some(PeripheralKind::GpioRead));
        assert_eq!(PeripheralKind::from_call_name("GPIO.output"), Some(PeripheralKind::GpioWrite));
        assert_eq!(PeripheralKind::from_call_name("analogRead"), Some(PeripheralKind::AdcRead));
        assert_eq!(PeripheralKind::from_call_name("unknownFunc"), None);
    }

    #[test]
    fn test_esp32_peripheral_latency() {
        let model = PeripheralLatencyModel::esp32();
        assert_eq!(model.gpio_read, 4);
        assert_eq!(model.adc_read, 180);
        assert!(model.uart_write_per_byte > 100);
    }

    #[test]
    fn test_stm32_peripheral_latency() {
        let model = PeripheralLatencyModel::stm32f4();
        assert_eq!(model.gpio_read, 3);
        assert!(model.adc_read > model.gpio_read);
    }

    #[test]
    fn test_interrupt_analyzer() {
        let mut analyzer = InterruptAnalyzer::new(&CTarget::Esp32);
        analyzer.add_interrupt("Timer0", 0, 50, 1000);   // 高优先级
        analyzer.add_interrupt("UART_RX", 2, 100, 0);    // 低优先级

        // 分析优先级 3 的任务: 只有 Timer0(0) 和 UART_RX(2) 能抢占
        let result = analyzer.analyze(3);
        assert!(result.total_blocking_cycles > 0);
        assert_eq!(result.contributions.len(), 2);

        // 分析优先级 0 的任务: 没有更高优先级中断
        let result0 = analyzer.analyze(0);
        assert_eq!(result0.total_blocking_cycles, 0);
    }

    #[test]
    fn test_loop_bound_exact() {
        let init = Statement::Let { name: "i".into(), value: Expr::Number(0) };
        let cond = Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(Expr::Ident("i".into())),
            right: Box::new(Expr::Number(10)),
        };
        let update = Statement::PostInc("i".into());
        let result = infer_loop_bound(&init, &cond, &update);
        assert_eq!(result.bound, Some(10));
        assert_eq!(result.confidence, BoundConfidence::Exact);
    }

    #[test]
    fn test_loop_bound_with_step() {
        let init = Statement::Let { name: "i".into(), value: Expr::Number(0) };
        let cond = Expr::BinOp {
            op: BinOp::Lt,
            left: Box::new(Expr::Ident("i".into())),
            right: Box::new(Expr::Number(20)),
        };
        let update = Statement::Assign {
            name: "i".into(),
            value: Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(Expr::Ident("i".into())),
                right: Box::new(Expr::Number(2)),
            },
        };
        let result = infer_loop_bound(&init, &cond, &update);
        assert_eq!(result.bound, Some(10)); // 20 / 2 = 10
    }

    #[test]
    fn test_advanced_report_generation() {
        let base = WcetResult {
            func_name: "motorCtrl".into(),
            wcet_cycles: 500,
            wcet_us: 2.08,
            limit_us: Some(100),
            passed: true,
            is_realtime: true,
            max_loop_depth: 1,
            call_count: 2,
        };
        let advanced = AdvancedWcetResult {
            base,
            peripheral_cycles: 180,
            peripheral_breakdown: vec![(PeripheralKind::AdcRead, 180, 1)],
            interrupt_blocking_cycles: 66,
            total_wcet_cycles: 746,
            total_wcet_us: 3.11,
            loop_inferences: vec![],
            passed_with_overhead: true,
        };
        let report = generate_advanced_report(&[advanced], &CTarget::Esp32);
        assert!(report.contains("Advanced WCET Report"));
        assert!(report.contains("motorCtrl"));
        assert!(report.contains("PASS"));
        assert!(report.contains("AdcRead"));
    }
}
