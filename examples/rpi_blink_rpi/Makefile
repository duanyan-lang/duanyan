# DUANYAN Raspberry Pi Makefile (auto-generated)
CC      = gcc
TARGET  = rpi_blink

CFLAGS  = -std=c99 -Wall -O2
LDFLAGS =

# 交叉编译 (可选)
# CC    = arm-linux-gnueabihf-gcc

SRCS    = main.c
OBJS    = $(SRCS:.c=.o)

all: $(TARGET)

$(TARGET): $(OBJS)
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $^

%.o: %.c
	$(CC) $(CFLAGS) -c -o $@ $<

clean:
	rm -f $(OBJS) $(TARGET)

run: $(TARGET)
	sudo ./$(TARGET)

.PHONY: all clean run
