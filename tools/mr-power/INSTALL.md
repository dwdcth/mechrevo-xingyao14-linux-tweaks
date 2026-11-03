# mr-power 安装

Ryzen SMU 功耗墙（PPT）快捷脚本：`quiet`=12W 静音档、`<N>`=钳到 N 瓦（5-65）、`stock`=按当前 Fn+X 档位用 WMI 重写恢复、`info`=查当前限额。中文输出。

## 前提

- **ryzenadj**：`mise use -g github:FlyGoat/RyzenAdj`（上游发 manylinux 免编译二进制）或 AUR `ryzenadj`
- **mechrevo-ctl** 已装（`stock` 子命令走它的 WMI 通道）：见 [../mechrevo-ctl/INSTALL.md](../mechrevo-ctl/INSTALL.md)

## 安装

```bash
sudo install -Dm755 mr-power /usr/local/bin/mr-power
```

脚本会经 `SUDO_USER`（兜底 uid 1000）定位该用户 mise 目录下的 ryzenadj 真实路径——root 的 mise 不信任用户 shim，不能直接调。

## 用法与验证

```bash
mr-power info     # 功耗墙 + SMU 实时功率 + 分域功耗（CPU/GFX/NPU/封装/系统）
mr-power status   # 实时功耗：SoC 封装 = CPU+GFX+NPU+其余 · 电池（AC 离线放电时给出整机实际功耗）
mr-power quiet    # 12W
mr-power 20       # 20W
mr-power stock    # 恢复机器默认
```

`status`/`info` 分域的数据源：amdgpu `gpu_metrics` sysfs（**v3.0**，264B；表头 `{u16 size; u8 fmt; u8 cnt}`，布局经模块 BTF `pahole -C gpu_metrics_v3_0 /sys/kernel/btf/amdgpu` 实测）——`average_all_core_power`/`average_gfx_power`/`average_ipu_power`/`average_socket_power`/`average_sys_power`，NPU 负载下与 amdxdna 传感器 ioctl 交叉验证一致。布局不符时自动回退 amdgpu hwmon `power1_average`（整域：实测纯 CPU 压测从 16W 跳到 44W 对上快速墙，与 RAPL 同域一致）。电池按 `POWER_SUPPLY_TYPE=Battery` 匹配（本机 EC 名叫 LCBT），瓦数与状态来自 uevent。

## 注意

- **切 KDE 电源模式或按 Fn+X 换档不会自动解除 ryzenadj 钳制**（PMF 不重写 PPT），恢复用 `mr-power stock`、Fn+X 按两下或重启
- 空闲功耗与功耗墙无关（实测空闲 ~4W 不随钳制变化）
- **AC 输入功耗本机读不到**：Mains 电源只报 online 标志（无电压/电流），充电芯片也不出 iio 通道。想知道真实整机功耗（含风扇/屏/盘/内存）：拔电源用 `mr-power status`，AC 离线放电时电池输出就是整机实际功耗（EC 在限充点附近状态会抖，AC 在线时即使报 Discharging 也不采信）
- `info` 的「实时功率(SMU)」行取自 PM 表的 STAPM/PPT VALUE 字段，整域口径，与分域行的 SoC 封装互相印证
- **PM 表 `0x5D000B`（Strix Point）**：本机已给 `ryzen_smu`（AUR ryzen_smu-dkms）打补丁接受该版本（同 5D0009 布局；补丁在 `/usr/src/ryzen_smu-191.d298366/smu.c`，包重装会丢）。但该表**实时功率字段与 5D0009 有出入**（`socket_power@0xD0` 在本机是时钟字段）——分域功耗不要指望 pm_table，用上面的 gpu_metrics 即可（详见 [../../docs/notes/ryzen-smu-5d000b-pm-table.md](../../docs/notes/ryzen-smu-5d000b-pm-table.md) 与 [../../docs/notes/smu-mailbox-brick-amdgpu.md](../../docs/notes/smu-mailbox-brick-amdgpu.md)）
