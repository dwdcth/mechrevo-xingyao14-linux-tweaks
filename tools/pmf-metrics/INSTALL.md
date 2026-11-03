# pmf-metrics(默认禁用)

曾用于直读 AMD PMF 指标表(amd_pmf 驱动的 smu_pmf_metrics:每核/GFX/NPU 功率)。
**2026-11-03 实测该方案会炸机,read/collect 已加保险丝默认禁用**,保留仅为存档。

## ⚠️ 为什么禁用

本工具经 `/dev/mem` 直接编程 MP1 SMU mailbox(SET_DRAM_ADDR + TRANSFER_TABLE +
清响应寄存器)。当日运行约 9 分钟后 SMU 固件失联 → amdgpu 连锁崩溃(屏幕点不亮、
内核 D 状态死锁、注销无效),只能重启。完整复盘:
[../../docs/notes/smu-mailbox-brick-amdgpu.md](../../docs/notes/smu-mailbox-brick-amdgpu.md)

要点:暂停 amd_pmf 的 `metrics_table_loop_ms` 挡不住 amdgpu 对同一 mailbox 的常驻
流量;SET_DRAM_ADDR 是全局唯一 DMA 目标,用户页退出即悬空。

## 想要分域功耗?用这个代替

[../gpu-metrics](../gpu-metrics/INSTALL.md) —— gpu_metrics sysfs 只读解码,
同样的字段(CPU/GFX/NPU/每核),零风险。本工具已无使用场景。

## 如果确实要解禁(自担风险)

依赖(仅解禁后需要):root;ryzen_smu DKMS 模块且含 0x5D000B 补丁(见
[../../docs/notes/ryzen-smu-5d000b-pm-table.md](../../docs/notes/ryzen-smu-5d000b-pm-table.md));
`collect` 另需 glxgears。

```bash
PMF_MBOX_WRITE_OK=1 pmf-metrics.py read      # 明确知晓风险后
pmf-metrics.py decode [file]                 # 分析历史采样 pkl(无风险,不碰硬件)
```
