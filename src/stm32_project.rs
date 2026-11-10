//! STM32 项目生成器
//!
//! Phase E: STM32 后端
//! 生成标准 STM32 HAL 项目结构，供 arm-none-eabi-gcc 编译

use std::fs;
use std::path::{Path, PathBuf};

use crate::c_codegen::CGenerateResult;

/// STM32 项目路径
pub struct Stm32ProjectPaths {
    pub project_dir: PathBuf,
    pub main_c: PathBuf,
    pub makefile: PathBuf,
    pub linker_script: PathBuf,
    pub openocd_cfg: PathBuf,
}

/// 生成 STM32 HAL 项目
pub fn generate_stm32_project(
    c_result: &CGenerateResult,
    base_dir: &Path,
    stem: &str,
) -> Result<Stm32ProjectPaths, String> {
    let project_name = format!("{}_stm32", sanitize_name(stem));
    let project_dir = base_dir.join(&project_name);
    let src_dir = project_dir.join("src");
    fs::create_dir_all(&src_dir)
        .map_err(|e| format!("创建 STM32 项目目录失败: {}", e))?;

    // main.c
    let main_c = src_dir.join("main.c");
    fs::write(&main_c, &c_result.source)
        .map_err(|e| format!("写入 main.c 失败: {}", e))?;

    // Makefile
    let makefile = project_dir.join("Makefile");
    fs::write(&makefile, generate_makefile(stem))
        .map_err(|e| format!("写入 Makefile 失败: {}", e))?;

    // Linker script (STM32F103C8T6 default)
    let linker_script = project_dir.join("STM32F103.ld");
    fs::write(&linker_script, generate_linker_script())
        .map_err(|e| format!("写入 linker script 失败: {}", e))?;

    // OpenOCD config
    let openocd_cfg = project_dir.join("openocd.cfg");
    fs::write(&openocd_cfg, generate_openocd_cfg())
        .map_err(|e| format!("写入 openocd.cfg 失败: {}", e))?;

    Ok(Stm32ProjectPaths {
        project_dir,
        main_c,
        makefile,
        linker_script,
        openocd_cfg,
    })
}

fn generate_makefile(stem: &str) -> String {
    let name = sanitize_name(stem);
    format!(
r#"# DUANYAN STM32 Makefile (auto-generated)
# 需要设置环境变量: STM32_HAL_PATH 指向 STM32 HAL 库根目录

PREFIX  = arm-none-eabi-
CC      = $(PREFIX)gcc
OBJCOPY = $(PREFIX)objcopy
TARGET  = {name}

# STM32 HAL 路径 (用户需设置)
HAL_PATH ?= $(STM32_HAL_PATH)

CFLAGS  = -mcpu=cortex-m3 -mthumb -std=c99 -Os -Wall
CFLAGS += -DSTM32F103xB -DUSE_HAL_DRIVER
CFLAGS += -I$(HAL_PATH)/Inc -I$(HAL_PATH)/Inc/Legacy
CFLAGS += -I$(HAL_PATH)/Drivers/CMSIS/Device/ST/STM32F1xx/Include
CFLAGS += -I$(HAL_PATH)/Drivers/CMSIS/Include

LDFLAGS = -T STM32F103.ld -Wl,--gc-sections -nostartfiles

SRCS    = src/main.c
OBJS    = $(SRCS:.c=.o)

all: $(TARGET).bin

$(TARGET).elf: $(OBJS)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $^

$(TARGET).bin: $(TARGET).elf
	$(OBJCOPY) -O binary $< $@

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(OBJS) $(TARGET).elf $(TARGET).bin

flash: $(TARGET).bin
	st-flash write $< 0x08000000

debug: $(TARGET).elf
	openocd -f openocd.cfg -c "program $(TARGET).elf reset halt"

.PHONY: all clean flash debug
"#,
        name = name
    )
}

fn generate_linker_script() -> String {
    r#"/* STM32F103C8T6 Linker Script (DUANYAN auto-generated) */
MEMORY
{
    FLASH (rx)  : ORIGIN = 0x08000000, LENGTH = 64K
    RAM   (rwx) : ORIGIN = 0x20000000, LENGTH = 20K
}

ENTRY(Reset_Handler)

SECTIONS
{
    .text : {
        KEEP(*(.isr_vector))
        *(.text*)
        *(.rodata*)
        . = ALIGN(4);
        _etext = .;
    } > FLASH

    .data : AT(_etext) {
        _sdata = .;
        *(.data*)
        . = ALIGN(4);
        _edata = .;
    } > RAM

    .bss : {
        _sbss = .;
        *(.bss*)
        *(COMMON)
        . = ALIGN(4);
        _ebss = .;
    } > RAM

    _stack_top = ORIGIN(RAM) + LENGTH(RAM);
}
"#.to_string()
}

fn generate_openocd_cfg() -> String {
    r#"# DUANYAN OpenOCD config for STM32F103 (ST-Link)
source [find interface/stlink.cfg]
source [find target/stm32f1x.cfg]
adapter speed 4000
"#.to_string()
}

fn sanitize_name(name: &str) -> String {
    let s: String = name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    if s.is_empty() { "duanyan_app".to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("my-app"), "my_app");
        assert_eq!(sanitize_name("hello.c"), "hello_c");
        assert_eq!(sanitize_name(""), "duanyan_app");
        assert_eq!(sanitize_name("blink"), "blink");
    }

    #[test]
    fn test_makefile_generation() {
        let mk = generate_makefile("blink");
        assert!(mk.contains("arm-none-eabi-"));
        assert!(mk.contains("TARGET  = blink"));
        assert!(mk.contains("st-flash write"));
    }

    #[test]
    fn test_linker_script() {
        let ld = generate_linker_script();
        assert!(ld.contains("0x08000000"));
        assert!(ld.contains("64K"));
        assert!(ld.contains("20K"));
    }
}
