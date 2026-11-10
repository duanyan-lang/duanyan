/* DUANYAN RTOS BSP: STM32F407VG */
/* Target: ARM Cortex-M | Crystal: 8000000Hz → SysClk: 168000000Hz */

__attribute__((section(".isr_vector")))
const uint32_t _vector_table[] = {
    0x20020000,  /* Initial SP */
    (uint32_t)&Reset_Handler,
    (uint32_t)&NMI_Handler,
    (uint32_t)&HardFault_Handler,
    (uint32_t)&MemManage_Handler,
    (uint32_t)&BusFault_Handler,
    (uint32_t)&UsageFault_Handler,
    0, 0, 0, 0,
    (uint32_t)&SVCall_Handler,
    0, 0,
    (uint32_t)&PendSV_Handler,
    (uint32_t)&SysTick_Handler,
};

void Reset_Handler(void) {
    /* Copy .data from Flash to RAM */
    extern uint32_t _sidata, _sdata, _edata;
    uint32_t *src = &_sidata, *dst = &_sdata;
    while (dst < &_edata) *dst++ = *src++;
    /* Zero .bss */
    extern uint32_t _sbss, _ebss;
    dst = &_sbss;
    while (dst < &_ebss) *dst++ = 0;
    /* System clock init */
    _system_clock_init();
    /* Jump to main */
    main();
    while(1);
}

/* PLL: 8000000Hz x 21 = 168000000Hz */
static void _system_clock_init(void) {
    /* Enable HSE, configure PLL, switch SYSCLK */
}
