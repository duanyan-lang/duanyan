import sys
data = open(r'c:\DuanYanOS\DUANYAN\examples\esp32\chip_info_v1.bin','rb').read()
print(f'Total size: {len(data)} bytes')
print(f'Header: {data[0:8].hex()}')
print(f'  magic=0x{data[0]:02x} segments={data[1]} spi=0x{data[2]:02x} size_freq=0x{data[3]:02x}')
ep = int.from_bytes(data[4:8],'little')
print(f'  entry_point=0x{ep:08x}')
print(f'Extended hdr (8-23): {data[8:24].hex()}')
print(f'Seg hdr (24-31): {data[24:32].hex()}')
la = int.from_bytes(data[24:28],'little')
sl = int.from_bytes(data[28:32],'little')
print(f'  load_addr=0x{la:08x} len=0x{sl:x} ({sl})')
seg_start = 32
seg_end = seg_start + sl
print(f'Segment data: offset {seg_start} to {seg_end}')
print(f'Byte at seg_end (checksum pos): 0x{data[seg_end]:02x}')
# The ROM calculates XOR of segment data starting with 0xEF
xor = 0xEF
for b in data[seg_start:seg_end]:
    xor ^= b
print(f'ROM calculated checksum: 0x{xor:02x}')
print(f'Stored at seg_end: 0x{data[seg_end]:02x}')
# Check if padding is needed (checksum after 16-byte aligned padding)
aligned_end = (seg_end + 15) & ~15
print(f'Aligned end: {aligned_end}, byte there: 0x{data[aligned_end]:02x}' if aligned_end < len(data) else 'aligned end beyond file')
print(f'Last 48 bytes hex: {data[-48:].hex()}')
print(f'File len={len(data)}, last byte=0x{data[-1]:02x}')
