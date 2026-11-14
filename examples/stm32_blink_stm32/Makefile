# DUANYAN STM32 Makefile (auto-generated)
# 需要设置环境变量: STM32_HAL_PATH 指向 STM32 HAL 库根目录

PREFIX  = arm-none-eabi-
CC      = $(PREFIX)gcc
OBJCOPY = $(PREFIX)objcopy
TARGET  = stm32_blink

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
