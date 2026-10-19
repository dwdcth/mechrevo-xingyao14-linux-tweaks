import subprocess, sys
so = sys.argv[1]; vt_mangled = sys.argv[2]
n = int(sys.argv[3]) if len(sys.argv)>3 else 16
# dynsym + symtab addr->name
addr2name={}
for use_d in (False, True):
    cmd = ['nm','-D'] if use_d else ['nm']
    out = subprocess.run(cmd+['--defined-only',so],capture_output=True,text=True).stdout
    for line in out.splitlines():
        f=line.split()
        if len(f)==3 and f[1] in 'TtWwDdBbRrVv':
            a=int(f[0],16)
            addr2name.setdefault(a, f[2])
# find vtable address
vt=None
out = subprocess.run(['nm','-D','--defined-only',so],capture_output=True,text=True).stdout
for line in out.splitlines():
    f=line.split()
    if len(f)==3 and f[2]==vt_mangled: vt=int(f[0],16)
if vt is None:
    out = subprocess.run(['nm','--defined-only',so],capture_output=True,text=True).stdout
    for line in out.splitlines():
        f=line.split()
        if len(f)==3 and f[2]==vt_mangled: vt=int(f[0],16)
assert vt, 'vtable symbol not found'
rel = subprocess.run(['readelf','-rW',so],capture_output=True,text=True).stdout
relocs={}
for line in rel.splitlines():
    f=line.split()
    if len(f)>=4 and f[2]=='R_X86_64_RELATIVE':
        relocs[int(f[0],16)] = int(f[3],16)
print(f'===== {vt_mangled} @0x{vt:x}')
import subprocess as sp
for i in range(2, n+2):
    a = relocs.get(vt+8*i)
    if a is None: break
    nm_ = addr2name.get(a, hex(a))
    d = sp.run(['c++filt',nm_],capture_output=True,text=True).stdout.strip()
    print(f'  v{i-2}: [{a:#x}] {d}')
