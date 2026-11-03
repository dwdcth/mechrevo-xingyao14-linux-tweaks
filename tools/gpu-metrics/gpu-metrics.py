#!/usr/bin/env python3
# gpu-metrics — 只读解码 amdgpu gpu_metrics_v3.0(Strix Point 分域功耗/温度/频率)
#
# 数据源:/sys/class/drm/card*/device/gpu_metrics(内核 amdgpu 驱动经 SMU mailbox
# 刷新,驱动内部有锁——用户态纯读,符合"对 SMU 只读"铁律,见 docs/notes/smu-mailbox-brick-amdgpu.md)。
# 布局:pahole -C gpu_metrics_v3_0 /sys/kernel/btf/amdgpu(内核 7.3.0-2-cachyos 实测)。
#
# 用法:
#   gpu-metrics.py             打印一次
#   gpu-metrics.py -w 5        每 2s 刷一次,共 5 轮
#   gpu-metrics.py --json      机器可读输出
#   gpu-metrics.py --cores     额外打印每核明细
import glob, sys, time, struct, argparse, json

FIELDS = [  # (name, offset, total_size, count) — pahole 第三列是成员总大小
    ('temperature_gfx', 4, 2, 1), ('temperature_soc', 6, 2, 1),
    ('temperature_core', 8, 32, 16), ('temperature_skin', 40, 2, 1),
    ('average_gfx_activity', 42, 2, 1), ('average_vcn_activity', 44, 2, 1),
    ('average_ipu_activity', 46, 16, 8),
    ('average_core_c0_activity', 62, 32, 16),
    ('average_dram_reads', 94, 2, 1), ('average_dram_writes', 96, 2, 1),
    ('system_clock_counter', 104, 8, 1),
    ('average_socket_power', 112, 4, 1), ('average_ipu_power', 116, 2, 1),
    ('average_apu_power', 120, 4, 1), ('average_gfx_power', 124, 4, 1),
    ('average_dgpu_power', 128, 4, 1), ('average_all_core_power', 132, 4, 1),
    ('average_core_power', 136, 32, 16), ('average_sys_power', 168, 2, 1),
    ('average_gfxclk_frequency', 174, 2, 1), ('average_socclk_frequency', 176, 2, 1),
    ('average_ipuclk_frequency', 180, 2, 1), ('average_fclk_frequency', 182, 2, 1),
    ('average_uclk_frequency', 186, 2, 1),
    ('current_coreclk', 190, 32, 16),
    ('current_core_maxfreq', 222, 2, 1), ('current_gfx_maxfreq', 224, 2, 1),
]

def read_once():
    paths = glob.glob('/sys/class/drm/card*/device/gpu_metrics')
    if not paths:
        sys.exit('找不到 gpu_metrics(需要 amdgpu)')
    raw = open(paths[0], 'rb').read()
    size, fmt_rev, cnt_rev = struct.unpack_from('<HBB', raw, 0)
    if size != len(raw):
        sys.exit(f'头部长度不符:size={size} 实际={len(raw)},布局可能变了')
    if fmt_rev != 3:
        sys.exit(f'format_revision={fmt_rev}(本工具只测过 v3.0,请重新 pahole 校对布局)')
    d = {}
    for name, off, total, cnt in FIELDS:
        esz = total // cnt
        fmtc = {2: '<H', 4: '<I', 8: '<Q'}[esz]
        v = [struct.unpack_from(fmtc, raw, off + esz * i)[0] for i in range(cnt)]
        d[name] = v if cnt > 1 else v[0]
    return d

def fmt(d, cores=False):
    cC = lambda v: v / 100
    L = []
    p = d
    L.append(f"温度   gfx {cC(p['temperature_gfx']):.1f}°C · soc {cC(p['temperature_soc']):.1f}°C · "
             f"core 最热 {cC(max(p['temperature_core'])):.1f}°C")
    L.append(f"功耗   socket {p['average_socket_power']/1e3:.2f}W = CPU {p['average_all_core_power']/1e3:.2f}W"
             f" + GFX {p['average_gfx_power']/1e3:.2f}W + NPU {p['average_ipu_power']/1e3:.3f}W"
             f" (apu {p['average_apu_power']/1e3:.2f}W · sys {p['average_sys_power']/1e3:.2f}W)")
    L.append(f"频率   gfx {p['average_gfxclk_frequency']}MHz · soc {p['average_socclk_frequency']}MHz · "
             f"fclk {p['average_fclk_frequency']}MHz · mem {p['average_uclk_frequency']}MHz · "
             f"核峰 {p['current_core_maxfreq']}MHz")
    L.append(f"占用   gfx {p['average_gfx_activity']}% · NPU {max(p['average_ipu_activity'])}% · "
             f"C0 最忙 {max(p['average_core_c0_activity'])}%")
    if cores:
        cs = [f"#{i}:{p['current_coreclk'][i]}MHz/{p['average_core_power'][i]}mW"
              for i in range(16) if p['current_coreclk'][i] or p['average_core_power'][i]]
        L.append('每核   ' + ' '.join(cs))
    return '\n'.join(L)

if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('-w', '--watch', type=int, default=0, metavar='N', help='轮询 N 次,间隔 2s')
    ap.add_argument('--json', action='store_true')
    ap.add_argument('--cores', action='store_true')
    a = ap.parse_args()
    n = max(a.watch, 1)
    for i in range(n):
        d = read_once()
        if a.json:
            print(json.dumps(d))
        else:
            if n > 1:
                print(f'── {time.strftime("%H:%M:%S")}')
            print(fmt(d, a.cores))
        if i + 1 < n:
            time.sleep(2)
