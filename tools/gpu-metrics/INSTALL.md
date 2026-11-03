# gpu-metrics 安装

只读解码 amdgpu `gpu_metrics` sysfs(**v3.0**,Strix Point 实测):CPU/GFX/NPU 分域功耗、每核功率/频率/温度/C0、SoC 封装与系统功率、降频驻留。零依赖单文件。

## 依赖

| 依赖 | 说明 |
|---|---|
| Python ≥ 3.7 | 仅标准库(struct/glob/json/argparse),无 pip 包 |
| amdgpu 驱动 | 读 `/sys/class/drm/card*/device/gpu_metrics`,无需 root(文件全局可读) |
| 表布局 v3.0 | 头部校验 `{u16 size; u8 fmt; u8 cnt}`,size=264/fmt=3;不符会明确报错而不是乱解 |

无需 DKMS 模块、无需 ryzenadj、无需 root。

## 用法

```bash
gpu-metrics.py             # 打印一次(温度/分域功耗/频率/占用)
gpu-metrics.py --cores     # 额外打印每核 频率/功率 明细
gpu-metrics.py -w 5        # 5 轮 × 2s 连续观测
gpu-metrics.py --json      # 机器可读(供其他脚本消费)
```

输出示例:

```
功耗   socket 5.43W = CPU 1.19W + GFX 0.52W + NPU 0.000W (apu 5.43W · sys 10.99W)
频率   gfx 661MHz · soc 615MHz · fclk 1231MHz · mem 848MHz · 核峰 3519MHz
```

## 布局来源与内核升级后的校对

字段偏移取自**运行内核的模块 BTF**(vmlinux BTF 里没有,amdgpu 是模块):

```bash
pahole -C gpu_metrics_v3_0 /sys/kernel/btf/amdgpu
```

表头是 4 字节 `{u16 structure_size; u8 format_revision; u8 content_revision}`——网上流传的
"metrics_header 6 字节"说法是错的。内核升级改变了布局时脚本会拒绝输出并提示重新校对;
把新 pahole 偏移更新进脚本顶部 `FIELDS` 表即可(注意 pahole 第三列是成员**总大小**,
数组元素大小 = 总大小/元素数)。

## 数据可信度

- NPU 域(`average_ipu_power`)与 amdxdna 传感器 ioctl(`DRM_AMDXDNA_QUERY_SENSORS`)
  在 NPU 负载下交叉验证:峰值 2505 vs 2368 mW、逐拍形状一致 ✓
- `average_core_power[16]` 偶发单槽异常大值(乱跳),**域级合计正常**,每核瓦数仅供参考
- 全程内核托管(amdgpu 驱动内部有锁的 SMU mailbox 流量),用户态纯读——SMU 只读铁律
  背景见 [../../docs/notes/smu-mailbox-brick-amdgpu.md](../../docs/notes/smu-mailbox-brick-amdgpu.md)
