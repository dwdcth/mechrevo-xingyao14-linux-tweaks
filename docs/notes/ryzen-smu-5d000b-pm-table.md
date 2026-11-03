# ryzen_smu 0x5D000B 补丁与 Strix Point PM 表测绘(2026-11-03)

星耀14(Ryzen AI 9 HX 370,Strix Point,family 0x1A model 0x24)的 SMU PM 表版本为 `0x5D000B`,社区全链条都不认:AUR `ryzen_smu-dkms`(amkilliam rev d298366,即 nixpkgs 维护的"上游+未合并 PR"集)只认到 `0x5D0009`;RyzenAdj PR#403 把 `0x5D000B` 当 `0x5D0009` 同布局收录(表大小 0xD54,与实测 853 行×4 字节吻合)。

## 模块补丁(已应用)

`/usr/src/ryzen_smu-191.d298366/smu.c` 的 `CODENAME_STRIXPOINT` 分支,`case 0x5D0009:` 后追加一行:

```c
    case 0x5D000B: // XingYao14 AI 9 HX 370 (verified 0x5D000B) - same layout as 5D0009 per RyzenAdj PR#403
```

重建:`sudo dkms remove ryzen_smu/191.d298366 --all && sudo dkms install ryzen_smu/191.d298366 -k $(uname -r) && sudo modprobe ryzen_smu`。

**包重装(内核更新走 dkms 或 paru 重编)会丢补丁,需重打。**

效果:模块加载后 `/sys/kernel/ryzen_smu_drv/pm_table`(3412 字节 float32 数组)可读;ryzenadj 检测到模块后走模块模式正常工作(比 /dev/mem 干净,无撕裂读)。注意内核更新后 dkms 重编用的还是这份打过补丁的 /usr/src 源,补丁不会自己消失,只有包重装才丢。

## 表布局实测(0x5D000B,与 ryzenadj --info 对照验证)

| 字段 | 偏移 | 实测 |
|---|---|---|
| STAPM LIMIT / VALUE | 0x00 / 0x04 | 28 / 27.9(活) ✓ |
| PPT LIMIT FAST / VALUE | 0x08 / 0x0C | 45 / 25.5(活) ✓ |
| PPT LIMIT SLOW / VALUE | 0x10 / 0x14 | 35 / 24.1(活) ✓ |
| PPT LIMIT APU / VALUE | 0x18 / 0x1C | 45 / 恒 0 ✓ |
| TDC LIMIT/VALUE VDD | 0x30 / 0x34 | 54 / 25.4(活) ✓ |
| TDC LIMIT/VALUE SOC | 0x38 / 0x3C | 30 / 1.5 ✓ |
| THM LIMIT / VALUE CORE | **0x40 / 0x44** | 95 / 78.1(Tctl,活)——注意不是 EDC |
| CCLK SETPOINT? | 0xCC | 恒 5.047≈5.05GHz 设定点,busy 值另有其处 |

流传的偏移表 0x40=EDC LIMIT VDD、0x44=THM LIMIT 是错的(两行都标错/错位);`StapmTimeConst@0x9C0` 在本表为 0,错;RyzenAdj PR#403 的 `socket_power@0xD0` 在本机读出恒 5.050(与 0xCC 的时钟值雷同),**5D000B 的实时功率字段与 5D0009 不同布局,不能照抄**。

## 分域功耗:未收敛

- 模块 sysfs 读取干净(中位数法稳定),但**整表找不到 cpu/gfx/npu 分域瓦数字段**:候选偏移要么是占用率/温度(基线 ~50-90、负载归零的反向形态),要么撕裂不可复现。
- 实测机器常驻在 28W STAPM 帽附近(后台负载),合成压测拉不开总功耗差距,四相回归无判别力。要在真正空闲(基线 <15W)时重测才有意义。
- 若要继续:空闲状态下四相回归 + 用 0x04/0x0C/0x14 三个活字段当交叉锚。GPU 活动可用 0x01D4/0x04C8(GPU 压测时 1.4→60.6,疑似 gfx busy/时钟)做旁证。

## ⚠️ 事故(2026-11-03):用户态直写 PMF mailbox = 炸机

为补本节缺失的分域功耗,当日曾用 /dev/mem 直写 MP1 mailbox 取 PMF 指标表,五轮之内 SMU 失联、amdgpu 全死、只能重启。详见 [smu-mailbox-brick-amdgpu.md](smu-mailbox-brick-amdgpu.md)。**本表只经 ryzen_smu 模块读,不再有任何用户态写路径**;`tools/pmf-metrics/pmf-metrics.py` 的 read/collect 已默认禁用。
