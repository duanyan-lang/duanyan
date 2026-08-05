/* esp32s3_bare.ld — ESP32-S3 裸机链接脚本
 * 代码放 IRAM (0x403C9700) - 仅用于执行
 * 数据放 DRAM (0x3FCE0000) - 用于读写
 * ROM bootloader 在 DIO 模式下可加载到两个地址
 */
MEMORY
{
    iram (rx)  : ORIGIN = 0x403C9700, LENGTH = 64K
    dram (rw)  : ORIGIN = 0x3FCE0000, LENGTH = 64K
}

ENTRY(_start)

SECTIONS
{
    .text :
    {
        *(.literal*)
        *(.text.startup)
        *(.text*)
    } > iram

    .rodata :
    {
        *(.rodata*)
    } > dram

    .data :
    {
        *(.data*)
        *(.sdata*)
    } > dram

    .bss :
    {
        *(.bss*)
        *(.sbss*)
        *(COMMON)
    } > dram

    /DISCARD/ :
    {
        *(.comment)
        *(.note*)
        *(.eh_frame*)
    }
}
