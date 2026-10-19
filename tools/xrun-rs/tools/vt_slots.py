import subprocess, sys
so = sys.argv[1]; vt_hex = sys.argv[2]; count = int(sys.argv[3]); label = sys.argv[4]
vt = int(vt_hex,16)
# addr -> name from dynsym+symtab
addr2name = {}
for extra in ([], ['-D']):
    out = subprocess.run(['nm','--defined-only']+extra+[so],capture_output=True,text=True).stdout
    for line in out.splitlines():
        f = line.split()
        if len(f)==3 and f[1] in 'TtWwDdBbRrVv':
            addr2name.setdefault(int(f[0],16), f[2])
rel = {}
for line in subprocess.run(['readelf','-rW',so],capture_output=True,text=True).stdout.splitlines():
    f = line.split()
    if len(f)>=4 and f[2]=='R_X86_64_RELATIVE':
        rel[int(f[0],16)] = ('rel', int(f[3],16), None)
    elif len(f)>=5 and f[2]=='R_X86_64_64':
        sym = f[4].split('@')[0] if len(f)>4 else ''
        rel[int(f[0],16)] = ('abs', int(f[4].split('+')[0],16) if False else 0, sym)
print(f'===== {label} vtable@{vt:#x} (vptr={vt+0x10:#x})')
for k in range(count):
    a = vt+0x10+8*k
    e = rel.get(a)
    if e is None:
        print(f'  slot {k}: <no reloc — raw file bytes>'); continue
    kind, add, sym = e
    if kind=='rel':
        n = addr2name.get(add, hex(add))
    else:
        n = sym or '?'
    d = subprocess.run(['c++filt',n],capture_output=True,text=True).stdout.strip()
    print(f'  slot {k}: {d}')
