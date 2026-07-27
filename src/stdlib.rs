//! 系统标准库模块
//!
//! Phase C: setTimeout/setInterval/console 系统库
//! 静态编译为 COUNPRE64 机器码

use std::collections::HashMap;

/// 定时器 ID 类型
pub type TimerId = u32;

/// 定时器条目
#[derive(Debug, Clone)]
pub struct TimerEntry {
    pub id: TimerId,
    /// 延迟毫秒数
    pub delay_ms: u32,
    /// 已过去时间
    pub elapsed_ms: u32,
    /// 是否是 setInterval
    pub repeating: bool,
    /// 回调函数地址 (编译后的 PC)
    pub callback_pc: usize,
    /// 是否已取消
    pub cancelled: bool,
}

/// 定时器管理器
#[derive(Debug)]
pub struct TimerManager {
    timers: HashMap<TimerId, TimerEntry>,
    next_id: TimerId,
    /// 当前系统时间 (ms)
    current_time_ms: u64,
}

impl TimerManager {
    pub fn new() -> Self {
        TimerManager {
            timers: HashMap::new(),
            next_id: 1,
            current_time_ms: 0,
        }
    }

    /// setTimeout(callback, delay)
    pub fn set_timeout(&mut self, callback_pc: usize, delay_ms: u32) -> TimerId {
        let id = self.next_id;
        self.next_id += 1;

        self.timers.insert(id, TimerEntry {
            id,
            delay_ms,
            elapsed_ms: 0,
            repeating: false,
            callback_pc,
            cancelled: false,
        });

        id
    }

    /// setInterval(callback, interval)
    pub fn set_interval(&mut self, callback_pc: usize, interval_ms: u32) -> TimerId {
        let id = self.next_id;
        self.next_id += 1;

        self.timers.insert(id, TimerEntry {
            id,
            delay_ms: interval_ms,
            elapsed_ms: 0,
            repeating: true,
            callback_pc,
            cancelled: false,
        });

        id
    }

    /// clearTimeout(id) / clearInterval(id)
    pub fn clear(&mut self, id: TimerId) {
        if let Some(timer) = self.timers.get_mut(&id) {
            timer.cancelled = true;
        }
    }

    /// 推进时间，返回需要触发的回调
    pub fn tick(&mut self, elapsed_ms: u32) -> Vec<usize> {
        self.current_time_ms += elapsed_ms as u64;
        let mut callbacks = Vec::new();

        let mut to_remove = Vec::new();

        for (id, timer) in self.timers.iter_mut() {
            if timer.cancelled {
                to_remove.push(*id);
                continue;
            }

            timer.elapsed_ms += elapsed_ms;

            if timer.elapsed_ms >= timer.delay_ms {
                callbacks.push(timer.callback_pc);

                if timer.repeating {
                    // setInterval: 重置计时器
                    timer.elapsed_ms = timer.elapsed_ms.saturating_sub(timer.delay_ms);
                } else {
                    // setTimeout: 执行后移除
                    to_remove.push(*id);
                }
            }
        }

        for id in to_remove {
            self.timers.remove(&id);
        }

        callbacks
    }

    /// 获取活跃定时器数量
    pub fn active_count(&self) -> usize {
        self.timers.values().filter(|t| !t.cancelled).count()
    }

    /// 获取当前时间
    pub fn current_time(&self) -> u64 {
        self.current_time_ms
    }
}

/// Console 系统库
pub struct ConsoleLib {
    output_buffer: String,
}

impl ConsoleLib {
    pub fn new() -> Self {
        ConsoleLib {
            output_buffer: String::new(),
        }
    }

    /// console.log(args...)
    pub fn log(&mut self, args: &[ConsoleValue]) {
        let line: Vec<String> = args.iter().map(|v| v.to_string()).collect();
        let output = line.join(" ");
        self.output_buffer.push_str(&output);
        self.output_buffer.push('\n');
    }

    /// console.error(args...)
    pub fn error(&mut self, args: &[ConsoleValue]) {
        let line: Vec<String> = args.iter().map(|v| v.to_string()).collect();
        let output = format!("[ERROR] {}", line.join(" "));
        self.output_buffer.push_str(&output);
        self.output_buffer.push('\n');
    }

    /// 获取并清空输出缓冲
    pub fn flush(&mut self) -> String {
        let output = self.output_buffer.clone();
        self.output_buffer.clear();
        output
    }
}

/// console 值类型
#[derive(Debug, Clone)]
pub enum ConsoleValue {
    Number(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
    Undefined,
    Array(Vec<ConsoleValue>),
    Object(Vec<(String, ConsoleValue)>),
}

impl std::fmt::Display for ConsoleValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{}", n),
            Self::Float(n) => write!(f, "{}", n),
            Self::String(s) => write!(f, "{}", s),
            Self::Bool(b) => write!(f, "{}", b),
            Self::Null => write!(f, "null"),
            Self::Undefined => write!(f, "undefined"),
            Self::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                write!(f, "[{}]", items.join(", "))
            }
            Self::Object(props) => {
                let items: Vec<String> = props.iter()
                    .map(|(k, v)| format!("{}: {}", k, v))
                    .collect();
                write!(f, "{{ {} }}", items.join(", "))
            }
        }
    }
}

/// Math 内建函数（已有编译器支持，这里提供运行时模拟）
pub struct MathLib;

impl MathLib {
    pub fn max(a: i64, b: i64) -> i64 {
        if a >= b { a } else { b }
    }

    pub fn min(a: i64, b: i64) -> i64 {
        if a <= b { a } else { b }
    }

    pub fn abs(a: i64) -> i64 {
        a.abs()
    }

    pub fn floor(a: f64) -> i64 {
        a.floor() as i64
    }

    pub fn ceil(a: f64) -> i64 {
        a.ceil() as i64
    }

    pub fn round(a: f64) -> i64 {
        a.round() as i64
    }
}

/// 系统库集成管理器
pub struct SystemLib {
    pub timer: TimerManager,
    pub console: ConsoleLib,
}

impl SystemLib {
    pub fn new() -> Self {
        SystemLib {
            timer: TimerManager::new(),
            console: ConsoleLib::new(),
        }
    }
}

// === Phase v1.5: 异步任务调度器 ===

/// 异步任务状态
#[derive(Debug, Clone, PartialEq)]
pub enum AsyncTaskState {
    /// 正在运行
    Running,
    /// 等待异步操作完成
    Pending,
    /// 已完成，带返回值
    Completed(i64),
    /// 失败
    Failed(String),
}

/// 异步任务
#[derive(Debug, Clone)]
pub struct AsyncTask {
    pub id: u32,
    /// poll 函数地址
    pub poll_fn_addr: usize,
    /// 上下文结构体地址
    pub ctx_addr: usize,
    /// 当前状态
    pub state: AsyncTaskState,
}

/// 异步任务调度器 (Phase v1.5)
///
/// 管理所有异步任务的生命周期，通过事件循环驱动。
/// 最多同时运行 16 个异步任务，总上下文开销 < 4KB。
pub struct AsyncScheduler {
    tasks: Vec<AsyncTask>,
    next_id: u32,
    /// 最大并发任务数
    max_tasks: usize,
}

impl AsyncScheduler {
    pub fn new() -> Self {
        AsyncScheduler {
            tasks: Vec::new(),
            next_id: 1,
            max_tasks: 16,
        }
    }

    /// 注册新的异步任务
    pub fn spawn(&mut self, poll_fn_addr: usize, ctx_addr: usize) -> Option<u32> {
        if self.tasks.len() >= self.max_tasks {
            return None; // 超过最大并发数
        }
        let id = self.next_id;
        self.next_id += 1;
        self.tasks.push(AsyncTask {
            id,
            poll_fn_addr,
            ctx_addr,
            state: AsyncTaskState::Running,
        });
        Some(id)
    }

    /// 轮询所有任务，返回已完成的任务 ID 列表
    pub fn poll_all(&mut self) -> Vec<u32> {
        let mut completed = Vec::new();
        for task in &mut self.tasks {
            match task.state {
                AsyncTaskState::Completed(_) | AsyncTaskState::Failed(_) => {
                    completed.push(task.id);
                }
                _ => {}
            }
        }
        // 移除已完成的任务
        self.tasks.retain(|t| !completed.contains(&t.id));
        completed
    }

    /// 获取活跃任务数
    pub fn active_count(&self) -> usize {
        self.tasks.iter().filter(|t| {
            matches!(t.state, AsyncTaskState::Running | AsyncTaskState::Pending)
        }).count()
    }

    /// 是否所有任务都已完成
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// 获取所有任务的 poll 函数地址 (用于事件循环执行)
    pub fn get_poll_addrs(&self) -> Vec<(usize, usize)> {
        self.tasks.iter()
            .filter(|t| matches!(t.state, AsyncTaskState::Running | AsyncTaskState::Pending))
            .map(|t| (t.poll_fn_addr, t.ctx_addr))
            .collect()
    }

    /// 更新任务状态
    pub fn update_state(&mut self, id: u32, state: AsyncTaskState) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
            task.state = state;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_timeout() {
        let mut timer = TimerManager::new();
        let id = timer.set_timeout(100, 1000);
        assert_eq!(timer.active_count(), 1);

        // 500ms 后不应触发
        let callbacks = timer.tick(500);
        assert!(callbacks.is_empty());

        // 再 500ms 后应触发
        let callbacks = timer.tick(500);
        assert_eq!(callbacks.len(), 1);
        assert_eq!(callbacks[0], 100);

        // setTimeout 执行后应移除
        assert_eq!(timer.active_count(), 0);
    }

    #[test]
    fn test_set_interval() {
        let mut timer = TimerManager::new();
        let id = timer.set_interval(200, 500);
        assert_eq!(timer.active_count(), 1);

        // 500ms 后应触发第一次
        let callbacks = timer.tick(500);
        assert_eq!(callbacks.len(), 1);
        assert_eq!(callbacks[0], 200);
        assert_eq!(timer.active_count(), 1);  // setInterval 不移除

        // 再 500ms 后应触发第二次
        let callbacks = timer.tick(500);
        assert_eq!(callbacks.len(), 1);

        // 清除定时器
        timer.clear(id);
        assert_eq!(timer.active_count(), 0);
    }

    #[test]
    fn test_console_log() {
        let mut console = ConsoleLib::new();
        console.log(&[ConsoleValue::String("Hello".to_string()), ConsoleValue::Number(42)]);
        let output = console.flush();
        assert_eq!(output, "Hello 42\n");
    }

    #[test]
    fn test_math_functions() {
        assert_eq!(MathLib::max(3, 7), 7);
        assert_eq!(MathLib::min(3, 7), 3);
        assert_eq!(MathLib::abs(-5), 5);
        assert_eq!(MathLib::floor(3.7), 3);
        assert_eq!(MathLib::ceil(3.2), 4);
        assert_eq!(MathLib::round(3.5), 4);
    }

    // Phase v1.5: AsyncScheduler 测试
    #[test]
    fn test_async_scheduler_spawn() {
        let mut sched = AsyncScheduler::new();
        let id = sched.spawn(100, 0x800);
        assert_eq!(id, Some(1));
        assert_eq!(sched.active_count(), 1);
    }

    #[test]
    fn test_async_scheduler_max_tasks() {
        let mut sched = AsyncScheduler::new();
        for i in 0..16 {
            assert!(sched.spawn(i * 10, 0x800 + i as usize * 64).is_some());
        }
        // 第 17 个应失败
        assert_eq!(sched.spawn(200, 0xC00), None);
    }

    #[test]
    fn test_async_scheduler_poll_complete() {
        let mut sched = AsyncScheduler::new();
        let id = sched.spawn(100, 0x800).unwrap();
        sched.update_state(id, AsyncTaskState::Completed(42));
        let completed = sched.poll_all();
        assert_eq!(completed, vec![id]);
        assert!(sched.is_empty());
    }

    #[test]
    fn test_async_scheduler_pending() {
        let mut sched = AsyncScheduler::new();
        let id = sched.spawn(100, 0x800).unwrap();
        sched.update_state(id, AsyncTaskState::Pending);
        let completed = sched.poll_all();
        assert!(completed.is_empty());
        assert_eq!(sched.active_count(), 1);
    }
}
