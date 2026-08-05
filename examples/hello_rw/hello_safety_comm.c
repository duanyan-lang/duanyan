/* HardyScript RW: EN 50159 Safety Communication — hello */
/* SIL: SIL 4 | Safety Code: 32 bits */

typedef struct __attribute__((packed)) {
    uint32_t sequence;      /* sequence number */
    uint32_t timestamp;     /* send time (ms) */
    uint16_t msg_type;      /* message type */
    uint16_t payload_len;   /* data length */
    uint8_t  payload[256];  /* application data */
    uint32_t safety_code;   /* CRC/MAC for integrity */
} _safe_msg_t;

static uint32_t _tx_sequence = 0;

int _safe_send(uint16_t type, const uint8_t *data, uint16_t len) {
    _safe_msg_t msg;
    msg.sequence = _tx_sequence++;
    msg.timestamp = _get_tick_ms();
    msg.msg_type = type;
    msg.payload_len = len;
    __builtin_memcpy(msg.payload, data, len);
    msg.safety_code = _crc32_compute(&msg, sizeof(msg) - 4);
    return _transport_send(&msg, sizeof(msg));
}

int _safe_recv(_safe_msg_t *out) {
    _safe_msg_t msg;
    if (_transport_recv(&msg, sizeof(msg)) < 0) return -1;
    /* Check timeout */
    if (_get_tick_ms() - msg.timestamp > 500) return -2; /* TIMEOUT */
    /* Verify safety code */
    uint32_t expected = _crc32_compute(&msg, sizeof(msg) - 4);
    if (msg.safety_code != expected) return -3; /* INTEGRITY FAIL */
    /* Check sequence */
    /* ... sequence validation ... */
    *out = msg;
    return 0;
}
