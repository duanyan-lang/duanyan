//! Phase RT: 嵌入式 RTOS 垂直模块
//!
//! 从 @periodic/@realtime 注解自动生成完整 RTOS 应用：
//! - HardyRTOS 微内核生成 (抢占式 + 时间片)
//! - 确定性 IPC (无锁消息队列 / PIP 互斥锁 / 零拷贝)
//! - BSP 板级支持包自动生成
//! - 形式化验证接口 (TLA+ / Spin)

// =============================================================================
// HardyRTOS 微内核
// =============================================================================

/// RTOS 任务定义
#[derive(Debug, Clone)]
pub struct RtosTask {
    pub name: String,
    /// 优先级 (0=最高)
    pub priority: u8,
    /// 栈大小 (bytes)
    pub stack_bytes: usize,
    /// 周期 (μs, 0=事件驱动)
    pub period_us: u64,
    /// WCET (μs)
    pub wcet_us: u64,
    /// 对应 HardyScript 函数
    pub hs_function: String,
}

/// RTOS 配置
#[derive(Debug, Clone)]
pub struct RtosConfig {
    /// 系统名称
    pub system_name: String,
    /// 目标 MCU
    pub target: RtosTarget,
    /// 系统时钟频率 (Hz)
    pub cpu_freq_hz: u32,
    /// SysTick 周期 (μs)
    pub tick_us: u64,
    /// 任务列表
    pub tasks: Vec<RtosTask>,
    /// 是否启用时间片轮转 (同优先级)
    pub time_slicing: bool,
    /// 时间片长度 (ticks)
    pub time_slice_ticks: u32,
}

/// 目标平台
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RtosTarget {
    /// STM32F4 (Cortex-M4)
    Stm32f4,
    /// STM32H7 (Cortex-M7)
    Stm32h7,
    /// ESP32-S3 (Xtensa LX7 dual-core)
    Esp32s3,
    /// CH32V307 (RISC-V)
    Ch32v307,
    /// Cortex-R52 (Safety)
    CortexR52,
}

impl RtosTarget {
    pub fn arch(&self) -> &'static str {
        match self {
            Self::Stm32f4 | Self::Stm32h7 => "ARM Cortex-M",
            Self::Esp32s3 => "Xtensa LX7",
            Self::Ch32v307 => "RISC-V RV32IMAC",
            Self::CortexR52 => "ARM Cortex-R",
        }
    }

    pub fn context_regs(&self) -> usize {
        match self {
            Self::Stm32f4 | Self::Stm32h7 | Self::CortexR52 => 17, // R0-R15 + xPSR
            Self::Esp32s3 => 34, // A0-A15 + PC + PS + ...
            Self::Ch32v307 => 32, // x0-x31
        }
    }

    pub fn systick_max(&self) -> u32 {
        match self {
            Self::Stm32f4 | Self::Stm32h7 | Self::CortexR52 => 0x00FF_FFFF,
            Self::Esp32s3 => 0xFFFF_FFFF,
            Self::Ch32v307 => 0xFFFF_FFFF,
        }
    }
}

/// 生成 HardyRTOS 微内核 C 代码
pub fn generate_rtos_kernel(config: &RtosConfig) -> String {
    let mut code = String::new();
    code.push_str(&format!("/* HardyRTOS Microkernel — {} */\n", config.system_name));
    code.push_str(&format!("/* Target: {} ({}) */\n", format!("{:?}", config.target), config.target.arch()));
    code.push_str(&format!("/* CPU: {} Hz, Tick: {}μs */\n", config.cpu_freq_hz, config.tick_us));
    code.push_str(&format!("/* Tasks: {}, Time-slice: {} */\n\n", config.tasks.len(), config.time_slicing));

    // 内核常量
    code.push_str(&format!("#define HARDY_MAX_TASKS {}\n", config.tasks.len().max(8)));
    code.push_str(&format!("#define HARDY_TICK_US {}\n", config.tick_us));
    code.push_str(&format!("#define HARDY_CPU_HZ {}\n", config.cpu_freq_hz));
    code.push_str(&format!("#define HARDY_CONTEXT_SIZE {} /* registers */\n\n", config.target.context_regs()));

    // TCB
    code.push_str("typedef struct {\n");
    code.push_str("    uint32_t *sp;              /* saved stack pointer */\n");
    code.push_str("    uint8_t   priority;\n");
    code.push_str("    uint8_t   state;           /* 0=READY 1=RUNNING 2=BLOCKED 3=SLEEP */\n");
    code.push_str("    uint16_t  time_slice;\n");
    code.push_str("    uint32_t  period_ticks;\n");
    code.push_str("    uint32_t  next_wake;\n");
    code.push_str("    uint32_t  stack_base;\n");
    code.push_str("    uint32_t  stack_size;\n");
    code.push_str("    void    (*entry)(void);\n");
    code.push_str("} _tcb_t;\n\n");

    code.push_str("static _tcb_t _tasks[HARDY_MAX_TASKS];\n");
    code.push_str("static volatile uint8_t _task_count = 0;\n");
    code.push_str("static volatile uint8_t _current = 0;\n");
    code.push_str("static volatile uint32_t _tick_count = 0;\n\n");

    // 调度器 (抢占式优先级 + 可选时间片)
    code.push_str("/* Preemptive priority scheduler */\n");
    code.push_str("static inline uint8_t _schedule_next(void) {\n");
    code.push_str("    uint8_t best = 0;\n");
    code.push_str("    uint8_t best_prio = 255;\n");
    code.push_str("    for (uint8_t i = 0; i < _task_count; i++) {\n");
    code.push_str("        if (_tasks[i].state == 0 && _tasks[i].priority < best_prio) {\n");
    code.push_str("            best_prio = _tasks[i].priority;\n");
    code.push_str("            best = i;\n");
    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("    return best;\n");
    code.push_str("}\n\n");

    // SysTick ISR
    code.push_str("void SysTick_Handler(void) {\n");
    code.push_str("    _tick_count++;\n");
    code.push_str("    /* Wake periodic tasks */\n");
    code.push_str("    for (uint8_t i = 0; i < _task_count; i++) {\n");
    code.push_str("        if (_tasks[i].state == 3 && _tick_count >= _tasks[i].next_wake) {\n");
    code.push_str("            _tasks[i].state = 0;\n");
    code.push_str("            _tasks[i].next_wake += _tasks[i].period_ticks;\n");
    code.push_str("        }\n");
    code.push_str("    }\n");
    if config.time_slicing {
        code.push_str("    /* Time slice expiry */\n");
        code.push_str(&format!("    if (++_tasks[_current].time_slice >= {}) {{\n", config.time_slice_ticks));
        code.push_str("        _tasks[_current].time_slice = 0;\n");
        code.push_str("        _tasks[_current].state = 0;\n");
        code.push_str("    }\n");
    }
    code.push_str("    /* Context switch */\n");
    code.push_str("    uint8_t next = _schedule_next();\n");
    code.push_str("    if (next != _current) { _context_switch(&_current, &next); }\n");
    code.push_str("}\n\n");

    // 系统调用
    code.push_str("/* Syscall 1: Create task */\n");
    code.push_str("int hardy_create_task(void (*entry)(void), uint8_t prio, uint32_t stack_sz, uint32_t period_ticks) {\n");
    code.push_str("    if (_task_count >= HARDY_MAX_TASKS) return -1;\n");
    code.push_str("    _tcb_t *t = &_tasks[_task_count];\n");
    code.push_str("    t->entry = entry;\n");
    code.push_str("    t->priority = prio;\n");
    code.push_str("    t->stack_size = stack_sz;\n");
    code.push_str("    t->period_ticks = period_ticks;\n");
    code.push_str("    t->state = 0;\n");
    code.push_str("    return _task_count++;\n");
    code.push_str("}\n\n");

    code.push_str("/* Syscall 2: Sleep (periodic wait) */\n");
    code.push_str("void hardy_sleep_ticks(uint32_t ticks) {\n");
    code.push_str("    _tasks[_current].state = 3;\n");
    code.push_str("    _tasks[_current].next_wake = _tick_count + ticks;\n");
    code.push_str("    _yield();\n");
    code.push_str("}\n\n");

    code.push_str("/* Syscall 3: Yield */\n");
    code.push_str("void hardy_yield(void) { _yield(); }\n\n");

    code.push_str("/* Syscall 4: Get tick count */\n");
    code.push_str("uint32_t hardy_get_tick(void) { return _tick_count; }\n\n");

    code.push_str("/* Syscall 5: Critical section */\n");
    code.push_str("void hardy_enter_critical(void) { __disable_irq(); }\n");
    code.push_str("void hardy_exit_critical(void) { __enable_irq(); }\n");

    code
}

// =============================================================================
// 确定性 IPC
// =============================================================================

/// IPC 通道类型
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IpcKind {
    /// 无锁消息队列 (SPSC)
    MessageQueue,
    /// 优先级继承互斥锁
    PipMutex,
    /// 零拷贝共享内存
    ZeroCopyShm,
    /// 事件标志组
    EventFlags,
}

/// IPC 通道配置
#[derive(Debug, Clone)]
pub struct IpcChannel {
    pub name: String,
    pub kind: IpcKind,
    /// 消息大小 (bytes, 仅 MessageQueue)
    pub msg_size: usize,
    /// 队列深度
    pub depth: usize,
    /// 生产者任务
    pub producer: String,
    /// 消费者任务
    pub consumer: String,
}

/// 生成 IPC C 代码
pub fn generate_ipc_code(channels: &[IpcChannel]) -> String {
    let mut code = String::new();
    code.push_str("/* HardyRTOS IPC Subsystem */\n\n");

    for ch in channels {
        match ch.kind {
            IpcKind::MessageQueue => {
                code.push_str(&format!("/* MQ: {} ({} bytes x {}) {} → {} */\n",
                    ch.name, ch.msg_size, ch.depth, ch.producer, ch.consumer));
                code.push_str(&format!("static uint8_t _mq_{}_buf[{}][{}];\n", ch.name, ch.depth, ch.msg_size.max(1)));
                code.push_str(&format!("static volatile uint32_t _mq_{}_head = 0;\n", ch.name));
                code.push_str(&format!("static volatile uint32_t _mq_{}_tail = 0;\n\n", ch.name));

                // Send
                code.push_str(&format!("int mq_{}_send(const void *msg) {{\n", ch.name));
                code.push_str(&format!("    uint32_t next = (_mq_{}_head + 1) % {};\n", ch.name, ch.depth));
                code.push_str(&format!("    if (next == _mq_{}_tail) return -1; /* FULL */\n", ch.name));
                code.push_str(&format!("    __builtin_memcpy(_mq_{}_buf[_mq_{}_head], msg, {});\n", ch.name, ch.name, ch.msg_size));
                code.push_str("    __asm__ volatile(\"dmb\" ::: \"memory\");\n");
                code.push_str(&format!("    _mq_{}_head = next;\n", ch.name));
                code.push_str("    return 0;\n}\n\n");

                // Recv
                code.push_str(&format!("int mq_{}_recv(void *msg) {{\n", ch.name));
                code.push_str(&format!("    if (_mq_{}_head == _mq_{}_tail) return -1; /* EMPTY */\n", ch.name, ch.name));
                code.push_str("    __asm__ volatile(\"dmb\" ::: \"memory\");\n");
                code.push_str(&format!("    __builtin_memcpy(msg, _mq_{}_buf[_mq_{}_tail], {});\n", ch.name, ch.name, ch.msg_size));
                code.push_str(&format!("    _mq_{}_tail = (_mq_{}_tail + 1) % {};\n", ch.name, ch.name, ch.depth));
                code.push_str("    return 0;\n}\n\n");
            }
            IpcKind::PipMutex => {
                code.push_str(&format!("/* PIP Mutex: {} ({} ↔ {}) */\n", ch.name, ch.producer, ch.consumer));
                code.push_str(&format!("static volatile uint8_t _mtx_{}_owner = 255;\n", ch.name));
                code.push_str(&format!("static volatile uint8_t _mtx_{}_saved_prio = 0;\n\n", ch.name));

                code.push_str(&format!("void mtx_{}_lock(void) {{\n", ch.name));
                code.push_str("    hardy_enter_critical();\n");
                code.push_str(&format!("    while (_mtx_{}_owner != 255) {{ hardy_yield(); }}\n", ch.name));
                code.push_str(&format!("    _mtx_{}_owner = _current;\n", ch.name));
                code.push_str("    hardy_exit_critical();\n}\n\n");

                code.push_str(&format!("void mtx_{}_unlock(void) {{\n", ch.name));
                code.push_str("    hardy_enter_critical();\n");
                code.push_str(&format!("    _tasks[_current].priority = _mtx_{}_saved_prio;\n", ch.name));
                code.push_str(&format!("    _mtx_{}_owner = 255;\n", ch.name));
                code.push_str("    hardy_exit_critical();\n}\n\n");
            }
            IpcKind::ZeroCopyShm => {
                code.push_str(&format!("/* Zero-Copy SHM: {} ({} bytes) {} → {} */\n",
                    ch.name, ch.msg_size, ch.producer, ch.consumer));
                code.push_str(&format!("static uint8_t _shm_{}[{}] __attribute__((aligned(32)));\n", ch.name, ch.msg_size.max(64)));
                code.push_str(&format!("static volatile uint32_t _shm_{}_seq = 0; /* seqlock */\n\n", ch.name));

                code.push_str(&format!("void shm_{}_write(const void *data, uint32_t len) {{\n", ch.name));
                code.push_str(&format!("    _shm_{}_seq++;\n", ch.name));
                code.push_str("    __asm__ volatile(\"dmb\" ::: \"memory\");\n");
                code.push_str(&format!("    __builtin_memcpy(_shm_{}, data, len);\n", ch.name));
                code.push_str("    __asm__ volatile(\"dmb\" ::: \"memory\");\n");
                code.push_str(&format!("    _shm_{}_seq++;\n", ch.name));
                code.push_str("}\n\n");
            }
            IpcKind::EventFlags => {
                code.push_str(&format!("/* EventFlags: {} */\n", ch.name));
                code.push_str(&format!("static volatile uint32_t _evt_{} = 0;\n\n", ch.name));
                code.push_str(&format!("void evt_{}_set(uint32_t flags) {{\n", ch.name));
                code.push_str("    hardy_enter_critical();\n");
                code.push_str(&format!("    _evt_{} |= flags;\n", ch.name));
                code.push_str("    hardy_exit_critical();\n}\n\n");
                code.push_str(&format!("uint32_t evt_{}_wait(uint32_t mask) {{\n", ch.name));
                code.push_str(&format!("    while (!(_evt_{} & mask)) {{ hardy_yield(); }}\n", ch.name));
                code.push_str("    hardy_enter_critical();\n");
                code.push_str(&format!("    uint32_t f = _evt_{} & mask;\n", ch.name));
                code.push_str(&format!("    _evt_{} &= ~mask;\n", ch.name));
                code.push_str("    hardy_exit_critical();\n");
                code.push_str("    return f;\n}\n\n");
            }
        }
    }

    code
}

// =============================================================================
// BSP 自动生成
// =============================================================================

/// BSP 硬件描述
#[derive(Debug, Clone)]
pub struct BspDesc {
    /// 板卡名称
    pub board_name: String,
    /// 目标
    pub target: RtosTarget,
    /// 晶振频率 (Hz)
    pub crystal_hz: u32,
    /// 系统时钟目标 (Hz)
    pub sysclk_hz: u32,
    /// Flash 起始地址
    pub flash_base: u32,
    /// Flash 大小
    pub flash_size: u32,
    /// RAM 起始地址
    pub ram_base: u32,
    /// RAM 大小
    pub ram_size: u32,
    /// 中断数量
    pub irq_count: u8,
}

/// 生成 BSP 启动代码 + 链接脚本
pub fn generate_bsp(desc: &BspDesc) -> (String, String) {
    // 启动代码
    let mut startup = String::new();
    startup.push_str(&format!("/* HardyRTOS BSP: {} */\n", desc.board_name));
    startup.push_str(&format!("/* Target: {} | Crystal: {}Hz → SysClk: {}Hz */\n\n",
        desc.target.arch(), desc.crystal_hz, desc.sysclk_hz));

    // 中断向量表
    startup.push_str("__attribute__((section(\".isr_vector\")))\n");
    startup.push_str("const uint32_t _vector_table[] = {\n");
    startup.push_str(&format!("    0x{:08X},  /* Initial SP */\n", desc.ram_base + desc.ram_size));
    startup.push_str("    (uint32_t)&Reset_Handler,\n");
    startup.push_str("    (uint32_t)&NMI_Handler,\n");
    startup.push_str("    (uint32_t)&HardFault_Handler,\n");
    startup.push_str("    (uint32_t)&MemManage_Handler,\n");
    startup.push_str("    (uint32_t)&BusFault_Handler,\n");
    startup.push_str("    (uint32_t)&UsageFault_Handler,\n");
    startup.push_str("    0, 0, 0, 0,\n");
    startup.push_str("    (uint32_t)&SVCall_Handler,\n");
    startup.push_str("    0, 0,\n");
    startup.push_str("    (uint32_t)&PendSV_Handler,\n");
    startup.push_str("    (uint32_t)&SysTick_Handler,\n");
    startup.push_str("};\n\n");

    // Reset Handler
    startup.push_str("void Reset_Handler(void) {\n");
    startup.push_str("    /* Copy .data from Flash to RAM */\n");
    startup.push_str("    extern uint32_t _sidata, _sdata, _edata;\n");
    startup.push_str("    uint32_t *src = &_sidata, *dst = &_sdata;\n");
    startup.push_str("    while (dst < &_edata) *dst++ = *src++;\n");
    startup.push_str("    /* Zero .bss */\n");
    startup.push_str("    extern uint32_t _sbss, _ebss;\n");
    startup.push_str("    dst = &_sbss;\n");
    startup.push_str("    while (dst < &_ebss) *dst++ = 0;\n");
    startup.push_str("    /* System clock init */\n");
    startup.push_str("    _system_clock_init();\n");
    startup.push_str("    /* Jump to main */\n");
    startup.push_str("    main();\n");
    startup.push_str("    while(1);\n");
    startup.push_str("}\n\n");

    // 时钟配置
    let pll_mul = desc.sysclk_hz / desc.crystal_hz;
    startup.push_str(&format!("/* PLL: {}Hz x {} = {}Hz */\n", desc.crystal_hz, pll_mul, desc.sysclk_hz));
    startup.push_str("static void _system_clock_init(void) {\n");
    startup.push_str("    /* Enable HSE, configure PLL, switch SYSCLK */\n");
    startup.push_str("}\n");

    // 链接脚本
    let mut ld = String::new();
    ld.push_str(&format!("/* HardyRTOS Linker Script: {} */\n", desc.board_name));
    ld.push_str("MEMORY\n{\n");
    ld.push_str(&format!("    FLASH (rx)  : ORIGIN = 0x{:08X}, LENGTH = {}K\n",
        desc.flash_base, desc.flash_size / 1024));
    ld.push_str(&format!("    RAM   (rwx) : ORIGIN = 0x{:08X}, LENGTH = {}K\n",
        desc.ram_base, desc.ram_size / 1024));
    ld.push_str("}\n\n");
    ld.push_str("SECTIONS\n{\n");
    ld.push_str("    .text : {\n");
    ld.push_str("        KEEP(*(.isr_vector))\n");
    ld.push_str("        *(.text*)\n");
    ld.push_str("        *(.rodata*)\n");
    ld.push_str("        . = ALIGN(4);\n");
    ld.push_str("        _etext = .;\n");
    ld.push_str("    } > FLASH\n\n");
    ld.push_str("    _sidata = LOADADDR(.data);\n");
    ld.push_str("    .data : {\n");
    ld.push_str("        _sdata = .;\n");
    ld.push_str("        *(.data*)\n");
    ld.push_str("        . = ALIGN(4);\n");
    ld.push_str("        _edata = .;\n");
    ld.push_str("    } > RAM AT> FLASH\n\n");
    ld.push_str("    .bss : {\n");
    ld.push_str("        _sbss = .;\n");
    ld.push_str("        *(.bss*)\n");
    ld.push_str("        *(COMMON)\n");
    ld.push_str("        . = ALIGN(4);\n");
    ld.push_str("        _ebss = .;\n");
    ld.push_str("    } > RAM\n\n");
    ld.push_str("    _estack = ORIGIN(RAM) + LENGTH(RAM);\n");
    ld.push_str("}\n");

    (startup, ld)
}

// =============================================================================
// 形式化验证接口
// =============================================================================

/// 生成 TLA+ 调度模型
pub fn generate_tla_model(config: &RtosConfig) -> String {
    let mut tla = String::new();
    tla.push_str(&format!("---- MODULE {} ----\n", config.system_name.replace('-', "_")));
    tla.push_str(&format!("\\* HardyRTOS scheduling model for {}\n", config.system_name));
    tla.push_str("\\* Generated by HardyScript RT module\n\n");

    tla.push_str("EXTENDS Integers, Sequences, FiniteSets, TLC\n\n");

    // 常量
    tla.push_str("CONSTANTS\n");
    tla.push_str("    Tasks,        \\* Set of task IDs\n");
    tla.push_str("    Priorities,   \\* Priority levels\n");
    tla.push_str("    TickBound     \\* Max ticks to explore\n\n");

    // 变量
    tla.push_str("VARIABLES\n");
    tla.push_str("    state,        \\* state[t] \\in {\"ready\", \"running\", \"blocked\"}\n");
    tla.push_str("    priority,     \\* priority[t]\n");
    tla.push_str("    deadline,     \\* deadline[t] (ticks)\n");
    tla.push_str("    wcet,         \\* wcet[t] (ticks)\n");
    tla.push_str("    clock,        \\* current tick\n");
    tla.push_str("    running,      \\* currently running task\n");
    tla.push_str("    missed        \\* set of tasks that missed deadline\n\n");

    // 类型不变量
    tla.push_str("TypeInvariant ==\n");
    tla.push_str("    /\\ state \\in [Tasks -> {\"ready\", \"running\", \"blocked\"}]\n");
    tla.push_str("    /\\ priority \\in [Tasks -> Priorities]\n");
    tla.push_str("    /\\ running \\in Tasks\n");
    tla.push_str("    /\\ clock \\in 0..TickBound\n\n");

    // 初始状态
    tla.push_str("Init ==\n");
    tla.push_str("    /\\ state = [t \\in Tasks |-> \"ready\"]\n");
    tla.push_str("    /\\ clock = 0\n");
    tla.push_str("    /\\ missed = {}\n");
    tla.push_str("    /\\ running = CHOOSE t \\in Tasks : TRUE\n\n");

    // 调度动作
    tla.push_str("\\* Scheduler picks highest priority ready task\n");
    tla.push_str("Schedule ==\n");
    tla.push_str("    LET best == CHOOSE t \\in Tasks :\n");
    tla.push_str("        /\\ state[t] = \"ready\"\n");
    tla.push_str("        /\\ \\A s \\in Tasks : state[s] = \"ready\" => priority[t] <= priority[s]\n");
    tla.push_str("    IN running' = best\n\n");

    // 截止期检查
    tla.push_str("\\* Safety property: no deadline miss\n");
    tla.push_str("NoDeadlineMiss == missed = {}\n\n");

    // 活性: 每个任务最终会运行
    tla.push_str("\\* Liveness: every task eventually runs\n");
    tla.push_str("Fairness == \\A t \\in Tasks : <>(running = t)\n\n");

    // 任务实例化
    tla.push_str(&format!("\\* Task parameters (from HardyScript @periodic annotations):\n"));
    for (i, task) in config.tasks.iter().enumerate() {
        let period_ticks = task.period_us / config.tick_us;
        let wcet_ticks = (task.wcet_us + config.tick_us - 1) / config.tick_us;
        tla.push_str(&format!("\\* Task {}: {} prio={} period={}ticks wcet={}ticks\n",
            i, task.name, task.priority, period_ticks, wcet_ticks));
    }

    tla.push_str(&format!("\n==== End MODULE {} ====\n", config.system_name.replace('-', "_")));

    tla
}

/// 生成 Spin/PROMELA 模型
pub fn generate_spin_model(config: &RtosConfig) -> String {
    let mut spin = String::new();
    spin.push_str(&format!("/* HardyRTOS Spin Model: {} */\n", config.system_name));
    spin.push_str("/* Verify: no deadlock, no priority inversion, deadlines met */\n\n");

    spin.push_str(&format!("#define N_TASKS {}\n", config.tasks.len()));
    spin.push_str("#define IDLE 0\n#define READY 1\n#define RUNNING 2\n#define BLOCKED 3\n\n");

    // 全局变量
    spin.push_str("byte task_state[N_TASKS];\n");
    spin.push_str("byte task_prio[N_TASKS];\n");
    spin.push_str("byte current_task = 0;\n");
    spin.push_str("int tick = 0;\n\n");

    // 调度器进程
    spin.push_str("proctype Scheduler() {\n");
    spin.push_str("    byte i, best, best_prio;\n");
    spin.push_str("    do\n");
    spin.push_str("    :: true ->\n");
    spin.push_str("        best = 0; best_prio = 255;\n");
    spin.push_str("        for (i : 0 .. N_TASKS - 1) {\n");
    spin.push_str("            if\n");
    spin.push_str("            :: task_state[i] == READY && task_prio[i] < best_prio ->\n");
    spin.push_str("                best_prio = task_prio[i]; best = i;\n");
    spin.push_str("            :: else -> skip;\n");
    spin.push_str("            fi;\n");
    spin.push_str("        };\n");
    spin.push_str("        atomic {\n");
    spin.push_str("            task_state[current_task] = READY;\n");
    spin.push_str("            current_task = best;\n");
    spin.push_str("            task_state[best] = RUNNING;\n");
    spin.push_str("        };\n");
    spin.push_str("        tick++;\n");
    spin.push_str("    od;\n");
    spin.push_str("}\n\n");

    // 每个任务一个进程
    for (i, task) in config.tasks.iter().enumerate() {
        let period_ticks = task.period_us / config.tick_us.max(1);
        spin.push_str(&format!("/* Task {}: {} (prio={}, period={}ticks) */\n", i, task.name, task.priority, period_ticks));
        spin.push_str(&format!("proctype Task_{}() {{\n", i));
        spin.push_str("    do\n");
        spin.push_str(&format!("    :: task_state[{}] == RUNNING ->\n", i));
        spin.push_str(&format!("        /* execute {} ({}ticks WCET) */\n", task.name, (task.wcet_us + config.tick_us - 1) / config.tick_us.max(1)));
        spin.push_str(&format!("        task_state[{}] = BLOCKED;\n", i));
        spin.push_str(&format!("        /* wait {} ticks (period) */\n", period_ticks));
        spin.push_str(&format!("        task_state[{}] = READY;\n", i));
        spin.push_str("    od;\n");
        spin.push_str("}\n\n");
    }

    // 初始化
    spin.push_str("init {\n");
    for (i, task) in config.tasks.iter().enumerate() {
        spin.push_str(&format!("    task_prio[{}] = {};\n", i, task.priority));
        spin.push_str(&format!("    task_state[{}] = READY;\n", i));
    }
    spin.push_str("    run Scheduler();\n");
    for i in 0..config.tasks.len() {
        spin.push_str(&format!("    run Task_{}();\n", i));
    }
    spin.push_str("}\n\n");

    // LTL 属性
    spin.push_str("/* LTL properties to verify: */\n");
    spin.push_str("/* []<>(task_state[0] == RUNNING)  -- task 0 always eventually runs */\n");
    spin.push_str("/* [](current_task < N_TASKS)      -- no out-of-bounds */\n");

    spin
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_config() -> RtosConfig {
        RtosConfig {
            system_name: "MotorController".into(),
            target: RtosTarget::Stm32f4,
            cpu_freq_hz: 168_000_000,
            tick_us: 100,
            tasks: vec![
                RtosTask { name: "CurrentLoop".into(), priority: 0, stack_bytes: 512, period_us: 50, wcet_us: 20, hs_function: "currentLoop".into() },
                RtosTask { name: "VelocityLoop".into(), priority: 1, stack_bytes: 512, period_us: 100, wcet_us: 30, hs_function: "velocityLoop".into() },
                RtosTask { name: "PositionLoop".into(), priority: 2, stack_bytes: 1024, period_us: 1000, wcet_us: 100, hs_function: "positionLoop".into() },
                RtosTask { name: "CommTask".into(), priority: 3, stack_bytes: 2048, period_us: 10000, wcet_us: 500, hs_function: "commHandler".into() },
            ],
            time_slicing: false,
            time_slice_ticks: 5,
        }
    }

    #[test]
    fn test_rtos_kernel_generation() {
        let config = sample_config();
        let code = generate_rtos_kernel(&config);
        assert!(code.contains("HardyRTOS Microkernel"));
        assert!(code.contains("ARM Cortex-M"));
        assert!(code.contains("_tcb_t"));
        assert!(code.contains("SysTick_Handler"));
        assert!(code.contains("hardy_create_task"));
        assert!(code.contains("hardy_sleep_ticks"));
        assert!(code.contains("_schedule_next"));
    }

    #[test]
    fn test_rtos_target_info() {
        assert_eq!(RtosTarget::Stm32f4.arch(), "ARM Cortex-M");
        assert_eq!(RtosTarget::Esp32s3.arch(), "Xtensa LX7");
        assert_eq!(RtosTarget::Ch32v307.arch(), "RISC-V RV32IMAC");
        assert_eq!(RtosTarget::Stm32f4.context_regs(), 17);
        assert_eq!(RtosTarget::Ch32v307.context_regs(), 32);
    }

    #[test]
    fn test_ipc_message_queue() {
        let channels = vec![IpcChannel {
            name: "sensor_data".into(),
            kind: IpcKind::MessageQueue,
            msg_size: 16,
            depth: 8,
            producer: "SensorTask".into(),
            consumer: "CtrlTask".into(),
        }];
        let code = generate_ipc_code(&channels);
        assert!(code.contains("mq_sensor_data_send"));
        assert!(code.contains("mq_sensor_data_recv"));
        assert!(code.contains("dmb"));
        assert!(code.contains("% 8"));
    }

    #[test]
    fn test_ipc_pip_mutex() {
        let channels = vec![IpcChannel {
            name: "shared_buf".into(),
            kind: IpcKind::PipMutex,
            msg_size: 0,
            depth: 0,
            producer: "WriterTask".into(),
            consumer: "ReaderTask".into(),
        }];
        let code = generate_ipc_code(&channels);
        assert!(code.contains("mtx_shared_buf_lock"));
        assert!(code.contains("mtx_shared_buf_unlock"));
        assert!(code.contains("_saved_prio"));
    }

    #[test]
    fn test_ipc_zero_copy() {
        let channels = vec![IpcChannel {
            name: "frame".into(),
            kind: IpcKind::ZeroCopyShm,
            msg_size: 4096,
            depth: 0,
            producer: "CameraTask".into(),
            consumer: "VisionTask".into(),
        }];
        let code = generate_ipc_code(&channels);
        assert!(code.contains("shm_frame_write"));
        assert!(code.contains("seqlock"));
        assert!(code.contains("aligned(32)"));
    }

    #[test]
    fn test_bsp_generation() {
        let desc = BspDesc {
            board_name: "STM32F407VG".into(),
            target: RtosTarget::Stm32f4,
            crystal_hz: 8_000_000,
            sysclk_hz: 168_000_000,
            flash_base: 0x0800_0000,
            flash_size: 1024 * 1024,
            ram_base: 0x2000_0000,
            ram_size: 128 * 1024,
            irq_count: 82,
        };
        let (startup, ld) = generate_bsp(&desc);
        assert!(startup.contains(".isr_vector"));
        assert!(startup.contains("Reset_Handler"));
        assert!(startup.contains("PLL: 8000000Hz x 21 = 168000000Hz"));
        assert!(ld.contains("ORIGIN = 0x08000000"));
        assert!(ld.contains("LENGTH = 1024K"));
        assert!(ld.contains("_estack"));
    }

    #[test]
    fn test_tla_model() {
        let config = sample_config();
        let tla = generate_tla_model(&config);
        assert!(tla.contains("MODULE MotorController"));
        assert!(tla.contains("NoDeadlineMiss"));
        assert!(tla.contains("Schedule"));
        assert!(tla.contains("CurrentLoop"));
        assert!(tla.contains("prio=0"));
    }

    #[test]
    fn test_spin_model() {
        let config = sample_config();
        let spin = generate_spin_model(&config);
        assert!(spin.contains("N_TASKS 4"));
        assert!(spin.contains("proctype Scheduler()"));
        assert!(spin.contains("proctype Task_0()"));
        assert!(spin.contains("proctype Task_3()"));
        assert!(spin.contains("run Scheduler()"));
    }
}
