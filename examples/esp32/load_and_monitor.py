"""load_and_monitor.py — 加载代码到 ESP32-S3 RAM 并监听输出（不触发复位）"""
import serial
import time
import struct
import sys

PORT = 'COM4'
BAUD = 115200
BIN_FILE = r'c:\DuanYanOS\DUANYAN\examples\esp32\test_uart.bin'

def sync_rom(ser):
    """发送 SLIP 同步包让 ROM 进入下载模式"""
    # 触发下载模式: DTR=low, RTS=high -> EN=low(reset), IO0=low(boot)
    ser.dtr = False
    ser.rts = True
    time.sleep(0.1)
    ser.dtr = True   # EN=high (release reset)
    time.sleep(0.1)
    ser.rts = False  # IO0=high (after boot strapping)
    time.sleep(0.1)

def read_output(ser, duration=8):
    """读取串口输出"""
    start = time.time()
    data = b''
    while time.time() - start < duration:
        chunk = ser.read(4096)
        if chunk:
            data += chunk
            # 实时打印
            sys.stdout.write(chunk.decode('utf-8', errors='replace'))
            sys.stdout.flush()
    return data

print(f"Opening {PORT} @ {BAUD}...")
ser = serial.Serial(PORT, BAUD, timeout=0.1, dsrdtr=False, rtscts=False)
time.sleep(0.5)

# 清空缓冲区
ser.reset_input_buffer()

print("Triggering download mode...")
sync_rom(ser)
time.sleep(1)

# 读取 ROM 启动信息
print("=== ROM output (after reset) ===")
rom_data = ser.read(4096)
print(rom_data.decode('utf-8', errors='replace'))

print("\n=== Now monitoring for app output (10s) ===")
print("(If code was loaded via esptool, output appears here)")
read_output(ser, 10)

ser.close()
print("\nDone.")
