# 用户态直写 SMU mailbox 致 amdgpu 全死(2026-11-03)

为测绘分域功耗(CPU/GFX/NPU 瓦数),当日会话从 11:51 起用 `/dev/mem` 直接编程 MP1
SMU mailbox;第五轮脚本(11:59:26)启动 34 秒后 SMU 失联,amdgpu 连锁崩溃,屏幕点不亮,
SSH 可登录但无法恢复,只能重启。**结论:对 SMU 只读,永远不从用户态写 mailbox。**

## 内核时间线(前一启动,-b -1,本地时间)

```
11:59:49  (最后一条正常内核日志,在此之前本次开机无任何 amdgpu 报错)
12:00:00  amdgpu: SMU: No response msg_reg: f resp_reg: 0
12:00:00  amdgpu: in params:00000007            ← 参数 7 = 指标表 ID,正是脚本在发的命令
12:00:00  amdgpu: Failed to export SMU metrics table!   ← 第一张骨牌:MP1 固件失联
12:00:14  amdgpu: Failed to disable gfxoff!      ← gfxoff 开关全靠 SMU,开始连环失败
12:00:26  amdgpu: ring sdma0 timeout, signaled seq=909221, emitted seq=909223
12:00:26  Starting sdma0 ring reset → ring test failed (-110)
12:01:05  GPU Recovery Failed: -62               ← 恢复流程也要 SMU 配合,全败
12:01:07  DPM enable vpe failed, ret = -95
12:02:55  19 个内核线程(ttm/amdgpu-reset-dev)D 状态僵死,load 32+
```

## 致命脚本在做什么

```python
mm = mmap('/dev/mem', 0x1000, offset=0xfd110000)   # MP1 mailbox 寄存器页
REG = {'msg':0xA04, 'resp':0xA08, 'arg0':0xA0C}
def cmd(msg, arg):
    w32('resp', 0)          # ① 清响应寄存器 —— 与内核驱动的在途事务无任何同步
    w32('arg0', arg); w32('msg', msg)
cmd(0x14, phys>>32)         # ② SET_DRAM_ADDR_HIGH ┐ 把 SMU 表传输的全局 DMA 目标
cmd(0x15, phys&0xFFFFFFFF)  # ② SET_DRAM_ADDR_LOW  ┘ 改指到 python 的临时用户页
cmd(0x16, 7)                # ③ TRANSFER_TABLE(id=7) 反复发
# 配套:写 metrics_table_loop_ms=1000000 暂停 amd_pmf 轮询(每轮暂停 2.5s 再恢复)
```

## 死因机制(按证据强度排序)

1. **DMA 目标被全局改走**:SET_DRAM_ADDR 是该 mailbox 的唯一全局传输目标,amdgpu/amd_pmf
   与脚本共用。脚本把它指到自己的 mlock 用户页;python 退出后该物理页被释放,SMU 的传输
   目标成了悬空地址。12:00:00 amdgpu 发出的正是同一条 TRANSFER_TABLE(param=7)。
2. **mailbox 握手竞争**:脚本清 resp / 写 msg 的时机与 amdgpu 的在途事务( gfxoff、
   metrics 轮询每秒都在发)完全无锁交错,可当场搅乱 MP1 固件状态机。
3. **暂停 amd_pmf ≠ 安静**:metrics_table_loop_ms 只停 amd_pmf 一个客户端,amdgpu
   对同一 mailbox 的流量照常,2.5 秒的"等当前周期结束"只是赌运气。

SMU 一死,gfxoff/电源门控全部失效 → SDMA 通道停摆 → ring 超时;而恢复流程(pre_asic_reset
要过 SMU 电源门控)同样依赖 SMU,于是 -62/-95 连环失败,设备到断电前无法复活。
注销无效:僵死的是内核态(GPU + 19 个 D 状态内核线程),与用户会话无关。

## 反证:只读阶段无害

- 10:52–11:40 ryzenadj/ryzen_smu 模块读 pm_table(含高频采样、CPU/GPU 压测同跑)——无恙;
- 11:45–11:47 setpci 只读 SMN index/data——无恙;
- 11:51 起开始**写** mailbox,五轮之内(约 9 分钟)炸机。
- 界线清晰:**经内核托管接口读 = 安全;用户态直写 mailbox = 炸机。**

## 规矩

1. 对 SMU 只读。需要的读数走:`/sys/kernel/ryzen_smu_drv/pm_table`(内核托管)、
   amdgpu hwmon `power1_average`(整 APU 域,RAPL 同源,见 apu-power 笔记)。
2. `tools/pmf-metrics/pmf-metrics.py` 的 read/collect 已加保险丝,默认拒绝执行;
   `PMF_MBOX_WRITE_OK=1` 强制运行 = 自担再炸一次的风险。
3. NPU 分域功耗曾经**没有只读来源**(amdxdna 无 hwmon,pm_table 亦无 NPU 域)。
   → **(已解决,同日)** 分域功耗只读来源其实一直都在:
   - **amdgpu `gpu_metrics`**(sysfs,v3.0/264B):GFX/CPU 域/每核/NPU 功率、每核
     频率/温度/C0。表头是 4 字节 `{u16 size; u8 format_rev; u8 content_rev}`;
     精确布局查**模块 BTF**:`pahole -C gpu_metrics_v3_0 /sys/kernel/btf/amdgpu`。
   - **amdxdna 传感器 ioctl**(`DRM_AMDXDNA_QUERY_SENSORS`,/dev/accel/accel0):
     Total Power(mW)+ 8 列利用率。
   - NPU 负载实测(fastflowlm 跑 Qwen3-0.6B):两路读数同窗对照,峰值
     2505 vs 2368 mW、逐拍形状一致——同一物理量,交叉验证通过 ✓。
   工具:`tools/gpu-metrics/gpu-metrics.py`。
   (fastflowlm 备忘:默认 memlock 8MB 不够,已写 /etc/security/limits.d/
   99-flm-memlock.conf;当场跑用
   `sudo prlimit --memlock=unlimited:unlimited -- setpriv --reuid=$(id -u) --regid=$(id -g) --init-groups flm run <tag>`。)

## 存档

- 崩溃内核日志:`~/amdgpu-hang-20261103/kern-dmesg.txt`(48K,含完整调用栈)
- 全量 journal:`~/amdgpu-hang-20261103/full-journal.txt`
- 崩溃前采到的指标表数据:`~/amdgpu-hang-20261103/pmf-states.pkl`、`pmf2.pkl`
  (decode 子命令仍可分析它们——那是这次事故换来的数据,别浪费)
