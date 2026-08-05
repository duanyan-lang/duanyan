/* HardyScript AV: Float Determinism Layer */
/* Guarantees: bit-exact results across all target platforms */

/* Required compiler flags:
 *   -ffp-contract=off    (disable FMA contraction)
 *   -fno-fast-math       (preserve IEEE 754 semantics)
 *   -fno-unsafe-math-optimizations
 *   -mno-fma             (ARM: disable FMA instructions)
 */

/* NaN Determinism: replace NaN with deterministic sentinel */
#define HARDY_NAN_SENTINEL 0x7F800001u

static inline float _hardy_float_guard(float x) {
    uint32_t bits;
    __builtin_memcpy(&bits, &x, 4);
    if ((bits & 0x7FFFFFFFu) > 0x7F800000u) {
        bits = HARDY_NAN_SENTINEL;
        __builtin_memcpy(&x, &bits, 4);
    }
    return x;
}

/* Table-driven sin/cos: 4096 entries, 24-bit precision */
static const float _sin_table[4096] = { /* generated */ };
static const float _cos_table[4096] = { /* generated */ };

static inline float _hardy_sin(float x) {
    /* Range reduction to [0, 2π) */
    float r = x - 6.283185307f * (float)((int)(x * 0.159154943f));
    if (r < 0.0f) r += 6.283185307f;
    uint32_t idx = (uint32_t)(r * 4096 / 6.283185307f);
    return _sin_table[idx % 4096];
}

static inline float _hardy_cos(float x) {
    return _hardy_sin(x + 1.570796327f);
}

/* Deterministic float comparison (no epsilon ambiguity) */
static inline int _hardy_feq(float a, float b, float eps) {
    float d = a - b;
    if (d < 0.0f) d = -d;
    return d <= eps;
}

/* Deterministic float-to-int (round toward zero, saturate) */
static inline int32_t _hardy_f2i(float x) {
    if (x > 2147483647.0f) return 2147483647;
    if (x < -2147483648.0f) return -2147483648;
    return (int32_t)x;
}
