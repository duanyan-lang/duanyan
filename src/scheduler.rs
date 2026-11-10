//! Phase 1.3: 多任务调度器
//!
//! 将 setInterval / setTimeout 编译为:
//! - ESP32: FreeRTOS xTaskCreate + 独立 C 函数
//! - Arduino: millis() 协作式调度器
//! - STM32: HAL 定时器 + 主循环调度

use crate::c_codegen::CTarget;

/// 调度任务描述
#[derive(Debug, Clone)]
pub struct SchedulerTask {
    /// 任务编号 (唯一)
    pub id: usize,
    /// 回调函数名 (已提升的 C 函数名)
    pub func_name: String,
    /// 间隔 (毫秒)
    pub interval_ms: u32,
    /// true = setInterval (重复), false = setTimeout (单次)
    pub repeating: bool,
    /// Phase 3.5: 优先级 (0=最高, 默认=5)
    pub priority: u8,
}

/// 调度器 — 收集所有 setInterval/setTimeout 并生成对应平台代码
#[derive(Debug)]
pub struct Scheduler {
    tasks: Vec<SchedulerTask>,
    next_id: usize,
}

impl Scheduler {
    pub fn new() -> Self {
        Self { tasks: Vec::new(), next_id: 0 }
    }

    /// 注册一个 setInterval 任务
    pub fn add_interval(&mut self, func_name: &str, interval_ms: u32) -> usize {
        self.add_interval_with_priority(func_name, interval_ms, 5)
    }

    /// Phase 3.5: 注册带优先级的 setInterval 任务
    pub fn add_interval_with_priority(&mut self, func_name: &str, interval_ms: u32, priority: u8) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(SchedulerTask {
            id,
            func_name: func_name.to_string(),
            interval_ms,
            repeating: true,
            priority,
        });
        id
    }

    /// 注册一个 setTimeout 任务
    pub fn add_timeout(&mut self, func_name: &str, delay_ms: u32) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(SchedulerTask {
            id,
            func_name: func_name.to_string(),
            interval_ms: delay_ms,
            repeating: false,
            priority: 5,
        });
        id
    }

    /// 是否有任务
    pub fn has_tasks(&self) -> bool {
        !self.tasks.is_empty()
    }

    /// 任务数量
    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    /// 生成任务包装函数 (在所有用户函数之后、main 之前输出)
    pub fn generate_task_wrappers(&self, target: &CTarget) -> String {
        match target {
            CTarget::Esp32 => self.generate_esp32_wrappers(),
            CTarget::ArduinoAvr | CTarget::ArduinoArm => self.generate_arduino_wrappers(),
            CTarget::Stm32 => self.generate_stm32_wrappers(),
            CTarget::RaspberryPi => self.generate_rpi_wrappers(),
            CTarget::Esp32S3Bare => String::new(), // 裸机无 RTOS, 不支持多任务
        }
    }

    /// 生成 main 函数中的初始化调用
    pub fn generate_task_init(&self, target: &CTarget) -> String {
        match target {
            CTarget::Esp32 => self.generate_esp32_init(),
            CTarget::ArduinoAvr | CTarget::ArduinoArm => self.generate_arduino_init(),
            CTarget::Stm32 => self.generate_stm32_init(),
            CTarget::RaspberryPi => self.generate_rpi_init(),
            CTarget::Esp32S3Bare => String::new(),
        }
    }

    // ========== ESP32: FreeRTOS 任务 ==========

    fn generate_esp32_wrappers(&self) -> String {
        let mut code = String::new();
        code.push_str("/* DUANYAN Scheduler — FreeRTOS tasks */\n");
        for task in &self.tasks {
            // 生成任务包装函数
            code.push_str(&format!(
                "static void _duanyan_task_{}(void* arg) {{\n", task.id
            ));
            code.push_str("    (void)arg;\n");
            if task.repeating {
                code.push_str(&format!(
                    "    for (;;) {{\n        {}();\n        vTaskDelay(pdMS_TO_TICKS({}));\n    }}\n",
                    task.func_name, task.interval_ms
                ));
            } else {
                code.push_str(&format!(
                    "    vTaskDelay(pdMS_TO_TICKS({}));\n    {}();\n    vTaskDelete(NULL);\n",
                    task.interval_ms, task.func_name
                ));
            }
            code.push_str("}\n\n");
        }
        code
    }

    fn generate_esp32_init(&self) -> String {
        let mut code = String::new();
        for task in &self.tasks {
            // xTaskCreate(函数, 名称, 栈大小, 参数, 优先级, 句柄)
            let stack_size = 2048; // 默认栈大小
            // Phase 3.5: 使用任务优先级 (FreeRTOS: 数字越大优先级越高, 所以反转)
            let rt_priority = if task.priority == 0 { 5 } else { (6 - task.priority).min(5).max(1) };
            code.push_str(&format!(
                "    xTaskCreate(_duanyan_task_{}, \"task_{}\", {}, NULL, {}, NULL);\n",
                task.id, task.id, stack_size, rt_priority
            ));
        }
        code
    }

    // ========== Arduino: millis() 协作式调度 ==========

    fn generate_arduino_wrappers(&self) -> String {
        let mut code = String::new();
        code.push_str("/* DUANYAN Scheduler — millis() cooperative */\n");
        // 生成每个任务的上次执行时间变量
        for task in &self.tasks {
            code.push_str(&format!(
                "static uint32_t _duanyan_task_{}_last = 0;\n", task.id
            ));
            if !task.repeating {
                code.push_str(&format!(
                    "static uint8_t _duanyan_task_{}_done = 0;\n", task.id
                ));
            }
        }
        code.push('\n');
        // 生成调度器 poll 函数 (在 loop 中调用)
        code.push_str("static inline void _duanyan_scheduler_poll(void) {\n");
        code.push_str("    uint32_t now = millis();\n");
        for task in &self.tasks {
            if task.repeating {
                code.push_str(&format!(
                    "    if (now - _duanyan_task_{}_last >= {}) {{ {}(); _duanyan_task_{}_last = now; }}\n",
                    task.id, task.interval_ms, task.func_name, task.id
                ));
            } else {
                code.push_str(&format!(
                    "    if (!_duanyan_task_{}_done && now - _duanyan_task_{}_last >= {}) {{ {}(); _duanyan_task_{}_done = 1; }}\n",
                    task.id, task.id, task.interval_ms, task.func_name, task.id
                ));
            }
        }
        code.push_str("}\n\n");
        code
    }

    fn generate_arduino_init(&self) -> String {
        // Arduino: 在 setup 中初始化时间戳
        let mut code = String::new();
        for task in &self.tasks {
            code.push_str(&format!(
                "    _duanyan_task_{}_last = millis();\n", task.id
            ));
        }
        code
    }

    /// Arduino: 生成 loop 中的调度调用 (附加到 loop 末尾)
    pub fn generate_arduino_loop_call(&self) -> String {
        if self.tasks.is_empty() {
            return String::new();
        }
        "    _duanyan_scheduler_poll();\n".to_string()
    }

    // ========== STM32: 主循环 + HAL 延时 ==========

    fn generate_stm32_wrappers(&self) -> String {
        let mut code = String::new();
        code.push_str("/* DUANYAN Scheduler — main loop cooperative */\n");
        for task in &self.tasks {
            code.push_str(&format!(
                "static uint32_t _duanyan_task_{}_last = 0;\n", task.id
            ));
            if !task.repeating {
                code.push_str(&format!(
                    "static uint8_t _duanyan_task_{}_done = 0;\n", task.id
                ));
            }
        }
        code.push('\n');
        code.push_str("static inline void _duanyan_scheduler_poll(void) {\n");
        code.push_str("    uint32_t now = HAL_GetTick();\n");
        for task in &self.tasks {
            if task.repeating {
                code.push_str(&format!(
                    "    if (now - _duanyan_task_{}_last >= {}) {{ {}(); _duanyan_task_{}_last = now; }}\n",
                    task.id, task.interval_ms, task.func_name, task.id
                ));
            } else {
                code.push_str(&format!(
                    "    if (!_duanyan_task_{}_done && now - _duanyan_task_{}_last >= {}) {{ {}(); _duanyan_task_{}_done = 1; }}\n",
                    task.id, task.id, task.interval_ms, task.func_name, task.id
                ));
            }
        }
        code.push_str("}\n\n");
        code
    }

    fn generate_stm32_init(&self) -> String {
        // STM32: 主循环中调用 poll，init 无需额外操作
        String::new()
    }

    // ========== RaspberryPi: pthread / usleep ==========

    fn generate_rpi_wrappers(&self) -> String {
        let mut code = String::new();
        code.push_str("/* DUANYAN Scheduler — pthread tasks */\n");
        code.push_str("#include <pthread.h>\n\n");
        for task in &self.tasks {
            code.push_str(&format!(
                "static void* _duanyan_task_{}(void* arg) {{\n", task.id
            ));
            code.push_str("    (void)arg;\n");
            if task.repeating {
                code.push_str(&format!(
                    "    for (;;) {{ {}(); usleep({} * 1000); }}\n",
                    task.func_name, task.interval_ms
                ));
            } else {
                code.push_str(&format!(
                    "    usleep({} * 1000);\n    {}();\n    return NULL;\n",
                    task.interval_ms, task.func_name
                ));
            }
            code.push_str("}\n\n");
        }
        code
    }

    fn generate_rpi_init(&self) -> String {
        let mut code = String::new();
        for task in &self.tasks {
            code.push_str(&format!(
                "    {{ pthread_t t; pthread_create(&t, NULL, _duanyan_task_{}, NULL); pthread_detach(t); }}\n",
                task.id
            ));
        }
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scheduler_add_interval() {
        let mut sched = Scheduler::new();
        let id = sched.add_interval("readSensors", 10);
        assert_eq!(id, 0);
        assert_eq!(sched.task_count(), 1);
        assert!(sched.has_tasks());
    }

    #[test]
    fn test_scheduler_add_timeout() {
        let mut sched = Scheduler::new();
        let id = sched.add_timeout("initOnce", 5000);
        assert_eq!(id, 0);
        assert!(!sched.tasks[0].repeating);
    }

    #[test]
    fn test_scheduler_multiple_tasks() {
        let mut sched = Scheduler::new();
        sched.add_interval("task_a", 10);
        sched.add_interval("task_b", 100);
        sched.add_timeout("task_c", 1000);
        assert_eq!(sched.task_count(), 3);
    }

    #[test]
    fn test_esp32_freertos_generation() {
        let mut sched = Scheduler::new();
        sched.add_interval("readSensors", 10);
        sched.add_interval("updateMotors", 1);

        let wrappers = sched.generate_task_wrappers(&CTarget::Esp32);
        assert!(wrappers.contains("_duanyan_task_0"), "应生成任务0包装");
        assert!(wrappers.contains("_duanyan_task_1"), "应生成任务1包装");
        assert!(wrappers.contains("readSensors()"), "应调用回调函数");
        assert!(wrappers.contains("vTaskDelay(pdMS_TO_TICKS(10))"), "应包含延时期望值");
        assert!(wrappers.contains("for (;;)"), "重复任务应有无限循环");

        let init = sched.generate_task_init(&CTarget::Esp32);
        assert!(init.contains("xTaskCreate(_duanyan_task_0"), "应创建任务0");
        assert!(init.contains("xTaskCreate(_duanyan_task_1"), "应创建任务1");
    }

    #[test]
    fn test_esp32_timeout_generation() {
        let mut sched = Scheduler::new();
        sched.add_timeout("initOnce", 3000);

        let wrappers = sched.generate_task_wrappers(&CTarget::Esp32);
        assert!(wrappers.contains("vTaskDelay(pdMS_TO_TICKS(3000))"), "单次任务应延时");
        assert!(wrappers.contains("vTaskDelete(NULL)"), "单次任务执行后应自删除");
        assert!(!wrappers.contains("for (;;)"), "单次任务不应有循环");
    }

    #[test]
    fn test_arduino_millis_generation() {
        let mut sched = Scheduler::new();
        sched.add_interval("blink", 500);

        let wrappers = sched.generate_task_wrappers(&CTarget::ArduinoAvr);
        assert!(wrappers.contains("_duanyan_task_0_last"), "应生成时间戳变量");
        assert!(wrappers.contains("_duanyan_scheduler_poll"), "应生成 poll 函数");
        assert!(wrappers.contains("millis()"), "应使用 millis()");
        assert!(wrappers.contains("blink()"), "应调用回调");
    }

    #[test]
    fn test_stm32_hal_generation() {
        let mut sched = Scheduler::new();
        sched.add_interval("control", 1);

        let wrappers = sched.generate_task_wrappers(&CTarget::Stm32);
        assert!(wrappers.contains("HAL_GetTick()"), "应使用 HAL_GetTick");
        assert!(wrappers.contains("control()"), "应调用回调");
    }

    #[test]
    fn test_rpi_pthread_generation() {
        let mut sched = Scheduler::new();
        sched.add_interval("poll_net", 100);

        let wrappers = sched.generate_task_wrappers(&CTarget::RaspberryPi);
        assert!(wrappers.contains("pthread.h"), "应包含 pthread.h");
        assert!(wrappers.contains("usleep(100 * 1000)"), "应使用 usleep");
        assert!(wrappers.contains("poll_net()"), "应调用回调");

        let init = sched.generate_task_init(&CTarget::RaspberryPi);
        assert!(init.contains("pthread_create"), "应创建线程");
    }

    #[test]
    fn test_arduino_loop_call() {
        let mut sched = Scheduler::new();
        sched.add_interval("tick", 50);
        let loop_call = sched.generate_arduino_loop_call();
        assert!(loop_call.contains("_duanyan_scheduler_poll()"), "loop 中应调用 poll");
    }

    #[test]
    fn test_empty_scheduler() {
        let sched = Scheduler::new();
        assert!(!sched.has_tasks());
        assert_eq!(sched.task_count(), 0);
        assert_eq!(sched.generate_arduino_loop_call(), "");
    }

    // Phase 3.5: 优先级调度器测试
    #[test]
    fn test_priority_interval() {
        let mut sched = Scheduler::new();
        sched.add_interval_with_priority("highPriority", 1, 0);  // 最高优先级
        sched.add_interval_with_priority("lowPriority", 100, 2); // 低优先级

        let init = sched.generate_task_init(&CTarget::Esp32);
        // priority 0 → FreeRTOS 5 (最高)
        assert!(init.contains("xTaskCreate(_duanyan_task_0, \"task_0\", 2048, NULL, 5, NULL)"),
            "优先级 0 应映射为 FreeRTOS 5");
        // priority 2 → FreeRTOS 4
        assert!(init.contains("xTaskCreate(_duanyan_task_1, \"task_1\", 2048, NULL, 4, NULL)"),
            "优先级 2 应映射为 FreeRTOS 4");
    }

    #[test]
    fn test_default_priority() {
        let mut sched = Scheduler::new();
        sched.add_interval("normal", 50); // 默认优先级 5

        assert_eq!(sched.tasks[0].priority, 5, "默认优先级应为 5");
    }
}
