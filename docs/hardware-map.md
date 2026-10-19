# 机械革命星耀 14 (XINGYAO P916F-STX) 硬件寄存器与接口映射表

> 生成日期：2026-10-16 · BIOS 1.15 · EC v1.15 (ECSV=0x0F) · WMI 接口版本 0x00020004 (GVER)
> 分析对象：宿主机 ACPI（DSDT+30 SSDT 已反编译至本目录）+ Windows 驱动备份（Ventoy/Mechrevo_Drivers）

## 一、总体架构

```
Windows 控制中心 ──WMAA(WMI)──> DSDT 辅助方法 ──> EC (MMIO 0xFEEC2300) / ECMD burst / ALIB(AMD SMU)
Linux 可用路径：
  ① /sys/firmware/acpi/platform_profile   (amd_pmf 驱动，已可用，走 SMU 不走 EC FTVL)
  ② /dev/mem mmap 0xFEEC2300 (256B EC RAM 镜像，0x00-0x3F 已验证有实时数据)
  ③ 标准电池 sysfs /sys/class/power_supply/LCBT (电压/容量/循环，可用)
  ✗ ec_sys (/sys/kernel/debug/ec/ec0/io) 全零——本机 EC 不走传统 62/66 端口映射，死胡同
```

## 二、WMI 接口（方法块 ABBC0F5B → WMAA，事件块 ABBC0F5C）

输入缓冲 Arg2: `[0]=MFID [1]=SFID [2..]=参数`；返回 `Package{状态Buffer(4B), 数据Buffer(0x100)}`，数据 `[0]=STAT(0=成功)`。
**注意：Linux 用户态无法直接调 WMI 方法，需内核模块或改用下面的 EC 直读写路径。**

| MFID | SFID | 方法 | 功能 | 底层实现 |
|---|---|---|---|---|
| 01 | 01 | GVER | 接口版本 | 常量 0x00020004 |
| 02 | 01 | GFNS | 风扇转速 | 读 EC FNS0/FNS1 |
| 03 | 01/02 | GWLS/SWLS | Win 键锁定 | EC 0x34 bit0 WINS |
| 03 | 03/04 | GFLS/SFLS | Fn 键锁定 | EC 0x32 bit1 KBFN |
| 03 | 05/06 | GCLS/SCLS | Copilot 键锁定 | EC 0x34 bit1 CPLS |
| 03 | 07/08 | GTPS/STPS | Turbo 开关 | 读 EC 0x32 bit0 TPST；写=ECMD burst **0x90(开)/0x8F(关)** |
| 03 | 09/0A | GPFM/SPFM | 性能模式 | 读 EC 0x3F FTVL；写=THMM()+ECMD **0x94(模式1)/0x95(模式2)** |
| 03 | 0B/0C | GKBT/SKBT | 键盘状态字 | 读/写 EC 0x35-0x36 KBST(16bit) |

THMM(mode) 联动 AMD 功耗墙：ALIB(0x0C)，均衡=15W/30W/25W 三档 skin-temp 限制，性能=28W+……（dsdt.dsl:4064）

## 三、EC 寄存器地图（MMIO 基址 0xFEEC2300，长 0x100）

| 偏移 | 名称 | 位宽 | 功能 | 实测值(21:5x) |
|---|---|---|---|---|
| 0x00 | ECMV | 8 | EC 主版本 | 0x01 |
| 0x01 | ECSV | 8 | EC 子版本 | 0x0F (=1.15 ✓) |
| 0x02/0x03 | ECBV/ECTV | 8 | EC 构建/测试版本 | 0 |
| 0x04-0x07 | FLT0-3 | 8×4 | 疑似风扇阈值表 | 6A D1 BC 96 |
| 0x30 | ACIN | bit6 | AC 适配器在线 | 1 ✓ (与 ACAD ONLINE=1 一致) |
| 0x31 | LIDS | bit1 | 合盖状态 | 1 ✓ |
| 0x32 | TPST | bit0 | Turbo 状态 | 1 |
| 0x32 | KBFN | bit1 | Fn 锁 | 0 |
| **0x33** | **KBBL** | 8 | **键盘背光亮度**（待写验证） | 0x00 |
| 0x34 | WINS/CPLS | bit0/bit1 | Win/Copilot 键锁 | 0 |
| 0x35-0x36 | KBST | 16 | 键盘状态字（WMI GKBT/SKBT 目标） | 0 |
| **0x3B-0x3C** | **FNS0** | 16 | 风扇0 RPM（字节序待定，实测 0A 13 ≈ 2579） | ~2500 |
| **0x3D-0x3E** | **FNS1** | 16 | 风扇1 RPM | ~2561 |
| **0x3F** | **FTVL** | 8 | 性能模式值（1=当前/均衡） | 0x01 ✓ |
| 0x70 | APFL/MSFL | bit0/bit1 | 未知标志 | 01 |
| 0x80-0xCF | 电池区 | - | BFCL/BFCH 电流、BVLL/H 电压、BDCL/H 设计容量等 | **MMIO 镜像为零，走 sysfs LCBT** |
| 0x90 | BTPL/H | 16 | 电池温度/参数 | (镜像零) |
| 0xFE | TEMP | 8 | EC 温度 | (未采样) |

ECMD burst 命令（EC 写操作协议，dsdt.dsl:3668，经 WIBE/WOBE burst 握手 + EADD 寄存器）：
`0x90`=Turbo 开, `0x8F`=Turbo 关, `0x94`=性能模式1, `0x95`=性能模式2

## 四、Linux 现状盘点（2026-10-16 实测）

| 功能 | 现状 |
|---|---|
| 性能模式 | ✅ **已可用**：`/sys/firmware/acpi/platform_profile`（low-power/balanced/performance，amd_pmf 提供；实测写入成功，但不联动 EC FTVL——Windows 控制中心的"狂飙"是否=performance 待对比 ryzenadj 功耗验证） |
| 风扇 RPM | ❌ 无 hwmon → 读 EC 0x3B/0x3D 即可 |
| 键盘背光 | ❌ 无 /sys/class/leds 条目 → EC 0x33 KBBL / 0x35 KBST |
| 电池监控 | ✅ LCBT sysfs（13056mV/循环20 实测正常） |
| **电池 80% 限充** | ❌ **未找到寄存器**——不在 EC 地图 0x80+（镜像零）、不在 WMAA 子功能表 → 下一阶段逆向 UWACPIDriver.sys IOCTL 或解码二进制 MOF (wqba.bin) |
| Fn/Win/Copilot 键锁 | EC 0x32/0x34 直读写 |
| 指纹 (2808:1579 FocalTech FT93xx) | ❌ libfprint/AUR 均无支持（专有 TLS 协议）→ **按任务书跳过** |

## 五、方案结论

- tuxedo-drivers **不可用**：本机无 ABBC0F6B/C/D 管理方法块（仅有 ABBC0F5B 方法块 + ABBC0F5C 事件块，新一代 Uniwill 布局）
- 走任务书**方案 B**：Zig 单文件 `mechrevo-ctl`
  - EC 读写：`/dev/mem` mmap 0xFEEC2300（已验证可行；写入前校验 DMI 板号 `XINGYAO Series-P916F-STX` + EC 版本寄存器 0x00/0x01 == 01 0F）
  - 性能模式优先 platform_profile，"狂飙"补充 ryzenadj 功耗墙
  - 键盘背光：写 KBBL 0x33（先读后写、限幅 0-255 或 0-3 需实测）
- 下一阶段逆向目标：`UWACPIDriver.sys`（IOCTL→burst 命令映射，找电池限充）、`wqba.bin` 二进制 MOF 解码、FLT0-3 风扇曲线语义

## 附：文件索引

- `dsdt.dsl` / `SSDT*.dsl`：全量反编译 ACPI 表（WMAA 调度器 dsdt:4514，辅助方法 dsdt:4125-4355，EC 字段 dsdt:3192-3249/3985-4062）
- `wqba.bin`：WMI1 的二进制 MOF（FOMB 头，1092B，OEM WMI 类定义，未解码）
- 驱动备份：`/run/media/$USER/Ventoy/Mechrevo_Drivers/`（核心：`uwacpidriver.inf_amd64_*/UWACPIDriver.sys`）

## 六、阶段2实测结果（2026-10-16 22:50）

### 端口协议探测（ec-probe.zig，/dev/port 方式）
- 标准 0x80 RAM 读命令返回**垃圾数据**（与 MMIO 已验证值不符）→ 本 EC 不支持标准端口 RAM 读
- 16 位寻址试探 0x07B9 返回值不可信（基线协议本身无效）
- 结论：**0x62/0x66 端口只有自定义单字节命令（0x8F/0x90/0x94/0x95），无参数化 RAM 访问**
- 印证 ec_sys 全零现象：内核标准 EC 驱动在本机读不到真实 EC RAM

### 电池限充最终判定
| 访问路径 | 状态 |
|---|---|
| MMIO 窗口 (0xFEEC2300) | ✅ 可用，但只映射 page0（0x00-0xFF），够不到 0x07B9 |
| WMI WMBC (qc71 方式) | ❌ 固件无此方法块 |
| ACPI INOU0000 ECRR/ECRW (UWACPIDriver.sys 方式) | ❌ 固件无此设备（31 张表全查） |
| 标准端口 0x80 读 | ❌ 返回垃圾 |
| 内核 uniwill-laptop.ko | ❌ DMI 白名单仅 TUXEDO/Schenker，且依赖的 WMI 不存在 |

**限充在当前 BIOS (1.15) 上无软件可达路径。** 可选出路：
1. 检查 BIOS Setup（开机 F2）里有无"电池养护/充电阈值"直接选项
2. 关注 MECHREVO BIOS 更新（若新增 INOU0000 设备/WMI WMBC，主线 uniwill-acpi 驱动即可用）
3. Windows 控制中心实测（若能设且重启后 Linux 侧仍停充→EC 级持久，可再深挖）

### 工具链
- `mechrevo-ctl`（Zig 0.17 编译通过并实测验证）：info/fan/kbd get|set/fnlock/turbo/mode
- `ec-probe.zig`：端口协议探测（留作记录）
- mise: zig 0.17.0 / rust 1.99.0 / mr-boxington 1.24.0（rust 配置 mr_boxington=true 自动包裹 cargo）

## 七、麦克风勘误与修复（2026-10-16 23:55，结合官方规格）

**官方规格**：内置双麦克风阵列（摄像头两侧开孔，立体声拾音+降噪），侧边 3.5mm 复合接口。

**修正后的物理接线**（与实测完全吻合）：
- 内置 2 麦阵列 → **ALC256 模拟输入 node 0x12** = PipeWire "Stereo Microphone"(Mic2) ✅ 默认源，回路测试证实拾音
- ALC256 codec 的 DMIC 引脚 → 未接线 = "Digital Microphone"(Mic1) 录满幅方波 ❌ 已禁用
- AMD ACP PDM 控制器（acp-pdm-mach 卡）→ SoC 能力枚举但未接麦克风，输出满幅垃圾，忽略
- 3.5mm 耳机麦 → node 0x19

**已部署**：
- `~/.config/wireplumber/wireplumber.conf.d/50-mic-priority.conf` 禁用 Mic1
- `~/.config/systemd/user/mic-default.service` 开机设默认源=Mic2（手动重启音频服务后需补跑）
- Mic Boost=2；EasyEffects 输入预设 `mic-noise-reduction.json`（rnnoise 降噪 +6dB，对应 Windows Voice Clarity 的替代）

## 八、FN 功能键事件码与 OSD（2026-10-17 实测定稿）

**关键发现**：FN+F3/F8 的功能由 EC 直接执行，同时经 WMI 事件块 ABBC0F5C 上报；但 `huawei_wmi` 驱动用 sparse_keymap，**未映射的码只打内核日志（"Unknown key pressed"）不上报 evdev** —— /dev/input/event7 上读不到任何东西，唯一通道是 /dev/kmsg（dmesg_restrict=1，需 CAP_SYSLOG）。

**事件码表**（dmesg `input inputNN: Unknown key pressed, code: 0x00XX`，inputNN 序号随启动变化需运行时解析）：

| 码 | 键 | 语义 |
|---|---|---|
| 0x30 / 0x31 | FN+F3 | 触控板 关 / 开（码即状态，EC 直接执行） |
| 0x20 / 0x21 / 0x22 | FN+F8 | 键盘灯 关 / 暗 / 亮（码即档位，EC 直接执行） |
| 0xa1 | FN+F7 | 麦克风静音键（**EC 不执行**，仅上报；用户态 wpctl 切默认源静音） |
| 0xa0 | FN+F9 | 飞行模式键（**EC 不执行**，仅上报；用户态 rfkill block/unblock all） |
| 0x50 / 0x51 | FN+Esc | Fn锁 关 / 开（码即状态，EC 直接执行；已接 OSD showText） |

亮度 FN+F1/F2 不走此通道（video bus → amdgpu，原生 OSD）。

**OSD 实现**（`mechrevo-ctl osd` + `/etc/systemd/system/mechrevo-osd.service`）：
- 服务以 `User=$USER` + `AmbientCapabilities=CAP_SYSLOG` 运行（不给 root）—— dbus-broker **拒绝异 uid 连接用户总线**，root 服务调 plasmashell 会 ENOTCONN
- 尾部 /dev/kmsg → 匹配码 → `busctl --address=unix:path=/run/user/1000/bus call org.kde.plasmashell /org/kde/osdService org.kde.osdService {touchpadEnabledChanged b / keyboardBrightnessChanged i}`
- **150ms 静默合并 + 异步发射**：连按只发最终态。直接每按一发会把 plasmashell OSD 动画刷残卡屏（12 连发实测复现）；同步发射会排队滞后
- 键盘灯档位映射 0/50/100；触控板 false/true
- udev uaccess tag 与该设备既有 power-switch tag 冲突不落地（ACL 不生效），曾用 OWNER 直授后随方案废弃删除

**FN+F7/F9 用户态实现**（2026-10-17，EC 对两键完全不动手，rfkill/音量实测零变化）：
- 0xa1 → `wpctl set-mute @DEFAULT_AUDIO_SOURCE@ toggle` + 读回状态 → showText OSD（麦克风图标）
- 0xa0 → 读 /sys/class/rfkill/*/soft+hard 判断方向 → `rfkill block/unblock all` → wifiEnabledChanged OSD
- 动作键**逐次执行不合并**（toggle 语义）；显示键（灯/触控板）保持 150ms 合并
- 服务能力升级：`AmbientCapabilities=CAP_SYSLOG CAP_NET_ADMIN`（kmsg + rfkill；DAC 由 /dev/rfkill 自带 uaccess ACL 放行）+ `XDG_RUNTIME_DIR`（wpctl 连 PipeWire）

**不做项**：CapsLock/NumLock OSD —— fcitx 已有大写提示，且该键盘无数字小键盘区（无 NumLock 键）。Windows OSD 的此项为被吐槽的遮挡源，无迁移价值。
