# 适配器输出功耗不可读 + EC 遥测窗口(ERAM)电池字段实测(2026-11-03)

结论先行:**这台机器没有任何途径读到适配器(AC 输入)的实际输出功率**;
整机功耗 = 适配器输出 + 电池输出(带符号),所以插电时只能估(见
[tools/mr-power](../../tools/mr-power/INSTALL.md) 的 calibrate 标定路线),
拔电时电池输出即整机精确值。

## 适配器遥测:全链路排查记录

| 途径 | 结果 |
|---|---|
| DSDT ERAM 字段图(本机 ACPI 反编译) | 只有 `ACIN`/`ACLW` 两个状态位(0x330 bit6/bit7),无 V/I/功率 |
| 同机型逆向(YMGPwcca/Mechrevo-Xingyao-14 全套文档) | 同结论;其能量分析明写"wall-side adapter input was not measured" |
| `/sys/class/power_supply` Mains(ACAD) | 仅 `online` 标志 |
| 充电芯片(BQ257xx 家族)iio | 无通道 |
| `/sys/class/typec` + usb_power_delivery | 空目录(UCSI 未暴露,连 PD 协商档位都没有) |
| ~~EC 独占 SMBus 直读充电 IC~~ | 理论上有寄存器,但抢 EC 总线有挂机风险,不做([[smu-mailbox-brick-amdgpu.md]] 的教训:共享总线别碰) |

## ERAM 窗口电池遥测逐项验证(只读 MMIO 0xFEEC2300..FF)

DSDT `OperationRegion (ERAM, SystemMemory, 0xFEEC2300, 0x0100)` 字段与本机实测
(dd + /dev/mem 只读,2026-11-03):

| 字段 | 窗口偏移 | 实测 | 对照 sysfs(LCBT) |
|---|---|---|---|
| BATI/BAII/BACG/BAIC 标志 | 0x380 bit0-3 | 0x17 | present 等 |
| BFC 满充容量 | 0x381(u16) | 6256mAh | ~73Wh ✓ |
| BRM 剩余容量 | 0x383(u16) | 4943mAh | 4943/6256=79.0% ✓ |
| BDC 设计容量 | 0x385(u16) | 6392mAh | 健康度 ~98% |
| **BVL 电池电压** | **0x388(u16)** | 12553mV | 12550mV ✓ |
| **BCR 电池电流** | **0x38A(u16)** | 0mA | POWER_NOW=0 ✓(有符号,放电为正/负按 EC 口径待标) |
| BDV 设计电压 | 0x38C(u16) | 11730mV | VOLTAGE_MIN_DESIGN=11730 ✓ |
| BTP 出发点 | 0x390 | 500 | _BTP trip point(mAh) |
| SRNM 序列号 | 0x392(u16) | 6311 | 电池 SN |
| **RSO SOC** | **0x394(u16)** | 80 | 79%(判决值向上取整)✓ |
| BDVN 型号串 | 0x3A0(128b) | "L1421_A1" | 电池型号 ✓ |

电池 sysfs 数据本来就是 ACPI 从这个窗口读的——没有新信息,但证明**纯只读 MMIO
可以稳定采样全部电池遥测**(窗口是固件定义给 host 的数据区;EC 写、host 读)。
窗口里另有风扇转速 FNS0/FNS1@0x33B/0x33D、EC 温度@0x3FE,均只读可得。

## 实用口径

- **拔电**:整机 = 电池输出(sysfs POWER_NOW 或 ERAM BVL×BCR),精确
- **插电**:整机 ≈ APU(gpu_metrics 封装)+ 标定"其余";适配器输出 ≈ 整机 − 电池放电。
  "其余"(屏/内存/盘/风扇/VRM)实测 8-13W 随负载波动——`mr-power calibrate` 标定
- EC 在限充点附近状态会抖(AC 在线也报 Discharging 且 POWER_NOW≈0 或出现 ~10W
  主动放电),整机口径只在 AC 离线时采信电池读数
