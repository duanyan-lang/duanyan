/* DUANYAN AV: TMR Voting — hello */
/* Channels: 3, Diverse: true */

static float _ch_result[3];
static volatile uint8_t _ch_valid[3];

float _tmr_vote_ctrl(void) {
    /* Median-of-3 for floating point */
    float a = _ch_result[0], b = _ch_result[1], c = _ch_result[2];
    if (a > b) { float t = a; a = b; b = t; }
    if (b > c) { float t = b; b = c; c = t; }
    if (a > b) { float t = a; a = b; b = t; }
    /* b is median */
    float spread = c - a;
    if (spread > 0.001f) { _tmr_fault_handler(); } /* channels diverged */
    return b;
}

/* Diverse Compilation Strategy:
 * Channel 0: -O2 (standard optimization)
 * Channel 1: -O1 (less optimization, different code path)
 * Channel 2: -O0 + different register allocator
 * Rationale: reduces common-mode failure probability
 */

static void _tmr_fault_handler(void) {
    /* Log fault, increment counter */
    /* If persistent: transition to safe state */
}
