#!/usr/bin/env python3
# pmf-metrics — 读取 AMD PMFW 指标表(amd_pmf 驱动的 smu_pmf_metrics,含每核/GFX/NPU 功率)
#
# 原理:复刻内核 drivers/platform/x86/amd/pmf/metrics.c 的 V2 mailbox 流程——
#   SET_DRAM_ADDR_HIGH(0x14)/LOW(0x15) 注册自己的物理页,
#   SET_TRANSFER_TABLE(0x16, arg=7) 让 PMFW 把指标表(表 7)DMA 进来。
# mailbox 页地址 = SMN 0x13B102E8/EC 交叉掩码合成 + 0x10000(见内核 core.c)。
#
# 用法:
#   pmf-metrics.py read           单次读取并按已知布局解码
#   pmf-metrics.py collect        三态采样(空闲/CPU压测/GPU压测)存 /tmp/pmf-states.pkl
#   pmf-metrics.py decode [file]  分析采样,输出字段候选
#
# 依赖:root;ryzen_smu 模块(用于 SMN 读,需含 0x5D000B 补丁,见 docs/notes/ryzen-smu-5d000b-pm-table.md);
#       glxgears(GPU 压测,collect 用)。
# 注意:运行期间会临时把 amd_pmf 的 metrics_table_loop_ms 调成 1000000 暂停其轮询,结束自动恢复。
#
# ⚠️ 2026-11-03 事故:本工具的 read/collect 直写 MP1 mailbox,当日运行约 9 分钟后
#    SMU 失联 → amdgpu sdma0 超时 → GPU 永久僵死,只能重启。
#    详见 docs/notes/smu-mailbox-brick-amdgpu.md。read/collect 现已默认禁用。
import os, sys
if os.geteuid() != 0:
    os.execvp('sudo', ['sudo', '-E', 'python3', os.path.abspath(__file__)] + sys.argv[1:])

# 保险丝:对 SMU 只读。read/collect 需要显式 PMF_MBOX_WRITE_OK=1 且自担风险才会执行。
if os.environ.get('PMF_MBOX_WRITE_OK') != '1' and (len(sys.argv) < 2 or sys.argv[1] in ('read', 'collect')):
    sys.exit('已禁用(对 SMU 只读):read/collect 会经 /dev/mem 直写 MP1 mailbox\n'
             '(SET_DRAM_ADDR/TRANSFER_TABLE/清 resp)。2026-11-03 该做法致 SMU 失联、\n'
             'amdgpu 全死、只能重启 —— 见 docs/notes/smu-mailbox-brick-amdgpu.md。\n'
             "只读替代:ryzen_smu 模块 pm_table / amdgpu hwmon;decode 子命令不受限。\n"
             '自担风险强制运行:PMF_MBOX_WRITE_OK=1 ' + os.path.abspath(__file__))

import struct, mmap, os, sys, time, ctypes, subprocess, resource, pickle, glob

PMF_MAILBOX = 0xfd110000
SMN_BASE_LO, SMN_BASE_HI = 0x13B102E8, 0x13B102EC
PARAM = '/sys/module/amd_pmf/parameters/metrics_table_loop_ms'
SIZE = 4096

def smn_read(addr):
    if not os.path.exists('/sys/kernel/ryzen_smu_drv/smn'):
        subprocess.run(['modprobe', 'ryzen_smu'], check=True)
        time.sleep(0.3)
    with open('/sys/kernel/ryzen_smu_drv/smn', 'wb', buffering=0) as f:
        f.write(struct.pack('<I', addr))
    with open('/sys/kernel/ryzen_smu_drv/smn', 'rb') as f:
        return struct.unpack('<I', f.read(4))[0]

def amdgpu_hwmon_watts():
    for h in sorted(glob.glob('/sys/class/hwmon/hwmon*')):
        try:
            if open(h + '/name').read().strip() == 'amdgpu':
                return int(open(h + '/power1_average').read()) / 1e6
        except OSError:
            continue
    return -1.0

class PMF:
    def __init__(self):
        # 本机 ctypes 的 mmap 符号异常(恒 NULL),用 pymmap 文件映射绕开;
        # ctypes 只负责 VA 换算和 mlock
        libc = ctypes.CDLL('libc.so.6', use_errno=True)
        libc.mlock.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
        resource.setrlimit(resource.RLIMIT_MEMLOCK,
                           (resource.RLIM_INFINITY, resource.RLIM_INFINITY))
        fd = os.open('/tmp/pmf-dma-buffer', os.O_RDWR | os.O_CREAT | os.O_TRUNC, 0o600)
        os.ftruncate(fd, SIZE)
        self.buf = mmap.mmap(fd, SIZE)          # 共享文件映射,FW DMA 可见
        os.close(fd)
        self.addr = ctypes.addressof(ctypes.c_char.from_buffer(self.buf))
        for i in range(SIZE):
            self.buf[i] = 0
        assert libc.mlock(ctypes.c_void_p(self.addr), ctypes.c_size_t(SIZE)) == 0, 'mlock 失败'
        assert bytes(self.buf[:64]) == b'\0' * 64, '清零自检失败'
        pm = open('/proc/self/pagemap', 'rb')
        pm.seek((self.addr // 4096) * 8)
        entry, = struct.unpack('<Q', pm.read(8))
        assert entry & (1 << 63), '页不在内存'
        self.phys = (entry & ((1 << 55) - 1)) << 12

        lo = smn_read(SMN_BASE_LO); hi = smn_read(SMN_BASE_HI)
        # 掩码故意交叉:LO 取高 12 位,HI 取低 16 位(内核 core.c 同款)
        base = (hi & 0x0000FFFF) << 32 | (lo & 0xFFF00000)
        self.memfd = os.open('/dev/mem', os.O_RDWR | os.O_SYNC)
        self.mm = mmap.mmap(self.memfd, 0x1000, offset=base + 0x10000)
        self.R = {'msg': 0xA04, 'resp': 0xA08, 'arg0': 0xA0C}
        self._r32 = lambda k: struct.unpack_from('<I', self.mm, self.R[k])[0]
        self._w32 = lambda k, v: struct.pack_into('<I', self.mm, self.R[k], v)

    def cmd(self, msg, arg=None, wait=2.0):
        self._w32('resp', 0)
        if arg is not None:
            self._w32('arg0', arg)
        self._w32('msg', msg)
        t0 = time.time()
        while self._r32('resp') == 0 and time.time() - t0 < wait:
            time.sleep(0.0002)
        return self._r32('resp')

    def register(self):
        r1 = self.cmd(0x14, self.phys >> 32)
        r2 = self.cmd(0x15, self.phys & 0xFFFFFFFF)
        return r1, r2

    def refresh(self, prev=None, tries=8, gap=1.1):
        """触发一次传输并返回校验过的表内容(排除全零/堆指针脏页/重复)"""
        last = b''
        for _ in range(tries):
            for i in range(SIZE):
                self.buf[i] = 0
            self.cmd(0x16, 7)
            time.sleep(gap)
            t = bytes(self.buf[:512])
            nz = sum(1 for b in t if b)
            ptr_like = nz > 0 and struct.unpack_from('<Q', t, 24)[0] >> 40 == 0x7f
            if nz > 100 and not ptr_like and t != (prev or b''):
                return t, nz
            last = t
        return last, sum(1 for b in last if b)

    def pause_driver(self):
        with open(PARAM) as f:
            self._old_param = f.read().strip()
        open(PARAM, 'w').write('1000000')
        time.sleep(2.5)

    def resume_driver(self):
        open(PARAM, 'w').write(self._old_param)
        print('驱动轮询已恢复', file=sys.stderr)

def parse_args_table(t):
    """按实测布局解析(偏移基于 2026-11-03 实证,见 docs/notes/)"""
    g = lambda fmt, o: struct.unpack_from(fmt, t, o)
    return {
        'core_freq_MHz': g('<16H', 0),
        'core_power_mW': g('<16H', 32),
        'core_temp_cC': g('<16H', 64),
    }

def do_read():
    p = PMF()
    p.pause_driver()
    try:
        r1, r2 = p.register()
        t, nz = p.refresh()
        print(f"# 注册 resp {r1:#x}/{r2:#x}, 非零 {nz}")
        d = parse_args_table(t)
        print(f"core_freq MHz: {d['core_freq_MHz']}")
        print(f"core_power mW: {d['core_power_mW']}")
        print(f"core_temp °C:  {[v/100 for v in d['core_temp_cC']]}")
        print(f"对照 amdgpu hwmon: {amdgpu_hwmon_watts():.1f}W")
    finally:
        p.resume_driver()

def do_collect():
    p = PMF()
    p.pause_driver()
    try:
        p.register()
        def phase(tag, n, load=None):
            procs = []
            if load == 'cpu':
                procs = [subprocess.Popen(['yes'], stdout=subprocess.DEVNULL,
                                          stderr=subprocess.DEVNULL) for _ in range(12)]
                time.sleep(1.5)
            if load == 'gpu':
                procs = [subprocess.Popen(['glxgears'],
                         env={**os.environ, 'vblank_mode': '0'},
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)]
                time.sleep(2)
            snaps, hws, prev = [], [], None
            for i in range(n):
                t, nz = p.refresh(prev); prev = t
                snaps.append(t); hws.append(amdgpu_hwmon_watts())
                print(f"  {tag}[{i}]: 非零{nz} hwmon={hws[-1]:.1f}W", flush=True)
            for pr in procs:
                pr.kill()
            return snaps, hws
        idle, hi = phase('idle', 4)
        cpu, hc = phase('cpu', 4, 'cpu')
        gpu, hg = phase('gpu', 4, 'gpu')
        pickle.dump({'idle': idle, 'cpu': cpu, 'gpu': gpu,
                     'hw': {'i': hi, 'c': hc, 'g': hg}},
                    open('/tmp/pmf-states.pkl', 'wb'))
        print('已保存 /tmp/pmf-states.pkl')
    finally:
        p.resume_driver()

def do_decode(path='/tmp/pmf-states.pkl'):
    import statistics
    d = pickle.load(open(path, 'rb'))
    def med(ss, o, fmt='<H'):
        return statistics.median(struct.unpack_from(fmt, s, o)[0] for s in ss)
    print(f"{'偏移':>6} {'idle':>9} {'cpu':>9} {'gpu':>9}  量纲猜测")
    for o in range(0, 500, 2):
        mi, mc, mg = med(d['idle'], o), med(d['cpu'], o), med(d['gpu'], o)
        mx, mn = max(mi, mc, mg), min(mi, mc, mg)
        if mx == 0 or mx > 60000:
            continue
        spread = mx - mn
        kind = ''
        if 2500 <= mn and mx <= 6500:
            kind = '温度?'
        elif 300 <= mn and mx <= 6000:
            kind = '频率?'
        elif mx <= 10200:
            kind = '百分比?'
        elif spread > 1000:
            kind = '功率?'
        if kind and spread > 200:
            print(f"{o:#06x} {mi:9.0f} {mc:9.0f} {mg:9.0f}  {kind}")

if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else 'read'
    {'read': do_read, 'collect': do_collect, 'decode': do_decode}[cmd](*sys.argv[2:])
