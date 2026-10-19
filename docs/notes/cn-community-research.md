# 机械革命星耀14（P916F-STX）Linux 充电阈值/养护模式——中文社区调研报告

调研日期：2026-10-17
调研范围：中文搜索引擎（必应/Z.ai）、linux.do（L 站）、Arch 中文维基/中文论坛、知乎、B站、V2EX、GitHub、内核文档
目标机型：MECHREVO XINGYAO P916F-STX（Ryzen AI 9 365，BIOS 1.15，EC=ITE IT5571 固件 1.15，Uniwill/AIstone 公模）

---

## 0. 一句话结论

**有人在和你完全相同的机器（同 BIOS 1.15 / EC 1.15）上已经解决了 80% 充电上限**：GitHub 用户 YMGPwcca 的 `Mechrevo-Xingyao-14` 仓库（2026-09，CC BY 4.0）逆向出 IT5571 的 **ITE PMC2 端口协议**（I/O 0x68/0x6C，命令 0xF1/0xF2/0xF3 家族），live 验证了 `T1=80 / T2=100` 即为实际 80% 充电上限；同时明确否定了把无界14X 的 `0x07B9`/`0x07C3` I2EC 技巧照搬到本机的可行性（`0x07B9` 在本机 EC 固件中不是控制地址，I2EC 0x380 通道实测 cross-check 失败）。仓库已 clone 到 `refs/Mechrevo-Xingyao-14/`。

---

## 1. 最高价值发现：同款机器的完整解决方案

### 1.1 YMGPwcca/Mechrevo-Xingyao-14（GitHub，2026-09）

- 仓库：https://github.com/YMGPwcca/Mechrevo-Xingyao-14 （GitHub topic `p916f-stx`、`battery-charge-limit` 均指向它）
- 调查对象与你的机器逐项吻合：`MECHREVO XINGYAO Series-P916F-STX`、AI 9 365、Radeon 880M、32GiB、2880×1800、BIOS 1.15、EC `IT557x V1.09 E00 - 20230831`（固件 UI 标签 EC 1.15）、ITE 配置口 0x4E、Chip ID 0x5571 Rev 0x07、实测环境 CachyOS。电池型号 `588974-3S-G-A0`，SSD `YMTC PC41Q-1TB-B`。
- 文档体系非常严谨（evidence matrix / validation / open-questions 分层，标注 Live-confirmed vs Static-confirmed）；NOTICE.md 声明部分内容为 AI 辅助整理，但技术断言均锚定实测抓包与固件反汇编。
- 许可：原文档 **CC BY 4.0**（引用/复用需署名）。

**核心协议（对目标机 live 验证过）**——PMC2 传输层（KBC 风格，非 WMI、非 I2EC）：

| 操作 | 写 0x6C | 写 0x68 | 验证状态 |
|---|---|---|---|
| 禁用/复位 | `0xF1` | `0x10` | 仅静态确认，未实测 |
| 使能 | `0xF1` | `0x11` | **Live** |
| 读使能状态 | `0xF1` | `0x12` | **Live** |
| 读 T1 | `0xF1` | `0x13` | **Live** |
| 读 T2 | `0xF1` | `0x14` | **Live** |
| 设 T1 | `0xF2` | 阈值字节（80=0x50） | **Live**（80、85） |
| 设 T2 | `0xF3` | 阈值字节（100=0x64） | **Live**（100、90） |

时序：等 IBF(bit1)=0 → 命令字节写 0x6C → 等 IBF=0 → 数据字节写 0x68 → 读回类命令等 OBF(bit0)=1 后从 0x68 读。阈值范围 0..100（0x00..0x64，含 100，固件用 8051 带借位 SUBB 判定）。

**三区语义（85/90 实验 live 验证）**：`SOC<T1` 充电；`T1≤SOC≤T2` 保持（Not charging）；`SOC>T2` **主动放电**（AC 在线时电池净输出，约 10W，BQ25700A/BQ25710 家族 EN_LEARN 式行为的家族级推断）。因此 `T1=80/T2=100` 即常规意义的 80% 上限。

**EC 内部地址（静态确认+live 关联）**：使能位 `XRAM[0x0D01].bit4`、T1 `XRAM[0x0D13]`、T2 `XRAM[0x0D14]`、判决用 SOC `XRAM[0x0394]`。H2RAM 窗口：host `0xFEEC2300..0xFEEC23FF` ↔ EC XRAM `0x0300..0x03FF`（即 SOC 可直接 MMIO 读 `0xFEEC2394`）。注意 0x0D13/0x0D14 **不在** H2RAM 窗口内，只能走 PMC2。

**持久性（重要工程结论）**：
- 普通重启、正常关机（含拔 AC，内电池不断）→ 状态保留（1/85/90 读回不变）；
- Linux 写好后在 Windows 侧用同族 PMC2 读回一致（跨 OS 保留）；
- **电池耗尽导致整机完全断电 → 状态被清 0/0/0**。
- ⇒ 生产实现应做"开机读-比较-按需恢复"的幂等 systemd 服务；`0xF1 0x10` 复位路径仅静态确认，恢复流程并不需要它。

**该仓库明确否定的路径（别再踩）**：
- 通用 sysfs：`charge_control_start/end_threshold`、`charge_behaviour` 在 LCBT 上不存在（我在本机 7.2.10-cachyos 内核复核，仍然不存在）；
- 华为兼容 WMI（机器上竟有 `huawei-wmi` 平台设备）：GET 0x1103 返回不支持；
- **通用 Uniwill/同方偏移 `0x07B9`、`0x07D0`：在本机固件里只是比较用软件常量，不是控制地址**；
- `INOU0000`/`ECRR`/`ECRW` ACPI 接口不存在；
- **专用 I2EC 基址 0x380：只读测试全部返回 0xFF，被 cross-check 否定**（同时 H2RAM/MMIO 路径读到合理 SOC=92）；
- ACPI `_BTP` 只是电池 trip point，与充电限制无关。

### 1.2 与你既有认知的关系

你已知的无界14X 方案（w568w 逆向 + I2EC 0x07B9/0x07C3=0x04）**不可直接迁移**：P916F 的 IT5571 固件把充电限制做成了独立命令控制子系统（0x0D13/0x0D14 + PMC2 命令），而不是无界14X 那种裸 XRAM 寄存器 + 门控同步。你在 `re/battery_analysis.md` 里观察到的"写 0x07B9<85 会关掉 T2 影子但不停充"与该仓库"0x07B9 非本机控制地址"的结论互相印证。你计划中的 H2RAM/MMIO 路线与该仓库一致（且其 PMIO/PMC2 端口比 MMIO 写更安全，是固件定义的正规命令通道）。

### 1.3 我在本机做的复核（2026-10-17）

- `uname -r` = 7.2.10-1-cachyos（>6.19，主线已含 uniwill-laptop 驱动）；
- `lsmod` 无 `uniwill`/`tuxedo` 模块加载 → **主线 uniwill-laptop 的 DMI 识别不覆盖本机**；
- `/sys/class/power_supply/LCBT/` 无任何 `charge*threshold*`/`charge_behaviour` 属性。
⇒ 问题 6 的答案：**本机在当前内核下没有开箱即用的 `charge_control_end_threshold`，也没有任何现存内核驱动绑定**；upstream 也没有针对 P916F-STX 的报告（GitHub/邮件列表均未搜到）。

---

## 2. 家族级 EC 充电门控研究（同公模不同分支，理论价值高）

### 2.1 losewayy/uniwill-ec-charge-limit + open-revo Issue #38（2026-09-25）

- 仓库：https://github.com/losewayy/uniwill-ec-charge-limit （MIT；很新，工具为 Windows：`pin_limit.exe` 依赖 `UWACPIDriver.sys`；Linux 下可借用其寄存器结论）
- Bug 报告：https://github.com/faintonce/open-revo/issues/38
- 实测机型：蛟龙16 Pro 2025（8945HX + 5070Ti，BIOS N.1.21MRO34，EC 1.32）。
- **门控逻辑（对 w568w 技巧的普适化解释）**：
  `state_is(v) = (xram[0x07C3]==v) || (xram[0x0770]==v)`；`limit_enabled = state_is(4) || state_is(5)`。
  这解释了无界14X 的"覆写 0x07C3=0x04 触发 EC 同步"为什么有效；而当 0x07C3 是别的值（蛟龙16 Pro 为 7）时，**改写 ROMID 首字节 `xram[0x0770]=0x04`（0x700-0x7FF 可写窗口）同样能开门**，约 5 秒后 0x0742.bit2 与 0x07B9.bit7(REACHED) 置位，放电到 55% 后回充精确停在 60%。
- 写入是易失的 → 需 `--watch` 周期重写或开机自启；`--off` 恢复。
- 风险提示：ROMID 是 OEM 产品标识魔数（TUXEDO maintainer 原话"打开未测试代码路径"）；约 2020 年老机型启用充电限制可能永久损坏电池（引用 CVE-2026-64143）；只在一台机器一种 EC 版本上验证过。
- 对你的意义：这是理解 Uniwill 家族"设了不生效"的钥匙；但 **P916F 走的是另一套 0x0D13/0x0D14+PMC2 子系统**，无需也无处套用该门控。

### 2.2 w568w 两篇 gist 及评论区（无界14X/14XA 暴风雪，你已知，补充要点）

- 充电阈值 gist：https://gist.github.com/w568w/957976b59906e0ce5d6c13ad342e1593
  - 0x07B9 发现史：先误判 0x7CC（软关机即丢、抓包污染导致假阳性），后靠 Windows 活体运行时对比 EC RAM dump 定位 `0x07B9`=充电上限（动态）；
  - EC 固件 bug：进入 S5 后固件会把 `xram[0x07C3]` 重写为 0x05 且之后写不回 0x04（ROMID 0x0770 也无回写）→ 偏门 I2EC 技巧在关机后失效，需每次开机重设；
  - 直接写 0x07B9 在新 EC 固件上会与 EC 抢写（固件周期刷 100），必须开 0x07C3=0x04 的同步门。
- 控制中心逆向 gist：https://gist.github.com/w568w/b2fc5f9d1f4dff13efe751abec27b396
  - 评论区含中国用户补充：建议把寄存器知识落到 tuxedo-io/uniwill_wmi 驱动改造里，而不是裸 I2EC 工具（长期方向）。

---

## 3. 同公模（Uniwill/AIstone/同方）生态与工具

| 资源 | 要点 |
|---|---|
| **Arch 中文维基：Mechrevo 无界14X** https://wiki.archlinux.org.cn/title/Mechrevo_WUJIE14X （英文版 https://wiki.archlinux.org/title/MECHREVO ） | 最系统的中文汇总：推荐 `mechrevo-drivers-dkms`（AUR）；充电限制接口为 `/sys/devices/platform/tuxedo_keyboard/charging_profile/charge_type`（high_capacity/balanced/stationary 三档，对应 Windows 控制中心的低/中/高三档养护）；注明部分机型设置不生效的已知问题；引用 w568w ACPI 逆向与 sund3RRR 修复 |
| **AUR: mechrevo-drivers-dkms** https://aur.archlinux.org/packages/mechrevo-drivers-dkms | 基于 tuxedo-drivers 修改的机械革命 DKMS 驱动包（v4.22.1）；archlinuxcn 论坛 [t/15616](https://forum.archlinuxcn.org/t/topic/15616) 有机械革命用户讨论（uniwill_wmi 模块、禁用后风扇键盘失控等） |
| **LongSang01/wujie14X-Linux-Driver** https://github.com/LongSang01/wujie14X-Linux-Driver | 无界14X：先用 TUXEDO 驱动（充电上限/风扇可用，TDP 锁 45W），后按 w568w 逆向自制 acpi_call 版控制中心（Go），**充电百分比上限可用**，systemd service 示例 `-limit-up 80 -kbd-level 0`；2026-07 更新 |
| **faintonce/open-revo** https://github.com/faintonce/open-revo （352 star） | Windows 下的开源控制中心替代（Tauri+Rust+React），支持 8 种模具 24 种机型（40/50 系游戏本），含电池健康充电阈值 100/80/60 三档；**不含星耀14**；其 issue #38 即上文 losewayy 分析 |
| **sund3RRR/mechrevo14X-linux** https://github.com/sund3RRR/mechrevo14X-linux | 无界14X（同方 GX4HRXL 板 = TUXEDO InfinityBook Pro 14 Gen9）：给 tuxedo-drivers 打补丁加 MECHREVO vendor 后可用 TCC；另含 `amdgpu.dcdebugmask=0x10`（eDP 花屏）、`acpi.ec_no_wakeup=1`（睡眠秒醒）、`i8042.nomux`（挂起后键盘）、yt6801 网卡驱动 |
| **xuwd1/mechrevo-wujie14-kmod** https://github.com/xuwd1/mechrevo-wujie14-kmod （AUR: mechrevo-wujie14-dkms） | 无界14（IP3 xN39，非 X）内核模块：platform_profile 三档性能模式 + 键盘背光 sysfs；不涉及充电 |
| **Wer-Wolf/uniwill-laptop** https://github.com/Wer-Wolf/uniwill-laptop | tuxedo-drivers 中 uniwill 部分的独立 fork；issue #3 报告 NUC X15 上充电控制不可用（同族"设了不生效"问题旁证） |

### 相关中文装机/踩坑博文（同公模经验，可交叉参考）

- 知乎：[无界14X暴风雪 Arch Linux 踩坑记录](https://zhuanlan.zhihu.com/p/730538041)（amdgpu、BIOS 成熟度感慨）
- icyleaf：[Arch Linux 勇闯机械革命暴风雪](https://icyleaf.com/gears/wujie-14x-laptop)（无界14X 暴风雪，全面但未涉及充电阈值）
- atomopo：[机械革命无界14X 折腾 Fedora 记录](https://atomopo.me/2026/01/18/mechrevo-linux/)（H255 斗战版，TUXEDO 驱动路线）
- zeeko.dev：[提升无界15XPro 暴风雪的充电速度](https://zeeko.dev/2025/08/improving-mechanical-revolution-15xpro-charging-speed)（TCC 里把 USB-C 充电策略设为 Prioritize battery charging speed）
- rikka.im：[无界15X Linux 优化指南](https://rikka.im/post/mechrevo-wujie15x-linux)（mechrevo-drivers-dkms + TCC，评论区有翼龙15Pro 用户咨询——不同型号控制台是否一致）
- NaiveTomcat：[机械革命 Umi Pro 3 的 Linux EC 踩坑记录](https://blog.naivetomcat.cn/pages/0214cf)（旧款 Uniwill 机 EC 逆向，含充电阈值 0x07B9/0x07C3、EAEC 相关）
- Xu Zhao（khyan）：机械革命=Uniwill 志合讯信代工的说明文（library.khyan.top）
- V2EX：[有没有 Linux 下充电到指定电量就停止充电的软件](https://www.v2ex.com/t/1036612)（通用原理讨论：阈值最终都在 EC/BIOS 层）

### 主线内核状态（问题 6 的背景）

- `uniwill-laptop` 驱动 **v6.19 起进入主线**（源起 LWN 2025-08 的 "Add support for Uniwill laptop features" 系列；仓库 https://github.com/Wer-Wolf/uniwill-laptop ），文档 https://docs.kernel.org/admin-guide/laptops/uniwill-laptop.html ：支持 `charge_control_end_threshold`（1-100）与 `charge_types`（Standard/Trickle/Long Life），并内置"部分机型阈值实现不良、强制开启可能伤电池"的黑名单。
- 7.0/7.1 有充电相关修正（阈值初始化、接受 0、force 参数修复），更多排队在 7.3（patchew 系列，wse@tuxedocomputers.com）。
- **但本机（P916F-STX）DMI 不被识别、模块未加载（本机实测）**；tuxedo-control-center#268 也说明老 tuxedo-drivers 只有三档 profile、无标准 sysfs。给 P916F 提 DMI 适配/quirk 是一个可行的上游贡献方向（可参考 YMGPwcca 仓库的 PMC2 协议写 patch）。

---

## 4. linux.do（L 站）与论坛线索

L 站搜索页有 Cloudflare 防护，无法完整抓取，以下经搜索引擎片段确认：

- [有没有用机械革命装linux的佬](https://linux.do/t/topic/1158701)（2025-11）：无界14X 装 Ubuntu 24 的快捷键问题；有回复称蛟龙16K 在 Linux 下 Tuxedo 方案"完全支持"。
- [linux.do/t/topic/1338229](https://linux.do/t/topic/1338229)：搜索片段含"充电阈值/电池充电上限（设定为60%）？我有蛟龙16K……完全支持……"（页面未能完整抓取，标题与上下文存疑，需登录 L 站后核实）。
- [华硕笔记本在Linux系统下如何设置智能充电至80%后停止](https://linux.do/t/topic/1875733)（2026-04）：L 站同类问题（华硕），旁证社区对 80% 养护的普遍需求。
- 未发现任何 L 站帖子讨论**星耀14** 的 Linux 充电阈值（"星耀"关键词在 L 站只命中选购讨论，如 [t/838375](https://linux.do/t/topic/838375?page=2)）。
- Arch 中文论坛：[同方模具笔记本相关问题盘点 t/15616](https://forum.archlinuxcn.org/t/topic/15616)（mechrevo-drivers-dkms 实战）、[笔记本充电阈值 t/12083](https://forum.archlinuxcn.org/t/topic/12083)（TLP 通用）、[无界14 2026 兼容性询问 t/17211](https://forum.archlinuxcn.org/t/topic/17211)、[Arch 笔记本充电慢 t/15827](https://forum.archlinuxcn.org/t/topic/15827)。

---

## 5. 对本项目（mechrevo-star14）的行动建议

1. **直接采用/复现 PMC2 方案**：你的 `ec-probe.zig` 已具备 PMIO 能力；按 1.1 的命令表实现 `limit-get`/`limit-set T1 T2`/`enable`，`T1=80,T2=100` 即目标 80%。先 `0xF1 0x12/13/14` 读现状，再写。第三方在相同 BIOS/EC 1.15 上 live 验证过，风险等级远低于裸 XRAM 写。
2. **持久化服务**：开机（及 resume）时 read-compare-restore（电池耗尽断电会清状态）；避免不必要 EC 写。
3. **不要**：套用 0x07B9/0x07C3/0x0770 门控技巧（本机无此控制路径）、碰 I2EC 0x380（已被否定）、开 ec_sys 写、盲扫 PMC2 命令。
4. **可与 YMGPwcca 联系协作**（同机型、结论互补：他们的 PMC2 协议 + 你的 H2RAM/SMBus/ACPI 侧积累）；其文档 CC BY 4.0，复用需署名。
5. 若想回馈上游：给主线 uniwill-laptop 或 mechrevo-drivers-dkms 提 P916F 的 DMI/quirk + PMC2 charge threshold 支持，是最顺的落地路径。

---

## 6. 未找到 / 无法核实的内容（如实说明）

- **没有任何人报告过星耀14/STX 在 Linux 下 `charge_control_end_threshold` 开箱可用**；本机实测亦不可用。
- 除 YMGPwcca 仓库外，未发现第二个 P916F/星耀14 的 Linux 逆向公开成果；GitHub 搜 "P916F" 仅命中该仓库（topics: p916f-stx、battery-charge-limit）。
- Reddit r/linuxhardware 帖 1ng8iec（"How to enable 80% charge limit on MECHREVO-brand..."，2025-10，AI 9 365 + 32GB 疑似同硬件用户）被网络防护拦截，内容未能读取——**值得你登录 Reddit 人工查看**。
- linux.do t/1338229 的完整内容未能抓取（Cloudflare）；B 站未发现机械革命 Linux 充电阈值视频（仅 Windows 教程与充电器评测）。
- AUR mechrevo-drivers-dkms 的评论区（Anubis 反爬）未能读取，其中可能有 "wajwtup mode" 等用法讨论。

---

## 7. 链接总清单

**同款机型**
- https://github.com/YMGPwcca/Mechrevo-Xingyao-14 （已 clone 至 `refs/Mechrevo-Xingyao-14/`；重点读 `docs/battery-charge-limit.md`、`docs/battery-threshold-semantics.md`、`docs/battery-limit-power-loss-observation.md`、`docs/embedded-controller.md`、`docs/linux.md`）
- https://github.com/topics/p916f-stx 、 https://github.com/topics/battery-charge-limit

**家族级门控 / 无界14X 路线**
- https://github.com/losewayy/uniwill-ec-charge-limit
- https://github.com/faintonce/open-revo/issues/38
- https://gist.github.com/w568w/957976b59906e0ce5d6c13ad342e1593
- https://gist.github.com/w568w/b2fc5f9d1f4dff13efe751abec27b396
- https://github.com/LongSang01/wujie14X-Linux-Driver

**驱动/维基/论坛**
- https://wiki.archlinux.org.cn/title/Mechrevo_WUJIE14X （英文：https://wiki.archlinux.org/title/MECHREVO ）
- https://aur.archlinux.org/packages/mechrevo-drivers-dkms
- https://forum.archlinuxcn.org/t/topic/15616
- https://docs.kernel.org/admin-guide/laptops/uniwill-laptop.html
- https://github.com/Wer-Wolf/uniwill-laptop （含 issue #3）
- https://github.com/tuxedocomputers/tuxedo-drivers 、 https://github.com/tuxedocomputers/tuxedo-control-center/issues/268
- https://github.com/sund3RRR/mechrevo14X-linux 、 https://github.com/xuwd1/mechrevo-wujie14-kmod
- https://github.com/faintonce/open-revo （352 star，Windows）

**中文博客/社区**
- https://zhuanlan.zhihu.com/p/730538041 、 https://icyleaf.com/gears/wujie-14x-laptop 、 https://atomopo.me/2026/01/18/mechrevo-linux/ 、 https://zeeko.dev/2025/08/improving-mechanical-revolution-15xpro-charging-speed 、 https://rikka.im/post/mechrevo-wujie15x-linux 、 https://blog.naivetomcat.cn/pages/0214cf 、 https://www.v2ex.com/t/1036612
- https://linux.do/t/topic/1158701 、 https://linux.do/t/topic/1338229 、 https://linux.do/t/topic/1875733 、 https://forum.archlinuxcn.org/t/topic/12083

**未能读取（待人工）**
- https://www.reddit.com/r/linuxhardware/comments/1ng8iec/how_to_enable_80_charge_limit_on_mechrevobrand/
