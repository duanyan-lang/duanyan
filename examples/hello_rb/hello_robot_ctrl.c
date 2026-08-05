/* HardyScript RB: Embodied Robot Control — 6 DOF */

#define ROBOT_DOF 6

typedef struct {
    double position[ROBOT_DOF];  /* rad */
    double velocity[ROBOT_DOF];  /* rad/s */
    double torque[ROBOT_DOF];    /* Nm */
} _joint_state_t;

/* Impedance: M=2.0kg D=20.0Ns/m K=100.0N/m */
typedef struct {
    double force_cmd[3];
    double pos_correction[3];
} _impedance_out_t;

static inline _impedance_out_t _impedance_ctrl(
    const double pos_err[3], const double vel_err[3], const double accel[3]) {
    _impedance_out_t out;
    for (int i = 0; i < 3; i++) {
        out.force_cmd[i] = 2.0 * accel[i] + 20.0 * vel_err[i] + 100.0 * pos_err[i];
        out.pos_correction[i] = out.force_cmd[i] / 100.0;
    }
    return out;
}

/* Safety: F_max=150N, v_max=250mm/s (ISO/TS 15066) */
typedef enum { SAFE_CONTINUE, SAFE_REDUCE, SAFE_STOP, SAFE_ESTOP } _safety_action_t;

static _safety_action_t _safety_monitor(double force_n, double speed_mm_s) {
    if (force_n > 150.0 || speed_mm_s > 250.0) return SAFE_ESTOP;
    if (force_n > 120.0 || speed_mm_s > 200.0) return SAFE_REDUCE;
    return SAFE_CONTINUE;
}

/* Main servo loop (1kHz) */
void _robot_servo_tick(_joint_state_t *state) {
    /* 1. Read encoders + force sensors */
    /* 2. Compute impedance control law */
    /* 3. Safety check */
    /* 4. Output torque commands */
}
