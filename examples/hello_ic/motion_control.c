/* HardyScript IC: Multi-Axis Motion Control */
/* Axes: 2 */

typedef struct {
    volatile int32_t  position;     /* encoder counts */
    volatile int32_t  velocity;     /* counts/s */
    volatile int32_t  torque;       /* PWM duty */
    int32_t  target_pos;
    int32_t  target_vel;
    uint8_t  enabled;
    uint8_t  fault;
} _axis_state_t;

static _axis_state_t _axes[2];

/* Current loop (innermost, fastest) */
static inline int32_t _current_loop(int32_t ref, int32_t actual, int32_t integral) {
    int32_t err = ref - actual;
    integral += err;
    return err * 100 + integral * 10; /* P=100, I=10 */
}

/* Velocity loop */
static inline int32_t _velocity_loop(int32_t ref, int32_t actual) {
    int32_t err = ref - actual;
    return err * 50; /* P=50 */
}

/* Position loop (outermost, slowest) */
static inline int32_t _position_loop(int32_t target, int32_t actual) {
    int32_t err = target - actual;
    return err * 20; /* P=20, output = velocity ref */
}

/* Axis 0: X (pos:1000μs vel:100μs cur:50μs) */
/* Axis 1: Y (pos:1000μs vel:100μs cur:50μs) */
