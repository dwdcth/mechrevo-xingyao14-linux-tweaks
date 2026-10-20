#!/usr/bin/python3
# 清除 ELF PT_GNU_STACK 的 PF_X 位 (绕过 Arch 内核 execstack 限制)
# 用法: clear_execstack.py <file.so> [...]
import struct, sys

def clear(path):
    d = bytearray(open(path, 'rb').read())
    if d[:4] != b'\x7fELF':
        return False
    is64 = d[4] == 2
    e_phoff = struct.unpack_from('<Q' if is64 else '<I', d, 0x20 if is64 else 0x1C)[0]
    e_phentsize = struct.unpack_from('<H', d, 0x36 if is64 else 0x2A)[0]
    e_phnum = struct.unpack_from('<H', d, 0x38 if is64 else 0x2C)[0]
    hit = False
    for i in range(e_phnum):
        off = e_phoff + i * e_phentsize
        p_type = struct.unpack_from('<I', d, off)[0]
        if p_type == 0x6474E551:  # PT_GNU_STACK
            p_flags = struct.unpack_from('<I', d, off + 4)[0]
            if p_flags & 1:
                struct.pack_into('<I', d, off + 4, p_flags & ~1)
                hit = True
    if hit:
        open(path, 'wb').write(bytes(d))
    return hit

for p in sys.argv[1:]:
    try:
        print(('清除 ' if clear(p) else '无需 ') + p)
    except Exception as e:
        print(f'失败 {p}: {e}')
