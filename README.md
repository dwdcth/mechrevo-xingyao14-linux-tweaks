# mechrevo-xingyao14-linux-tweaks

机械革命 星耀14（MECHREVO XINGYAO **P916F-STX**，AMD Ryzen AI 9 365 / Strix Point）在 Linux 下的完整调教套件：EC 逆向、FN 键守护、充电阈值、功耗墙、锁屏指纹直触、休眠方案、扬声器校正、NPU 推理。

## 适用范围与安全声明

- 验证基线：**P916F-STX，BIOS 1.15，CachyOS（内核 7.2），KDE Plasma 6.8 Wayland，btrfs**
- 其他星耀/P916F 变体（-HPT-R / -ARL 等）未验证——**EC 写入操作在未验证机型上可能变砖**（本机就因休眠固件路径砖过一次，长按电源 30 秒清 EC 才救回）
- 本机休眠**只能用 `HibernateMode=shutdown`，禁用 platform 模式**（会砖），详见 [docs/hibernation.md](docs/hibernation.md)

## 功能矩阵

| 功能 | 组件 | 说明 |
|---|---|---|
| 电池充电阈值 80% | [tools/mechrevo-ctl](tools/mechrevo-ctl/INSTALL.md) | EC PMC2 端口协议（0x68/0x6C）自逆向，无标准 sysfs 节点机型可用 |
| FN 键全功能 OSD | [tools/mechrevo-ctl](tools/mechrevo-ctl/INSTALL.md) + [integrations/systemd](integrations/systemd/INSTALL.md) | 触控板/键盘灯/麦克风/飞行模式/Fn锁/**性能模式 Fn+X**/**刷新率 Fn+R**，事件码全表见 [docs/hardware-map.md](docs/hardware-map.md) |
| 性能模式联动 KDE | 同上 | Fn+X 时同步 powerdevil 电源模式（官方 DBus 通道） |
| SMU 功耗墙直调 | [tools/mr-power](tools/mr-power/INSTALL.md) | ryzenadj 包装：12W 静音档 / 任意瓦数 / 一键恢复 |
| 锁屏指纹直触解锁 | [integrations/kde](integrations/kde/INSTALL.md) | 电源键=指纹二合一机型：锁屏任意时刻轻触即解锁，QML 双补丁 + pacman hook |
| 休眠（不砖机） | [docs/hibernation.md](docs/hibernation.md) | shutdown 模式定稿，含 platform 砖机完整证据链与内核源码分析 |
| 扬声器校正 | [integrations/audio](integrations/audio/INSTALL.md) | 原厂 Nahimic 卷积 IR → EasyEffects，六套场景预设 + Mac 曲线 |
| NPU 模型推理 | [tools/xrun-rs](tools/xrun-rs/INSTALL.md) | Rust 直调 XRT/Vitis-AI ABI 跑 Windows 效果模型（31fps 人像分割），含模型提取逆向全记录 |

## 目录结构

```
├── docs/            # 文档：硬件地图（EC 寄存器/WMI 事件码）、休眠、触控板 I2C、逆向笔记
├── firmware/        # 本机 ACPI 反编译（DSDT + 30 SSDT）、EC 素材
├── tools/
│   ├── mechrevo-ctl/   # Rust 主工具：EC/充电/WMI/OSD 守护（零依赖单二进制）
│   ├── xrun-rs/        # Rust NPU 推理（模型/运行库/固件经 GitHub Release 分发，见其 INSTALL.md）
│   └── mr-power/       # 功耗墙快捷脚本
├── integrations/
│   ├── systemd/        # mechrevo-osd / mechrevo-charge / kde-lock-raise / fprintd-release
│   ├── kde/            # 锁屏指纹 QML 补丁 + pacman hook
│   └── audio/          # EasyEffects 预设与 IRS
└── NOTICE.md        # 厂商衍生素材（IRS/模型/运行库/固件）的来源与再分发说明
```

每个组件目录内都有 `INSTALL.md`（前提 / 安装命令 / 验证 / 卸载，人和 AI 都能直接照做）。

## 快速开始

```bash
git clone https://github.com/<你>/mechrevo-xingyao14-linux-tweaks.git
cd mechrevo-xingyao14-linux-tweaks
# 按需逐件安装，从主工具开始：
cd tools/mechrevo-ctl && cat INSTALL.md
```

## 背景：这台机器的逆向地图

- EC：ITE IT5571（识别 0x5571 rev 07），自带双通道 I2C 主机（触控板仲裁问题的根源，见 [docs/touchpad-i2c.md](docs/touchpad-i2c.md)）
- WMI：同方公模通道（`ABBC0F5B/5C`，内核 `huawei_wmi` 原生处理），FN 键事件码 0x20~0xa2 全表在 [docs/hardware-map.md](docs/hardware-map.md)
- 与 TUXEDO/Uniwill 驱动族**不通用**（无 ABBC0F6F/INOU0000 通道，分析见 [docs/notes/repo-analysis.md](docs/notes/repo-analysis.md)）

## 相关项目

- [Mechrevo-Xingyao-14](https://github.com/)——同机型纯技术参考库（寄存器地图/固件考证），与本仓库工具侧互补
- [tuxedo-drivers](https://github.com/tuxedocomputers/tuxedo-drivers) / [uniwill-laptop](https://github.com/wer-wolf/uniwill-laptop)——Uniwill 系参考实现
- [RyzenAdj](https://github.com/flygoat/ryzenadj)——SMU 功耗墙
- [nikhcevov.me 指纹解锁方案](https://nikhcevov.me/posts/kde-plasma-fingerprint-unlock-immediately)——锁屏补丁的出发点
- [EasyEffects](https://github.com/wwmm/easyeffects)——音频承载

## 许可

本仓库自有代码与文档：MIT（见 [LICENSE](LICENSE)）。仓库内嵌的厂商衍生二进制/模型素材见 [NOTICE.md](NOTICE.md)，不适用 MIT。
