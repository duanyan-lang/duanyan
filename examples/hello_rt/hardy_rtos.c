/* HardyRTOS Microkernel — hello */
/* Target: Stm32f4 (ARM Cortex-M) */
/* CPU: 168000000 Hz, Tick: 100μs */
/* Tasks: 0, Time-slice: false */

#define HARDY_MAX_TASKS 8
#define HARDY_TICK_US 100
#define HARDY_CPU_HZ 168000000
#define HARDY_CONTEXT_SIZE 17 /* registers */

typedef struct {
    uint32_t *sp;              /* saved stack pointer */
    uint8_t   priority;
    uint8_t   state;           /* 0=READY 1=RUNNING 2=BLOCKED 3=SLEEP */
    uint16_t  time_slice;
    uint32_t  period_ticks;
    uint32_t  next_wake;
    uint32_t  stack_base;
    uint32_t  stack_size;
    void    (*entry)(void);
} _tcb_t;

static _tcb_t _tasks[HARDY_MAX_TASKS];
static volatile uint8_t _task_count = 0;
static volatile uint8_t _current = 0;
static volatile uint32_t _tick_count = 0;

/* Preemptive priority scheduler */
static inline uint8_t _schedule_next(void) {
    uint8_t best = 0;
    uint8_t best_prio = 255;
    for (uint8_t i = 0; i < _task_count; i++) {
        if (_tasks[i].state == 0 && _tasks[i].priority < best_prio) {
            best_prio = _tasks[i].priority;
            best = i;
        }
    }
    return best;
}

void SysTick_Handler(void) {
    _tick_count++;
    /* Wake periodic tasks */
    for (uint8_t i = 0; i < _task_count; i++) {
        if (_tasks[i].state == 3 && _tick_count >= _tasks[i].next_wake) {
            _tasks[i].state = 0;
            _tasks[i].next_wake += _tasks[i].period_ticks;
        }
    }
    /* Context switch */
    uint8_t next = _schedule_next();
    if (next != _current) { _context_switch(&_current, &next); }
}

/* Syscall 1: Create task */
int hardy_create_task(void (*entry)(void), uint8_t prio, uint32_t stack_sz, uint32_t period_ticks) {
    if (_task_count >= HARDY_MAX_TASKS) return -1;
    _tcb_t *t = &_tasks[_task_count];
    t->entry = entry;
    t->priority = prio;
    t->stack_size = stack_sz;
    t->period_ticks = period_ticks;
    t->state = 0;
    return _task_count++;
}

/* Syscall 2: Sleep (periodic wait) */
void hardy_sleep_ticks(uint32_t ticks) {
    _tasks[_current].state = 3;
    _tasks[_current].next_wake = _tick_count + ticks;
    _yield();
}

/* Syscall 3: Yield */
void hardy_yield(void) { _yield(); }

/* Syscall 4: Get tick count */
uint32_t hardy_get_tick(void) { return _tick_count; }

/* Syscall 5: Critical section */
void hardy_enter_critical(void) { __disable_irq(); }
void hardy_exit_critical(void) { __enable_irq(); }
