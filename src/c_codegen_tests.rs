// c_codegen 单元测试区 (2026-08 工程治理: 自 c_codegen.rs 剥离)
use super::*;

    use super::*;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    fn compile_to_c(source: &str, target: CTarget) -> String {
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        let result = generate_c_source(&ast, &target).unwrap();
        result.source
    }

    // 扩展 1: 全管线宽度测试 helper（strip 前提取注解 → 剥离 → 解析 → 带宽度表生成）
    fn compile_to_c_widths(source: &str, target: CTarget) -> String {
        let widths = crate::strip::extract_width_types(source);
        let stripped = crate::strip::strip_types(source);
        let tokens = tokenize(&stripped).unwrap();
        let ast = parse(&tokens).unwrap();
        generate_c_source_with_widths(&ast, &target, &widths).unwrap().source
    }

    #[test]
    fn test_width_u8_let() {
        let c = compile_to_c_widths("let r: u8 = 200;", CTarget::Esp32);
        assert!(c.contains("uint8_t r = (uint8_t)(200);"), "got: {}", c);
    }

    #[test]
    fn test_width_u16_param() {
        let c = compile_to_c_widths("function f(a: u16) { return a; }", CTarget::Esp32);
        assert!(c.contains("int32_t f(uint16_t a)"), "got: {}", c);
    }

    #[test]
    fn test_width_const_u32() {
        let c = compile_to_c_widths("const BASE: u32 = 100000;", CTarget::Esp32);
        assert!(c.contains("const uint32_t BASE = (uint32_t)(100000);"), "got: {}", c);
    }

    #[test]
    fn test_width_assign_truncates() {
        let c = compile_to_c_widths("let r: u8 = 0; r = 12;", CTarget::Esp32);
        assert!(c.contains("r = (uint8_t)(12);"), "got: {}", c);
    }

    #[test]
    fn test_width_overflow_is_error() {
        let src = "let r: u8 = 300;";
        let widths = crate::strip::extract_width_types(src);
        let stripped = crate::strip::strip_types(src);
        let tokens = tokenize(&stripped).unwrap();
        let ast = parse(&tokens).unwrap();
        let res = generate_c_source_with_widths(&ast, &CTarget::Esp32, &widths);
        assert!(res.is_err(), "expected width overflow error");
    }

    #[test]
    fn test_plain_let_still_int32() {
        let c = compile_to_c_widths("let x = 42;", CTarget::Esp32);
        assert!(c.contains("int32_t x = 42;"), "got: {}", c);
    }

    // 扩展 2: 寄存器位域 helper（.js 路径，不 strip）
    fn compile_to_c_reg(source: &str, target: CTarget) -> String {
        let regs = crate::strip::extract_registers(source);
        let widths = crate::strip::extract_width_types(source);
        let tokens = tokenize(source).unwrap();
        let ast = parse(&tokens).unwrap();
        generate_c_source_with_ext(&ast, &target, &widths, &regs).unwrap().source
    }

    #[test]
    fn test_register_decl_no_storage() {
        let c = compile_to_c_reg("@register(0x40004000) const GPIO_OUT = { en: [1, 1] };", CTarget::Esp32);
        assert!(c.contains("/* @register GPIO_OUT @ 0x40004000: en[1:1] */"), "got: {}", c);
        assert!(!c.contains("const int32_t GPIO_OUT"), "got: {}", c);
    }

    #[test]
    fn test_register_write_rmw() {
        let src = "@register(0x40004000) const GPIO_OUT = { en: [1, 1], val: [7, 2] };\nGPIO_OUT.en = 1;";
        let c = compile_to_c_reg(src, CTarget::Esp32);
        assert!(c.contains("#define DUANYAN_REG32(addr) (*(volatile uint32_t *)(addr))"), "got: {}", c);
        assert!(c.contains("DUANYAN_REG32(0x40004000) = (DUANYAN_REG32(0x40004000) & ~(0x1 << 1)) | (((uint32_t)(1) & 0x1) << 1);"), "got: {}", c);
    }

    #[test]
    fn test_register_read_masked() {
        let src = "@register(0x40004000) const GPIO_OUT = { val: [7, 2] };\nlet x = GPIO_OUT.val;";
        let c = compile_to_c_reg(src, CTarget::Esp32);
        assert!(c.contains("((DUANYAN_REG32(0x40004000) >> 2) & 0x3F)"), "got: {}", c);
    }

    #[test]
    fn test_simple_variable() {
        let c = compile_to_c("let x = 42;", CTarget::Esp32);
        assert!(c.contains("int32_t x = 42;"));
    }

    #[test]
    fn test_const_declaration() {
        let c = compile_to_c("const MAX = 100;", CTarget::Esp32);
        assert!(c.contains("const int32_t MAX = 100;"));
    }

    #[test]
    fn test_function_generation() {
        let c = compile_to_c("function add(a, b) { return a + b; }", CTarget::Esp32);
        assert!(c.contains("int32_t add(int32_t a, int32_t b)"));
        assert!(c.contains("return (a + b);"));
    }

    #[test]
    fn test_console_log() {
        let c = compile_to_c("console.log(42);", CTarget::Esp32);
        assert!(c.contains("printf(\"%d\\n\", 42)"));
        assert!(c.contains("#include <stdio.h>"));
    }

    #[test]
    fn test_while_loop() {
        let c = compile_to_c("let i = 0; while (i < 10) { i++; }", CTarget::Esp32);
        assert!(c.contains("while ((i < 10))"));
        assert!(c.contains("i++;"));
    }

    #[test]
    fn test_if_else() {
        let c = compile_to_c("let x = 10; if (x > 5) { console.log(1); } else { console.log(0); }", CTarget::Esp32);
        assert!(c.contains("if ((x > 5))"));
        assert!(c.contains("} else {"));
    }

    #[test]
    fn test_esp32_includes() {
        let c = compile_to_c(
            "import { GPIO, Timer } from \"duanyan:hw\"; let led = GPIO.output(2);",
            CTarget::Esp32
        );
        assert!(c.contains("#include \"driver/gpio.h\""));
        assert!(c.contains("#include \"freertos/FreeRTOS.h\""));
    }

    #[test]
    fn test_esp32_app_main() {
        let c = compile_to_c("let x = 1;", CTarget::Esp32);
        assert!(c.contains("void app_main(void)"));
    }

    #[test]
    fn test_gpio_output() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let led = GPIO.output(2);",
            CTarget::Esp32
        );
        assert!(c.contains("duanyan_gpio_output(2)"));
    }

    #[test]
    fn test_timer_delay_esp32() {
        let c = compile_to_c(
            "import { Timer } from \"duanyan:hw\"; Timer.delay(500);",
            CTarget::Esp32
        );
        assert!(c.contains("vTaskDelay(pdMS_TO_TICKS(500))"));
    }

    #[test]
    fn test_switch_case() {
        let c = compile_to_c(
            "let x = 1; switch (x) { case 1: console.log(1); break; default: console.log(0); break; }",
            CTarget::Esp32
        );
        assert!(c.contains("switch (x)"));
        assert!(c.contains("case 1:"));
        assert!(c.contains("default:"));
    }

    #[test]
    fn test_arrow_function_hoist() {
        let c = compile_to_c("let add = (a, b) => a + b;", CTarget::Esp32);
        assert!(c.contains("_duanyan_fn_add"));
        assert!(c.contains("return (a + b);"));
    }

    #[test]
    fn test_ternary() {
        let c = compile_to_c("let x = 10; let y = x > 5 ? 1 : 0;", CTarget::Esp32);
        assert!(c.contains("(x > 5) ? 1 : 0"));
    }

    #[test]
    fn test_for_loop() {
        let c = compile_to_c("for (let i = 0; i < 10; i++) { console.log(i); }", CTarget::Esp32);
        assert!(c.contains("for (int32_t i = 0;"));
        assert!(c.contains("i < 10"));
        assert!(c.contains("i++"));
    }

    #[test]
    fn test_string_literal() {
        let c = compile_to_c("let s = \"hello\";", CTarget::Esp32);
        assert!(c.contains("\"hello\""));
    }

    #[test]
    fn test_bool_null() {
        let c = compile_to_c("let a = true; let b = false; let c = null;", CTarget::Esp32);
        assert!(c.contains("int32_t a = 1;"));
        assert!(c.contains("int32_t b = 0;"));
        assert!(c.contains("int32_t c = 0;"));
    }

    // ========== Arduino 后端测试 ==========

    #[test]
    fn test_arduino_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let led = GPIO.output(13);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("#include <Arduino.h>"));
        // Arduino 不需要 stdint.h/stdio.h
        assert!(!c.contains("#include <stdint.h>"));
        assert!(!c.contains("#include <stdio.h>"));
    }

    #[test]
    fn test_arduino_setup_loop() {
        let c = compile_to_c(
            "import { GPIO, Timer } from \"duanyan:hw\"; let led = GPIO.output(13); while (true) { led.high(); Timer.delay(1000); led.low(); Timer.delay(1000); }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("void setup()"));
        assert!(c.contains("void loop()"));
        assert!(c.contains("delay(1000)"));
        // setup 中应有 GPIO 初始化
        assert!(c.contains("duanyan_gpio_output(13)"));
    }

    #[test]
    fn test_arduino_setup_only() {
        // 没有 while(true) 时，所有代码放入 setup()
        let c = compile_to_c("let x = 42;", CTarget::ArduinoAvr);
        assert!(c.contains("void setup()"));
        assert!(c.contains("void loop()"));
        assert!(c.contains("int32_t x = 42;"));
    }

    #[test]
    fn test_arduino_console_log() {
        let c = compile_to_c("console.log(42);", CTarget::ArduinoAvr);
        assert!(c.contains("Serial.println(42)"));
        assert!(c.contains("Serial.begin(115200)"));
    }

    #[test]
    fn test_arduino_console_log_string() {
        let c = compile_to_c("console.log(\"hello\");", CTarget::ArduinoAvr);
        assert!(c.contains("Serial.println(\"hello\")"));
    }

    #[test]
    fn test_arduino_console_log_empty() {
        let c = compile_to_c("console.log();", CTarget::ArduinoAvr);
        assert!(c.contains("Serial.println()"));
    }

    #[test]
    fn test_arduino_gpio_high_low() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let led = GPIO.output(13); while (true) { led.high(); led.low(); }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("duanyan_gpio_high(13)"));
        assert!(c.contains("duanyan_gpio_low(13)"));
    }

    #[test]
    fn test_arduino_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"duanyan:hw\"; Timer.delay(500);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("delay(500)"));
    }

    #[test]
    fn test_arduino_arm_target() {
        let c = compile_to_c(
            "import { GPIO, Timer } from \"duanyan:hw\"; let led = GPIO.output(2); while (true) { led.high(); Timer.delay(100); led.low(); Timer.delay(100); }",
            CTarget::ArduinoArm
        );
        assert!(c.contains("#include <Arduino.h>"));
        assert!(c.contains("void setup()"));
        assert!(c.contains("void loop()"));
        assert!(c.contains("delay(100)"));
    }

    #[test]
    fn test_arduino_gpio_helpers_comment() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let x = GPIO.output(5);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("DUANYAN GPIO helpers (Arduino AVR)"));
    }

    #[test]
    fn test_arduino_arm_helpers_comment() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let x = GPIO.output(5);",
            CTarget::ArduinoArm
        );
        assert!(c.contains("DUANYAN GPIO helpers (Arduino ARM)"));
    }

    // ========== STM32 后端测试 ==========

    #[test]
    fn test_stm32_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let led = GPIO.output(5);",
            CTarget::Stm32
        );
        assert!(c.contains("#include \"stm32f1xx_hal.h\""));
    }

    #[test]
    fn test_stm32_gpio_helpers() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let led = GPIO.output(5); while (true) { led.high(); led.low(); }",
            CTarget::Stm32
        );
        assert!(c.contains("DUANYAN GPIO helpers (STM32 HAL)"));
        assert!(c.contains("HAL_GPIO_WritePin"));
        assert!(c.contains("duanyan_gpio_high(5)"));
        assert!(c.contains("duanyan_gpio_low(5)"));
    }

    #[test]
    fn test_stm32_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"duanyan:hw\"; Timer.delay(500);",
            CTarget::Stm32
        );
        assert!(c.contains("HAL_Delay(500)"));
    }

    #[test]
    fn test_stm32_main_function() {
        let c = compile_to_c("let x = 1;", CTarget::Stm32);
        assert!(c.contains("int main(void)"));
    }

    #[test]
    fn test_stm32_uart_helpers() {
        let c = compile_to_c(
            "import { UART } from \"duanyan:hw\"; UART.init(115200);",
            CTarget::Stm32
        );
        assert!(c.contains("#include <string.h>"));
        assert!(c.contains("duanyan_uart_init(115200)"));
    }

    // ========== 树莓派后端测试 ==========

    #[test]
    fn test_rpi_includes() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let led = GPIO.output(17);",
            CTarget::RaspberryPi
        );
        assert!(c.contains("#include <fcntl.h>"));
        assert!(c.contains("#include <unistd.h>"));
    }

    #[test]
    fn test_rpi_gpio_sysfs() {
        let c = compile_to_c(
            "import { GPIO } from \"duanyan:hw\"; let led = GPIO.output(17); while (true) { led.high(); led.low(); }",
            CTarget::RaspberryPi
        );
        assert!(c.contains("DUANYAN GPIO helpers (Linux sysfs)"));
        assert!(c.contains("/sys/class/gpio/"));
        assert!(c.contains("duanyan_gpio_high(17)"));
        assert!(c.contains("duanyan_gpio_low(17)"));
    }

    #[test]
    fn test_rpi_timer_delay() {
        let c = compile_to_c(
            "import { Timer } from \"duanyan:hw\"; Timer.delay(1000);",
            CTarget::RaspberryPi
        );
        assert!(c.contains("usleep(1000 * 1000)"));
    }

    #[test]
    fn test_rpi_main_function() {
        let c = compile_to_c("let x = 1;", CTarget::RaspberryPi);
        assert!(c.contains("int main(void)"));
    }

    #[test]
    fn test_rpi_console_log() {
        let c = compile_to_c("console.log(42);", CTarget::RaspberryPi);
        assert!(c.contains("printf"));
    }

    // Phase v1.5: async 函数 C 代码生成测试
    #[test]
    fn test_cgen_async_function() {
        let c = compile_to_c("async function readSensor() { let val = await ADC.read(0); return val; }", CTarget::Esp32);
        assert!(c.contains("readSensor_ctx"), "应生成上下文结构体");
        assert!(c.contains("readSensor_poll"), "应生成 poll 函数");
        assert!(c.contains("DUANYAN_PENDING"), "应返回 PENDING");
        assert!(c.contains("DUANYAN_READY"), "应返回 READY");
        assert!(c.contains("switch (ctx->state)"), "应包含状态分发");
    }

    #[test]
    fn test_cgen_async_spawn() {
        let c = compile_to_c("async function foo() { let x = await bar(); return x; }", CTarget::Esp32);
        assert!(c.contains("foo_spawn"), "应生成 spawn 函数");
        assert!(c.contains("duanyan_spawn"), "应调用 duanyan_spawn");
    }

    #[test]
    fn test_cgen_async_multi_await() {
        let c = compile_to_c(
            "async function multi() { let a = await r1(); let b = await r2(); return a; }",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("multi_ctx"));
        assert!(c.contains("multi_poll"));
        // 应该有多个 case
        assert!(c.contains("case 0:"));
        assert!(c.contains("case 1:"));
    }

    // Phase 1.1: Promise.all/race 代码生成测试
    #[test]
    fn test_promise_all_codegen() {
        let c = compile_to_c(
            "async function multi() { let r = await Promise.all([readA(), readB()]); return r; }",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_promise_all_1"), "应生成 Promise.all 辅助函数");
        assert!(c.contains("_duanyan_pall_1_done"), "应生成完成标志");
        assert!(c.contains("_duanyan_pall_1_results"), "应生成结果数组");
        assert!(c.contains("0x3"), "应检查所有任务完成 (2任务 = 0x3)");
    }

    #[test]
    fn test_promise_race_codegen() {
        let c = compile_to_c(
            "async function race() { let r = await Promise.race([sensorA(), sensorB(), sensorC()]); return r; }",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_promise_race_1"), "应生成 Promise.race 辅助函数");
        assert!(c.contains("_duanyan_prace_1_done"), "应生成完成标志");
        assert!(c.contains("_duanyan_prace_1_result"), "应生成结果变量");
    }

    #[test]
    fn test_promise_all_empty() {
        let c = compile_to_c(
            "async function empty() { let r = await Promise.all([]); return r; }",
            CTarget::Esp32
        );
        // 空 Promise.all 应直接返回 1 (立即完成)
        assert!(c.contains("multi_ctx") || c.contains("empty_ctx"), "应生成状态机");
    }

    // Phase 1.2: Servo/Motor 代码生成测试
    #[test]
    fn test_servo_attach_esp32() {
        let c = compile_to_c(
            "import { Servo } from \"duanyan:motor\"; Servo.attach(13, 500, 2500);",
            CTarget::Esp32
        );
        assert!(c.contains("#include \"driver/ledc.h\""), "应包含 LEDC 头文件");
        assert!(c.contains("DUANYAN Servo helpers (ESP32 LEDC 50Hz PWM)"), "应生成 Servo 辅助函数");
        assert!(c.contains("duanyan_servo_attach(13, 500, 2500);"), "应调用 duanyan_servo_attach");
    }

    #[test]
    fn test_servo_write_esp32() {
        let c = compile_to_c(
            "import { Servo } from \"duanyan:motor\"; Servo.attach(13, 500, 2500); Servo.write(13, 90);",
            CTarget::Esp32
        );
        assert!(c.contains("duanyan_servo_write(13, 90);"), "应调用 duanyan_servo_write");
        assert!(c.contains("duanyan_servo_attach"), "应包含 attach 辅助函数");
    }

    #[test]
    fn test_servo_arduino() {
        let c = compile_to_c(
            "import { Servo } from \"duanyan:motor\"; Servo.attach(9, 500, 2500); Servo.write(9, 45);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("#include <Servo.h>"), "应包含 Servo.h");
        assert!(c.contains("DUANYAN Servo helpers (Arduino Servo.h)"), "应生成 Arduino Servo 辅助");
        assert!(c.contains("duanyan_servo_attach(9, 500, 2500);"), "应调用 attach");
        assert!(c.contains("duanyan_servo_write(9, 45);"), "应调用 write");
    }

    #[test]
    fn test_servo_stm32() {
        let c = compile_to_c(
            "import { Servo } from \"duanyan:motor\"; Servo.attach(0, 500, 2500); Servo.write(0, 180);",
            CTarget::Stm32
        );
        assert!(c.contains("DUANYAN Servo helpers (STM32 TIM2 PWM 50Hz)"), "应生成 STM32 Servo 辅助");
        assert!(c.contains("HAL_TIM_PWM_Init"), "应调用 HAL TIM PWM");
        assert!(c.contains("duanyan_servo_write(0, 180);"), "应调用 write");
    }

    #[test]
    fn test_motor_init_esp32() {
        let c = compile_to_c(
            "import { Motor } from \"duanyan:motor\"; Motor.init(2, 3, 5);",
            CTarget::Esp32
        );
        assert!(c.contains("DUANYAN Motor helpers (ESP32 LEDC + GPIO)"), "应生成 Motor 辅助函数");
        assert!(c.contains("duanyan_motor_init(2, 3, 5);"), "应调用 duanyan_motor_init");
    }

    #[test]
    fn test_motor_set_speed_esp32() {
        let c = compile_to_c(
            "import { Motor } from \"duanyan:motor\"; Motor.init(2, 3, 5); Motor.setSpeed(0, 200);",
            CTarget::Esp32
        );
        assert!(c.contains("duanyan_motor_set_speed(0, 200);"), "应调用 duanyan_motor_set_speed");
    }

    #[test]
    fn test_motor_brake_esp32() {
        let c = compile_to_c(
            "import { Motor } from \"duanyan:motor\"; Motor.init(2, 3, 5); Motor.brake(0);",
            CTarget::Esp32
        );
        assert!(c.contains("duanyan_motor_brake(0);"), "应调用 duanyan_motor_brake");
    }

    #[test]
    fn test_motor_arduino() {
        let c = compile_to_c(
            "import { Motor } from \"duanyan:motor\"; Motor.init(4, 5, 6); Motor.setSpeed(0, 128); Motor.brake(0);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("DUANYAN Motor helpers (Arduino AVR)"), "应生成 Arduino Motor 辅助");
        assert!(c.contains("duanyan_motor_init(4, 5, 6);"), "应调用 init");
        assert!(c.contains("duanyan_motor_set_speed(0, 128);"), "应调用 setSpeed");
        assert!(c.contains("duanyan_motor_brake(0);"), "应调用 brake");
    }

    #[test]
    fn test_servo_attach_defaults() {
        let c = compile_to_c(
            "import { Servo } from \"duanyan:motor\"; Servo.attach(11);",
            CTarget::Esp32
        );
        // 默认 min=500, max=2500
        assert!(c.contains("duanyan_servo_attach(11, 500, 2500);"), "应使用默认脉宽");
    }

    // v1.6 P0: duanyan:servo 关节层代码生成测试
    #[test]
    fn test_joint_bind_write_esp32() {
        let c = compile_to_c(
            "import { Joint } from \"duanyan:servo\"; Joint.bind(0, 0, 0, 30, 150, 90); Joint.write(0, 90);",
            CTarget::Esp32
        );
        assert!(c.contains("#include \"driver/i2c.h\""), "PCA9685 走 I2C");
        assert!(c.contains("#include \"driver/uart.h\""), "ZX361 走 UART");
        assert!(c.contains("DUANYAN Joint/Servo helpers (v1.6 P0"), "应生成关节层 helper");
        assert!(c.contains("duanyan_pca9685_write_us"), "应含 PCA9685 helper");
        assert!(c.contains("duanyan_zx361_move"), "应含 ZX361 helper");
        assert!(c.contains("duanyan_joint_bind(0, 0, 0, 30, 150, 90);"), "应调用 duanyan_joint_bind");
        assert!(c.contains("duanyan_joint_write(0, 90, 0);"), "应调用 duanyan_joint_write (ms 默认 0)");
        assert!(c.contains("运行期包络钳位"), "应含运行期包络钳位注释");
    }

    #[test]
    fn test_joint_write_with_ms_and_read_home() {
        let c = compile_to_c(
            "import { Joint } from \"duanyan:servo\"; Joint.bind(4, 1, 2, 0, 240, 120); Joint.write(4, 120, 500); let p = Joint.read(4); Joint.home();",
            CTarget::Esp32
        );
        assert!(c.contains("duanyan_joint_write(4, 120, 500);"), "总线关节带行程 ms");
        assert!(c.contains("duanyan_joint_read(4)"), "Joint.read 生成值表达式");
        assert!(c.contains("duanyan_joint_home();"), "Joint.home 全归中");
    }

    // v1.6 P1: duanyan:servo 安全护栏代码生成测试
    #[test]
    fn test_safety_check_esp32() {
        let c = compile_to_c(
            "import { Safety } from \"duanyan:servo\"; let a = Safety.check(300, 40, 1000);",
            CTarget::Esp32
        );
        assert!(c.contains("#ifndef DUANYAN_LOAD_MAX"), "限额可编译期覆盖");
        assert!(c.contains("DUANYAN actuator safety guard (v1.6 P1"), "应生成安全护栏 helper");
        assert!(c.contains("_duanyan_safety_check(300, 40, 1000)"), "Safety.check 生成值表达式");
        assert!(c.contains("temp > stall > load") || c.contains("temp_c > _duanyan_temp_max"), "优先级 temp>stall>load");
    }

    #[test]
    fn test_safety_limits_esp32() {
        let c = compile_to_c(
            "import { Safety } from \"duanyan:servo\"; Safety.limits(900, 65, 3000);",
            CTarget::Esp32
        );
        assert!(c.contains("duanyan_safety_limits(900, 65, 3000);"), "Safety.limits 运行期设置限额");
    }

    // v1.6 P2: duanyan:robot 轨迹/glide 代码生成测试
    #[test]
    fn test_robot_joint_glide_esp32() {
        let c = compile_to_c(
            "import { Traj, Joint } from \"duanyan:robot\"; Joint.glide(0, 30, 120, 2, 20);",
            CTarget::Esp32
        );
        assert!(c.contains("#include <math.h>"), "轨迹数学需 math.h");
        assert!(c.contains("DUANYAN robot motion helpers (v1.6 P2"), "应生成轨迹 helper");
        assert!(c.contains("_duanyan_joint_glide(0, 30, 120, 2, 20);"), "Joint.glide 调用");
        assert!(c.contains("_duanyan_quintic_plan"), "应含五次规划 helper");
        assert!(c.contains("duanyan_joint_write(idx, p.pos"), "glide 逐采样点驱动关节");
    }

    #[test]
    fn test_robot_traj_quintic_sample() {
        let c = compile_to_c(
            "import { Traj } from \"duanyan:robot\"; let p = Traj.sample(Traj.quintic(30, 120, 2), 1);",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_quintic_plan(30, 120, 2)"), "Traj.quintic 生成规划调用");
        assert!(c.contains("_duanyan_quintic_sample("), "Traj.sample 生成采样调用");
    }

    // v1.6 P3: duanyan:modeb Mode-B 线协议直发代码生成测试
    #[test]
    fn test_modeb_arm_home_esp32() {
        let c = compile_to_c(
            "import { ModeB } from \"duanyan:modeb\"; ModeB.armHome();",
            CTarget::Esp32
        );
        assert!(c.contains("#include \"driver/uart.h\""), "Mode-B 走 UART");
        assert!(c.contains("DUANYAN Mode-B wire protocol (v1.6 P3"), "应生成 Mode-B helper");
        assert!(c.contains("_duanyan_modeb_crc8"), "应含 C 版 crc8");
        assert!(c.contains("_duanyan_modeb_arm_home();"), "armHome 无参调用");
        assert!(c.contains("4, 0,0x03,0x84, 1,0x03,0x84"), "arm_home 黄金帧载荷 (四关节 @900)");
        assert!(c.contains("for (int i = 0; i < 8; i++)"), "8×'U' 校准突发");
    }

    #[test]
    fn test_modeb_joint_set_and_heartbeat_esp32() {
        let c = compile_to_c(
            "import { ModeB } from \"duanyan:modeb\"; ModeB.jointSet(2, 900, 0, 0); ModeB.heartbeat(0);",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_modeb_joint_set(2, 900, 0, 0);"), "0x20 关节设置");
        assert!(c.contains("_duanyan_modeb_send(0x20, pl, 7);"), "joint_set 组 7 字节载荷");
        assert!(c.contains("_duanyan_modeb_heartbeat(0);"), "0x01 心跳");
    }

    #[test]
    fn test_modeb_send_and_frame_esp32() {
        let c = compile_to_c(
            "import { ModeB } from \"duanyan:modeb\"; ModeB.send(18, buf, 0); let n = ModeB.frame(18, buf, 0);",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_modeb_send(18, buf, 0);"), "send 为语句");
        assert!(c.contains("_duanyan_modeb_send(18, buf, 0)"), "frame 为值表达式");
    }

    #[test]
    fn test_modeb_spine_target_alias() {
        // duanyan-spine / spine 别名→ Esp32S3Bare
        assert_eq!(CTarget::from_str("duanyan-spine"), Some(CTarget::Esp32S3Bare));
        assert_eq!(CTarget::from_str("spine"), Some(CTarget::Esp32S3Bare));
        // 脊髄端骨架: 走裸机 UART0 FIFO 直发, 不依赖 ESP-IDF uart_write_bytes
        let c = compile_to_c(
            "import { ModeB } from \"duanyan:modeb\"; ModeB.armHome();",
            CTarget::Esp32S3Bare
        );
        assert!(c.contains("Mode-B spine skeleton (v1.6 P3, --target duanyan-spine)"), "应生成脊髄端骨架");
        assert!(c.contains("duanyan_uart_putc('U')"), "裸机 'U' 突发走 duanyan_uart_putc");
        assert!(c.contains("_duanyan_modeb_arm_home();"), "armHome 调用");
        assert!(!c.contains("uart_write_bytes"), "脊髄端不依赖 ESP-IDF UART 驱动");
    }

    // Phase 1.3: 调度器代码生成测试
    #[test]
    fn test_set_interval_esp32() {
        let c = compile_to_c(
            "function tick() { let x = 1; } setInterval(tick, 100);",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_task_0"), "应生成任务包装函数");
        assert!(c.contains("xTaskCreate"), "应调用 xTaskCreate");
        assert!(c.contains("tick()"), "应调用回调函数");
        assert!(c.contains("vTaskDelay(pdMS_TO_TICKS(100))"), "应包含延时期望值");
    }

    #[test]
    fn test_set_interval_arduino() {
        let c = compile_to_c(
            "function blink() { let x = 1; } setInterval(blink, 500);",
            CTarget::ArduinoAvr
        );
        assert!(c.contains("_duanyan_scheduler_poll"), "应生成调度器 poll");
        assert!(c.contains("millis()"), "应使用 millis()");
        assert!(c.contains("blink()"), "应调用回调");
        assert!(c.contains("_duanyan_task_0_last"), "应生成时间戳变量");
    }

    #[test]
    fn test_set_timeout_esp32() {
        let c = compile_to_c(
            "function initOnce() { let x = 1; } setTimeout(initOnce, 3000);",
            CTarget::Esp32
        );
        assert!(c.contains("vTaskDelete(NULL)"), "单次任务应自删除");
        assert!(c.contains("vTaskDelay(pdMS_TO_TICKS(3000))"), "应延时 3000ms");
    }

    #[test]
    fn test_set_interval_arrow_fn() {
        let c = compile_to_c(
            "setInterval(() => { let x = 1; }, 50);",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_task_fn_"), "应提升箭头函数");
        assert!(c.contains("xTaskCreate"), "应创建任务");
    }

    #[test]
    fn test_multiple_intervals_esp32() {
        let c = compile_to_c(
            "function a() { let x = 1; } function b() { let y = 2; } setInterval(a, 10); setInterval(b, 100);",
            CTarget::Esp32
        );
        assert!(c.contains("_duanyan_task_0"), "应生成任务0");
        assert!(c.contains("_duanyan_task_1"), "应生成任务1");
        assert!(c.contains("a()"), "应调用 a");
        assert!(c.contains("b()"), "应调用 b");
    }

    #[test]
    fn test_set_interval_stm32() {
        let c = compile_to_c(
            "function ctrl() { let x = 1; } setInterval(ctrl, 1);",
            CTarget::Stm32
        );
        assert!(c.contains("HAL_GetTick()"), "应使用 HAL_GetTick");
        assert!(c.contains("_duanyan_scheduler_poll"), "应生成 poll");
        assert!(c.contains("ctrl()"), "应调用回调");
    }

    // Phase 1.4: 端到端 Demo 测试
    #[test]
    fn test_robot_arm_esp32_end_to_end() {
        let source = r#"
import { Servo } from "duanyan:motor";
import { ADC, Timer } from "duanyan:hw";

const GRIP = 13;
const ELBOW = 12;
const SHOULDER = 14;

Servo.attach(SHOULDER, 500, 2500);
Servo.attach(ELBOW, 500, 2500);
Servo.attach(GRIP, 500, 2500);

async function grabAndPour() {
  Servo.write(SHOULDER, 45);
  await Timer.delay(500);
  Servo.write(GRIP, 90);
  await Timer.delay(200);
  Servo.write(ELBOW, 120);
  await Timer.delay(300);
  Servo.write(GRIP, 30);
}

function readSensors() {
  let pot = ADC.read(0);
  let angle = pot / 22;
  Servo.write(SHOULDER, angle);
}

setInterval(grabAndPour, 5000);
setInterval(readSensors, 10);
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        // 验证头文件
        assert!(c.contains("#include \"driver/ledc.h\""), "应包含 LEDC");
        assert!(c.contains("#include \"driver/gpio.h\""), "应包含 GPIO");
        assert!(c.contains("#include \"driver/adc.h\""), "应包含 ADC");
        assert!(c.contains("freertos/FreeRTOS.h"), "应包含 FreeRTOS");
        // 验证 Servo 辅助函数
        assert!(c.contains("duanyan_servo_attach"), "应生成 servo attach");
        assert!(c.contains("duanyan_servo_write"), "应生成 servo write");
        // 验证 ADC 辅助函数
        assert!(c.contains("duanyan_adc_read"), "应生成 adc read");
        // 验证 async 状态机
        assert!(c.contains("grabAndPour_ctx"), "应生成 async 上下文");
        assert!(c.contains("grabAndPour_poll"), "应生成 poll 函数");
        // 验证调度器
        assert!(c.contains("xTaskCreate"), "应创建 FreeRTOS 任务");
        assert!(c.contains("_duanyan_task_0"), "应生成任务0");
        assert!(c.contains("_duanyan_task_1"), "应生成任务1");
        // 验证 app_main
        assert!(c.contains("void app_main(void)"), "应生成 app_main");
    }

    #[test]
    fn test_robot_arm_arduino_end_to_end() {
        let source = r#"
import { Servo } from "duanyan:motor";
import { Timer } from "duanyan:hw";

Servo.attach(9, 500, 2500);

function sweep() {
  Servo.write(9, 0);
  Servo.write(9, 180);
}

setInterval(sweep, 2000);
"#;
        let c = compile_to_c(source, CTarget::ArduinoAvr);
        assert!(c.contains("#include <Servo.h>"), "应包含 Servo.h");
        assert!(c.contains("void setup()"), "应生成 setup");
        assert!(c.contains("void loop()"), "应生成 loop");
        assert!(c.contains("_duanyan_scheduler_poll()"), "loop 中应调用调度器");
        assert!(c.contains("duanyan_servo_write(9, 0)"), "应调用 servo write");
    }

    // Phase 2.3: 对象 → C struct 测试
    #[test]
    fn test_object_to_struct_basic() {
        let source = r#"
function test() {
  let pos = { x: 10, y: 20, z: 30 };
  return pos.x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("struct _duanyan_obj_pos"), "应生成 struct 定义");
        assert!(c.contains("int32_t x;"), "struct 应包含 x 字段");
        assert!(c.contains("int32_t y;"), "struct 应包含 y 字段");
        assert!(c.contains("int32_t z;"), "struct 应包含 z 字段");
        assert!(c.contains(".x = 10"), "应生成初始化");
        assert!(c.contains(".y = 20"), "应生成初始化");
        assert!(c.contains(".z = 30"), "应生成初始化");
    }

    #[test]
    fn test_object_struct_no_malloc() {
        let source = r#"
function make() {
  let obj = { a: 1, b: 2 };
  obj.a = 99;
  return obj.a;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(!c.contains("malloc"), "不应包含 malloc");
        assert!(!c.contains("calloc"), "不应包含 calloc");
        assert!(c.contains("struct _duanyan_obj_obj"), "应生成 struct");
        assert!(c.contains("obj.a = 99"), "应生成属性赋值");
    }

    #[test]
    fn test_multiple_objects_different_structs() {
        let source = r#"
function test() {
  let pos = { x: 1, y: 2 };
  let vel = { vx: 3, vy: 4 };
  return pos.x + vel.vx;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("struct _duanyan_obj_pos"), "应生成 pos struct");
        assert!(c.contains("struct _duanyan_obj_vel"), "应生成 vel struct");
        assert!(c.contains("int32_t vx;"), "vel 应有 vx 字段");
    }

    // Phase 2.4: 数组边界检查测试
    #[test]
    fn test_array_fixed_length() {
        let source = r#"
function test() {
  let arr = [10, 20, 30, 40, 50];
  return arr[2];
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("int32_t arr[5]"), "应生成定长数组");
        assert!(c.contains("_duanyan_arr_get(arr, 5, 2)"), "应生成边界检查访问");
        assert!(c.contains("_duanyan_arr_get"), "应包含 helper 函数");
        assert!(c.contains("ARRAY_OOB"), "应包含越界提示");
    }

    #[test]
    fn test_array_bounds_no_malloc() {
        let source = r#"
function test() {
  let data = [1, 2, 3];
  let x = data[0];
  return x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(!c.contains("malloc"), "不应包含 malloc");
        assert!(c.contains("int32_t data[3]"), "应生成定长数组");
    }

    #[test]
    fn test_array_runtime_index_checked() {
        let source = r#"
function test(i) {
  let arr = [1, 2, 3, 4];
  return arr[i];
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("_duanyan_arr_get(arr, 4, i)"), "运行时索引应边界检查");
    }

    // Phase 3.3: 硬件定时器中断映射测试
    #[test]
    fn test_realtime_annotation_in_output() {
        let source = r#"
@realtime
function motorCtrl() {
  let x = 1;
  return x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("/* @realtime */"), "应包含 @realtime 标记");
        assert!(c.contains("int32_t motorCtrl(void)"), "应生成函数");
    }

    #[test]
    fn test_interrupt_esp32() {
        let source = r#"
@interrupt("TIM2", period_us: 100)
function controlLoop() {
  let x = 1;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("timer ISR init"), "应包含定时器初始化");
        assert!(c.contains("TIM2"), "应包含定时器名");
        assert!(c.contains("100us"), "应包含周期");
        assert!(c.contains("controlLoop"), "应包含回调函数名");
    }

    #[test]
    fn test_interrupt_stm32() {
        let source = r#"
@interrupt("TIM3", period_us: 50)
function pid_loop() {
  let err = 0;
}
"#;
        let c = compile_to_c(source, CTarget::Stm32);
        assert!(c.contains("STM32 HAL"), "应包含 STM32 HAL 注释");
        assert!(c.contains("TIM3"), "应包含定时器名");
        assert!(c.contains("Prescaler=71"), "应包含预分频");
    }

    #[test]
    fn test_interrupt_arduino() {
        let source = r#"
@interrupt("TIMER2", period_us: 1000)
function blink_isr() {
  let x = 1;
}
"#;
        let c = compile_to_c(source, CTarget::ArduinoAvr);
        assert!(c.contains("ISR(TIMER2_COMPA_vect)"), "应包含 Arduino ISR");
        assert!(c.contains("OCR2A"), "应包含 OCR2A 设置");
    }

    #[test]
    fn test_priority_realtime_combined() {
        let source = r#"
@priority(0)
@realtime(500)
function highest() {
  let x = 1;
  return x;
}
"#;
        let c = compile_to_c(source, CTarget::Esp32);
        assert!(c.contains("/* @realtime */"), "应包含 @realtime 标记");
        assert!(c.contains("int32_t highest(void)"), "应生成函数");
    }

    // ========== ESP32-S3 裸机 RGB 后端 (examples/rgb_on.js + rgb_rainbow.js) ==========

    #[test]
    fn test_bare_rgb_on() {
        // 示例 1: 点亮 RGB 灯 (与 DUANYAN-IDE/examples/rgb_on.js 同源)
        let source = r#"
import { RGB, Timer } from "duanyan:hw";

while (true) {
    RGB.setColor(255, 255, 255);
    Timer.delay(500);
}
"#;
        let c = compile_to_c(source, CTarget::Esp32S3Bare);
        // HAL 关键件: 入口/供电使能/WS2812 时序
        assert!(c.contains("void _start(void)"), "应生成裸机入口 _start");
        assert!(c.contains("duanyan_boot_init();"), "应调用 boot 初始化");
        assert!(c.contains("duanyan_pin_setup(47)"), "应使能 WS2812 供电脚 GPIO47");
        assert!(c.contains("duanyan_rgb_set(255, 255, 255);"), "RGB.setColor 应映射到 duanyan_rgb_set");
        assert!(c.contains("duanyan_delay_ms(500);"), "Timer.delay 应映射到 duanyan_delay_ms");
        assert!(c.contains("duanyan_ws2812_send"), "应包含 WS2812 驱动");
        // 不得引入 ESP-IDF 依赖
        assert!(!c.contains("freertos"), "裸机目标不得依赖 FreeRTOS");
    }

    #[test]
    fn test_bare_rgb_rainbow() {
        // 示例 2: 彩虹渐变 (与 DUANYAN-IDE/examples/rgb_rainbow.js 同源)
        let source = r#"
import { RGB, Timer } from "duanyan:hw";

let hue = 0;

while (true) {
    RGB.setHue(hue);
    hue = hue + 1;
    if (hue >= 256) {
        hue = 0;
    }
    Timer.delay(20);
}
"#;
        let c = compile_to_c(source, CTarget::Esp32S3Bare);
        assert!(c.contains("int32_t hue = 0;"), "应声明 hue 变量");
        assert!(c.contains("duanyan_rgb_hue(hue);"), "RGB.setHue 应映射到色轮函数");
        assert!(c.contains("hue = (hue + 1);"), "应生成自增");
        assert!(c.contains("if ((hue >= 256))"), "应生成回绕判断");
        assert!(c.contains("duanyan_delay_ms(20);"), "应生成 20ms 帧延时");
    }

    #[test]
    fn test_bare_console_log() {
        let source = r#"
console.log("boot ok");
console.log(42);
"#;
        let c = compile_to_c(source, CTarget::Esp32S3Bare);
        assert!(c.contains("duanyan_uart_puts(\"boot ok\\r\\n\");"), "字符串应走 UART");
        assert!(c.contains("duanyan_uart_num(42);"), "数字应走 UART");
        assert!(!c.contains("printf"), "裸机不得使用 printf");
    }

    #[test]
    fn test_bare_adc_read() {
        let source = r#"
let m = ADC.read(6);
"#;
        let c = compile_to_c(source, CTarget::Esp32S3Bare);
        assert!(c.contains("duanyan_adc_read(6)"), "ADC.read 应映射到 duanyan_adc_read");
        assert!(c.contains("DUANYAN_SENS_CLK_GATE |= (1u << 30);"), "应包含 saradc 时钟门关键步骤");
        assert!(c.contains("DUANYAN_SENS_MEAS1_CTRL2"), "应包含转换控制寄存器");
    }

    // -----------------------------------------------------------------------
    // 语言完备性批 2：Math 扩展 / continue / 字符串谓词
    // -----------------------------------------------------------------------

    #[test]
    fn test_math_sqrt_helper_injected() {
        let c = compile_to_c("let r = Math.sqrt(144);", CTarget::Esp32);
        assert!(c.contains("duanyan_isqrt(144)"));
        assert!(c.contains("static int32_t duanyan_isqrt(int32_t n)"), "应注入 isqrt helper");
    }

    #[test]
    fn test_math_pow_helper_injected() {
        let c = compile_to_c("let p = Math.pow(2, 10);", CTarget::Esp32);
        assert!(c.contains("duanyan_ipow(2, 10)"));
        assert!(c.contains("static int32_t duanyan_ipow(int32_t b, int32_t e)"), "应注入 ipow helper");
    }

    #[test]
    fn test_math_floor_is_identity_desugar() {
        // 整数语义：floor 直接返回参数，不产生任何库调用
        let c = compile_to_c("let x = Math.floor(7);", CTarget::Esp32);
        assert!(c.contains("int32_t x = 7;"), "floor(7) 应脱糖为 7");
        assert!(!c.contains("floor"), "不应出现 floor 库调用");
    }

    #[test]
    fn test_math_sign_desugar() {
        let c = compile_to_c("let v = 9; let s = Math.sign(v);", CTarget::Esp32);
        assert!(c.contains("((v > 0) - (v < 0))"), "sign 应脱糖为 (v>0)-(v<0)");
    }

    #[test]
    fn test_continue_break_c_output() {
        let c = compile_to_c(
            "for (let i = 0; i < 3; i++) { if (i == 1) { continue; } if (i == 2) { break; } }",
            CTarget::Esp32);
        assert!(c.contains("continue;"));
        assert!(c.contains("break;"));
    }

    #[test]
    fn test_string_predicates_c() {
        let c = compile_to_c(
            "let cmd = \"RUN\"; if (cmd.startsWith(\"RU\")) { console.log(1); }", CTarget::Esp32);
        assert!(c.contains("strncmp"), "startsWith 应生成 strncmp 表达式");
        assert!(c.contains("#include <string.h>"));
        let c2 = compile_to_c(
            "let cmd = \"RUN\"; if (cmd.endsWith(\"UN\")) { console.log(1); }", CTarget::Esp32);
        assert!(c2.contains("strcmp"), "endsWith 应生成 strcmp 后缀比较");
        let c3 = compile_to_c(
            "let cmd = \"RUN\"; if (cmd.includes(\"U\")) { console.log(1); }", CTarget::Esp32);
        assert!(c3.contains("strstr"), "includes 应生成 strstr 表达式");
    }
