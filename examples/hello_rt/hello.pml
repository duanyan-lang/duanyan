/* HardyRTOS Spin Model: hello */
/* Verify: no deadlock, no priority inversion, deadlines met */

#define N_TASKS 0
#define IDLE 0
#define READY 1
#define RUNNING 2
#define BLOCKED 3

byte task_state[N_TASKS];
byte task_prio[N_TASKS];
byte current_task = 0;
int tick = 0;

proctype Scheduler() {
    byte i, best, best_prio;
    do
    :: true ->
        best = 0; best_prio = 255;
        for (i : 0 .. N_TASKS - 1) {
            if
            :: task_state[i] == READY && task_prio[i] < best_prio ->
                best_prio = task_prio[i]; best = i;
            :: else -> skip;
            fi;
        };
        atomic {
            task_state[current_task] = READY;
            current_task = best;
            task_state[best] = RUNNING;
        };
        tick++;
    od;
}

init {
    run Scheduler();
}

/* LTL properties to verify: */
/* []<>(task_state[0] == RUNNING)  -- task 0 always eventually runs */
/* [](current_task < N_TASKS)      -- no out-of-bounds */
