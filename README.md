# mechrevo-xingyao14-linux-tweaks

机械革命 星耀14（MECHREVO XINGYAO **P916F-STX**，AMD Ryzen AI 9 365 / Strix Point）在 Linux 下的完整调教套件：EC 逆向、FN 键守护、充电阈值、功耗墙、锁屏指纹直触、休眠方案、扬声器校正、NPU 推理。

## 适用范围与安全声明

> ⚠️ **先读再跑**：本仓库所有工具与文档**只在下面这一台具体机器上验证过**。同方公模的不同机型（甚至同型号不同 BIOS/EC 版本）寄存器语义可能完全不同——无界14X 的充电限制写法 `0x07B9` 在本机就不是控制地址，是现成的反例。**在非验证机型上照抄 EC/WMI 写操作可能变砖。** 不确定就只跑只读命令、先读对应文档的证据链，或直接放弃。

- 验证基线：**P916F-STX，BIOS 1.15，CachyOS（内核 7.2），KDE Plasma 6.8 Wayland，btrfs**——这是唯一验证环境
- 其他星耀/P916F 变体（-HPT-R / -ARL 等）未验证——**EC 写入操作在未验证机型上可能变砖**（本机就因休眠固件路径砖过一次，长按电源 30 秒清 EC 才救回）
- 本机休眠**只能用 `HibernateMode=shutdown`，禁用 platform 模式**（会砖），详见 [docs/hibernation.md](docs/hibernation.md)
- 各组件 INSTALL.md 的"前提"一节写明了验证环境；你的机型/固件对不上时，风险自担

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
| WPS 流氓进程治理 | [integrations/wps](integrations/wps/INSTALL.md) | EverythingDaemon 永禁（曾卡死 FUSE 挂载致合盖假死）+ WPS 退出后自动清场常驻进程，[附论坛分享稿](integrations/wps/forum-post.zh.md) |
| KDE 日历中国节假日 | [integrations/kde-calendar](integrations/kde-calendar/INSTALL.md) | 用户级数据覆盖（数据编译在 libKF6Holidays 内）+ 去英文化 + 补班红点，qmldir prefer 坑与 pacman hook 全记录 |
| NPU 语音听写（机身 AI 键） | [integrations/asr](integrations/asr/INSTALL.md) | flm + voxtype：AI 键(KEY_F23)说话→简体进光标；Wayland CJK 输出/模型下载排雷记见 [docs/notes/asr-dictation.md](docs/notes/asr-dictation.md) |
| 微信沙箱双向修复 | [integrations/wechat](integrations/wechat/INSTALL.md) | wechat-universal-bwrap 发文件（binds.list）+ 收图（symlink），沙箱死路径根因与修法 |
| XWayland 全屏黑屏排障 | [docs/notes/xwayland-scanout-black.md](docs/notes/xwayland-scanout-black.md) | AMD 直扫 + 应用自行缩放：截图正常肉眼黑，wmctrl 验证法与根治 |

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
│   ├── kde-calendar/   # KDE 时钟日历：中国节假日数据覆盖 + 补班红点
│   ├── audio/          # EasyEffects 预设与 IRS
│   ├── asr/            # 机身 AI 键语音听写（flm + voxtype 服务与配置）
│   ├── wechat/         # 微信 bwrap 沙箱发文件/收图双向修复
│   └── wps/            # WPS 流氓进程治理（一键脚本 + 论坛分享稿）
└── NOTICE.md        # 厂商衍生素材（IRS/模型/运行库/固件）的来源与再分发说明
```

每个组件目录内都有 `INSTALL.md`（前提 / 安装命令 / 验证 / 卸载，人和 AI 都能直接照做）。

## 快速开始

```bash
git clone https://github.com/dwdcth/mechrevo-xingyao14-linux-tweaks.git
cd mechrevo-xingyao14-linux-tweaks
# 按需逐件安装，从主工具开始：
cd tools/mechrevo-ctl && cat INSTALL.md
```

## 背景：这台机器的逆向地图

- EC：ITE IT5571（识别 0x5571 rev 07），自带双通道 I2C 主机（触控板仲裁问题的根源，见 [docs/touchpad-i2c.md](docs/touchpad-i2c.md)）
- WMI：同方公模通道（`ABBC0F5B/5C`，内核 `huawei_wmi` 原生处理），FN 键事件码 0x20~0xa2 全表在 [docs/hardware-map.md](docs/hardware-map.md)
- 与 TUXEDO/Uniwill 驱动族**不通用**（无 ABBC0F6F/INOU0000 通道，分析见 [docs/notes/repo-analysis.md](docs/notes/repo-analysis.md)）

## 相关项目

- [YMGPwcca/Mechrevo-Xingyao-14](https://github.com/YMGPwcca/Mechrevo-Xingyao-14)——同机型纯技术参考库（寄存器地图/固件考证），与本仓库工具侧互补
- [tuxedo-drivers](https://github.com/tuxedocomputers/tuxedo-drivers) / [uniwill-laptop](https://github.com/wer-wolf/uniwill-laptop)——Uniwill 系参考实现
- [RyzenAdj](https://github.com/flygoat/ryzenadj)——SMU 功耗墙
- [nikhcevov.me 指纹解锁方案](https://nikhcevov.me/posts/kde-plasma-fingerprint-unlock-immediately)——锁屏补丁的出发点
- [EasyEffects](https://github.com/wwmm/easyeffects)——音频承载

## 许可

本仓库自有代码与文档：MIT（见 [LICENSE](LICENSE)）。仓库内嵌的厂商衍生二进制/模型素材见 [NOTICE.md](NOTICE.md)，不适用 MIT。
