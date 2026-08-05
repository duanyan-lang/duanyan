/* HardyScript IC: EtherCAT Cyclic Executive */
/* Cycle: 1000μs, Slaves: 3 */

static uint8_t _ecat_output[72];
static uint8_t _ecat_input[72];

void _ecat_cyclic_task(void) {
    /* 1. 输出过程数据 → 从站 */
    _ecat_send_outputs();
    /* 2. 接收输入过程数据 */
    _ecat_recv_inputs();
    /* 3. 执行用户逻辑 */
    _hardy_plc_cycle();
    /* 4. 分布式时钟同步 */
    _ecat_dc_sync();
}

/* Timer: 1000μs cycle → 1000Hz */
