//! 硬件API PC模拟器 (snar run 时使用)
//!
//! Phase B: 硬件API的PC模拟器
//! 在 `snar run` 模式下虚拟GPIO/UART/PWM/Timer行为

use std::collections::HashMap;
use std::time::{Instant, Duration};

/// GPIO 模拟状态
#[derive(Debug, Clone)]
pub struct GpioSim {
    /// 引脚方向 (false=input, true=output)
    pub direction: HashMap<u8, bool>,
    /// 引脚输出状态
    pub output: HashMap<u8, bool>,
    /// 引脚输入状态（模拟外部输入）
    pub input: HashMap<u8, bool>,
    /// 上拉使能
    pub pullup: HashMap<u8, bool>,
    /// 引脚值变化日志
    pub log: Vec<GpioEvent>,
}

#[derive(Debug, Clone)]
pub struct GpioEvent {
    pub pin: u8,
    pub value: bool,
    pub timestamp_ms: u64,
}

impl GpioSim {
    pub fn new() -> Self {
        GpioSim {
            direction: HashMap::new(),
            output: HashMap::new(),
            input: HashMap::new(),
            pullup: HashMap::new(),
            log: Vec::new(),
        }
    }

    pub fn set_output(&mut self, pin: u8) {
        self.direction.insert(pin, true);
    }

    pub fn set_input(&mut self, pin: u8, pullup: bool) {
        self.direction.insert(pin, false);
        self.pullup.insert(pin, pullup);
    }

    pub fn write(&mut self, pin: u8, value: bool, time_ms: u64) {
        self.output.insert(pin, value);
        self.log.push(GpioEvent { pin, value, timestamp_ms: time_ms });
    }

    pub fn read(&self, pin: u8) -> bool {
        *self.input.get(&pin).unwrap_or_else(|| {
            self.pullup.get(&pin).unwrap_or(&false)
        })
    }

    pub fn set_external_input(&mut self, pin: u8, value: bool) {
        self.input.insert(pin, value);
    }
}

/// UART 模拟状态
#[derive(Debug, Clone)]
pub struct UartSim {
    pub baud_rate: u32,
    pub tx_buffer: Vec<u8>,
    pub rx_buffer: Vec<u8>,
    pub output_string: String,
}

impl UartSim {
    pub fn new() -> Self {
        UartSim {
            baud_rate: 115200,
            tx_buffer: Vec::new(),
            rx_buffer: Vec::new(),
            output_string: String::new(),
        }
    }

    pub fn init(&mut self, baud: u32) {
        self.baud_rate = baud;
    }

    pub fn write_byte(&mut self, byte: u8) {
        self.tx_buffer.push(byte);
        if byte >= 32 && byte < 127 || byte == b'\n' || byte == b'\r' || byte == b'\t' {
            self.output_string.push(byte as char);
        }
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        if self.rx_buffer.is_empty() {
            None
        } else {
            Some(self.rx_buffer.remove(0))
        }
    }

    pub fn inject_input(&mut self, data: &[u8]) {
        self.rx_buffer.extend_from_slice(data);
    }
}

/// Timer 模拟状态
#[derive(Debug, Clone)]
pub struct TimerSim {
    pub enabled: bool,
    pub oneshot: bool,
    pub period_ms: u32,
    pub count: u32,
    pub interrupt_pending: bool,
    /// 定时器到期次数
    pub fire_count: u32,
}

impl TimerSim {
    pub fn new() -> Self {
        TimerSim {
            enabled: false,
            oneshot: false,
            period_ms: 0,
            count: 0,
            interrupt_pending: false,
            fire_count: 0,
        }
    }

    pub fn start(&mut self, period_ms: u32, oneshot: bool) {
        self.enabled = true;
        self.oneshot = oneshot;
        self.period_ms = period_ms;
        self.count = 0;
    }

    pub fn stop(&mut self) {
        self.enabled = false;
    }

    pub fn tick(&mut self, elapsed_ms: u32) {
        if !self.enabled { return; }
        self.count += elapsed_ms;
        if self.count >= self.period_ms {
            self.interrupt_pending = true;
            self.fire_count += 1;
            if self.oneshot {
                self.enabled = false;
            }
            self.count = 0;
        }
    }

    pub fn clear_interrupt(&mut self) {
        self.interrupt_pending = false;
    }
}

/// PWM 模拟状态
#[derive(Debug, Clone)]
pub struct PwmSim {
    pub channels: HashMap<u8, PwmChannel>,
}

#[derive(Debug, Clone)]
pub struct PwmChannel {
    pub pin: u8,
    pub frequency_hz: u32,
    pub duty_cycle: u8,  // 0-255
    pub enabled: bool,
}

impl PwmSim {
    pub fn new() -> Self {
        PwmSim { channels: HashMap::new() }
    }

    pub fn configure(&mut self, pin: u8, frequency: u32) {
        self.channels.insert(pin, PwmChannel {
            pin,
            frequency_hz: frequency,
            duty_cycle: 128,  // 50% default
            enabled: true,
        });
    }

    pub fn set_duty(&mut self, pin: u8, duty: u8) {
        if let Some(ch) = self.channels.get_mut(&pin) {
            ch.duty_cycle = duty;
        }
    }
}

/// ADC 模拟状态
#[derive(Debug, Clone)]
pub struct AdcSim {
    pub channels: HashMap<u8, u16>,  // channel → value (0-4095)
    pub current_channel: u8,
}

impl AdcSim {
    pub fn new() -> Self {
        AdcSim {
            channels: HashMap::new(),
            current_channel: 0,
        }
    }

    pub fn set_channel(&mut self, channel: u8) {
        self.current_channel = channel;
    }

    pub fn read(&self) -> u16 {
        *self.channels.get(&self.current_channel).unwrap_or(&2048)  // default midpoint
    }

    pub fn set_value(&mut self, channel: u8, value: u16) {
        self.channels.insert(channel, value.min(4095));
    }
}

/// 完整硬件模拟环境
pub struct HardwareSimulator {
    pub gpio: GpioSim,
    pub uart: UartSim,
    pub timer: TimerSim,
    pub pwm: PwmSim,
    pub adc: AdcSim,
    /// 系统启动时间
    pub start_time: Instant,
    /// 当前模拟时间 (ms)
    pub current_time_ms: u64,
}

impl HardwareSimulator {
    pub fn new() -> Self {
        HardwareSimulator {
            gpio: GpioSim::new(),
            uart: UartSim::new(),
            timer: TimerSim::new(),
            pwm: PwmSim::new(),
            adc: AdcSim::new(),
            start_time: Instant::now(),
            current_time_ms: 0,
        }
    }

    /// 处理硬件内存地址写入
    pub fn handle_store(&mut self, addr: i64, value: i64) {
        match addr {
            // UART
            0x100 => {
                let byte = (value & 0xFF) as u8;
                if byte == 0xFF {
                    // 数字标记，由引擎处理
                } else {
                    self.uart.write_byte(byte);
                }
            }
            0x103 => self.uart.init(value as u32),

            // GPIO
            0x200 => {
                // GPIO_DIR: 写入 pin 号 → 设为 output
                let pin = value as u8;
                self.gpio.direction.insert(pin, true);
            }
            0x201 => {
                // GPIO_OUT: 写入 pin 号 → 设高
                let pin = value as u8;
                self.gpio.write(pin, true, self.current_time_ms);
            }
            0x203 => {
                // GPIO_PULLUP: 写入 pin 号 → 使能上拉
                let pin = value as u8;
                self.gpio.pullup.insert(pin, true);
            }
            0x206 => {
                // GPIO_LOW: 写入 pin 号 → 设低
                let pin = value as u8;
                self.gpio.write(pin, false, self.current_time_ms);
            }
            0x207 => {
                // GPIO_DIR_IN: 写入 pin 号 → 设为 input
                let pin = value as u8;
                self.gpio.direction.insert(pin, false);
            }

            // Timer
            0x300 => {
                let enable = value & 1 != 0;
                let oneshot = value & 2 != 0;
                if enable {
                    self.timer.start(self.timer.period_ms, oneshot);
                } else {
                    self.timer.stop();
                }
            }
            0x302 => self.timer.period_ms = value as u32,
            0x303 => self.timer.clear_interrupt(),

            // PWM
            0x400 => self.pwm.configure((value & 0xFF) as u8, ((value >> 8) & 0xFFFFFF) as u32),
            0x402 => {
                let pin = ((value >> 16) & 0xFF) as u8;
                let duty = (value & 0xFF) as u8;
                self.pwm.set_duty(pin, duty);
            }

            // ADC
            0x501 => self.adc.set_channel(value as u8),

            _ => {}  // 其他地址由引擎的通用内存处理
        }
    }

    /// 处理硬件内存地址读取
    pub fn handle_load(&mut self, addr: i64) -> i64 {
        match addr {
            0x101 => self.uart.read_byte().map(|b| b as i64).unwrap_or(0),
            0x102 => {
                // bit0=TX ready(1), bit1=RX available
                let mut status: i64 = 1;  // TX always ready
                if !self.uart.rx_buffer.is_empty() {
                    status |= 2;
                }
                status
            }
            0x202 => {
                // GPIO input register
                let mut val: i64 = 0;
                for i in 0..32u8 {
                    if self.gpio.read(i) {
                        val |= 1 << i;
                    }
                }
                val
            }
            0x301 => self.timer.count as i64,
            0x303 => if self.timer.interrupt_pending { 1 } else { 0 },
            0x502 => self.adc.read() as i64,
            _ => 0,
        }
    }

    /// 打印硬件状态摘要
    pub fn print_status(&self) {
        println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  🖥️  硬件模拟状态");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        // GPIO
        if !self.gpio.output.is_empty() {
            print!("  GPIO: ");
            let mut pins: Vec<_> = self.gpio.output.iter().collect();
            pins.sort_by_key(|(k, _)| *k);
            for (pin, val) in pins {
                print!("P{}={} ", pin, if *val { "H" } else { "L" });
            }
            println!();
        }

        // UART
        if !self.uart.output_string.is_empty() {
            let out = &self.uart.output_string;
            if out.len() > 80 {
                println!("  UART: {}...({} bytes)", &out[..80], out.len());
            } else {
                println!("  UART: {}", out);
            }
        }

        // Timer
        if self.timer.fire_count > 0 {
            println!("  Timer: {} 次触发, 周期 {}ms", self.timer.fire_count, self.timer.period_ms);
        }

        // PWM
        if !self.pwm.channels.is_empty() {
            for (pin, ch) in &self.pwm.channels {
                println!("  PWM P{}: {}Hz duty={}/255", pin, ch.frequency_hz, ch.duty_cycle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpio_sim() {
        let mut gpio = GpioSim::new();
        gpio.set_output(13);
        gpio.write(13, true, 0);
        assert_eq!(gpio.output[&13], true);
        gpio.write(13, false, 1);
        assert_eq!(gpio.output[&13], false);
    }

    #[test]
    fn test_uart_sim() {
        let mut uart = UartSim::new();
        uart.write_byte(b'H');
        uart.write_byte(b'i');
        assert_eq!(uart.output_string, "Hi");
    }

    #[test]
    fn test_timer_sim() {
        let mut timer = TimerSim::new();
        timer.start(100, true);
        timer.tick(50);
        assert!(!timer.interrupt_pending);
        timer.tick(50);
        assert!(timer.interrupt_pending);
        assert!(!timer.enabled);  // oneshot
    }

    #[test]
    fn test_hardware_simulator_store() {
        let mut sim = HardwareSimulator::new();
        sim.handle_store(0x100, b'A' as i64);
        assert_eq!(sim.uart.output_string, "A");
    }

    #[test]
    fn test_hardware_simulator_load() {
        let mut sim = HardwareSimulator::new();
        sim.uart.rx_buffer.push(42);
        let val = sim.handle_load(0x101);
        assert_eq!(val, 42);
    }
}
