/* HardyScript SE: Nano-Positioning Servo */
/* Axes: 2, Servo Period: 50μs */

/* Fixed-point: 1 LSB = 0.1nm (Q22.10 format) */
typedef int64_t nano_pos_t;
#define NM_TO_RAW(nm) ((nano_pos_t)((nm) * 10))
#define RAW_TO_NM(raw) ((double)(raw) / 10.0)

typedef struct {
    nano_pos_t target;
    nano_pos_t actual;
    nano_pos_t error;
    nano_pos_t integral;
    nano_pos_t prev_error;
    int32_t    output;      /* DAC counts */
    uint8_t    settled;
} _nano_axis_t;

static _nano_axis_t _axes[2];

/* PID: Kp=50.0 Ki=0.010 Kd=5.0 */
static inline void _servo_tick(uint8_t axis_id) {
    _nano_axis_t *a = &_axes[axis_id];
    a->error = a->target - a->actual;
    a->integral += a->error;
    /* Anti-windup */
    if (a->integral > NM_TO_RAW(1000)) a->integral = NM_TO_RAW(1000);
    if (a->integral < -NM_TO_RAW(1000)) a->integral = -NM_TO_RAW(1000);
    int64_t p = (int64_t)(a->error * 50.0);
    int64_t i = (int64_t)(a->integral * 0.0100);
    int64_t d = (int64_t)((a->error - a->prev_error) * 5.0);
    a->output = (int32_t)((p + i + d) >> 10);
    a->prev_error = a->error;
    /* Settling detection: |error| < 1nm for 100 samples */
    a->settled = (a->error < NM_TO_RAW(1) && a->error > -NM_TO_RAW(1));
}

/* Axis WaferStage_X: WaferStage_X (stroke=300000000nm, res=0.10nm, motor=PlanarMotor) */
/* Axis WaferStage_Y: WaferStage_Y (stroke=300000000nm, res=0.10nm, motor=PlanarMotor) */
