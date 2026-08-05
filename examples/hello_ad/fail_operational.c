/* HardyScript AD: Fail-Operational — hello */
/* Primary: ASIL-D, Backup: ASIL-B */

typedef enum {
    SYS_STATE_NORMAL = 0,
    SYS_STATE_DEGRADED,
    SYS_STATE_MRC,        /* Minimal Risk Condition */
    SYS_STATE_FAULT
} _sys_state_t;

static volatile _sys_state_t _sys_state = SYS_STATE_NORMAL;
static volatile uint32_t _heartbeat_counter = 0;
#define HEARTBEAT_TIMEOUT_MS 50
#define DEGRADED_SPEED_LIMIT 30

/* Health Monitor: heartbeat check */
void _health_monitor_tick(void) {
    _heartbeat_counter++;
    if (_heartbeat_counter > HEARTBEAT_TIMEOUT_MS) {
        _trigger_degradation();
    }
}

static void _trigger_degradation(void) {
    switch (_sys_state) {
    case SYS_STATE_NORMAL:
        _sys_state = SYS_STATE_DEGRADED;
        _activate_backup_channel();
        break;
    case SYS_STATE_DEGRADED:
        _sys_state = SYS_STATE_MRC;
        _execute_minimal_risk();
        break;
    default:
        _sys_state = SYS_STATE_FAULT;
        break;
    }
}

/* MRC: safe stop within 5.0s */
static void _execute_minimal_risk(void) {
    /* 1. 减速到 0 */
    /* 2. 开启双闪 */
    /* 3. 锁定转向 */
    /* 4. 通知远程监控 */
}

/* Program Flow Monitoring */
static volatile uint32_t _pfm_signature = 0;
static inline void _pfm_checkpoint(uint32_t expected) {
    _pfm_signature ^= expected;
}
static inline int _pfm_verify(uint32_t final_sig) {
    return _pfm_signature == final_sig;
}
