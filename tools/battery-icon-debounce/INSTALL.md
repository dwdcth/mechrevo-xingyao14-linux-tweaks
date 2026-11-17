# 电池图标闪电防抖(限充阈值点状态抖动)

## 问题

开启 80% 充电阈值(mechrevo-ctl charge)后,电池停在阈值点时,托盘电池图标的闪电
**以 ~30 秒粒度随机闪烁**,时有时无。

因果链(逐层实测,2026-11-17):

```
EC 状态位在阈值点每几秒横跳(40s 实测 Charging/Discharging/Not charging = 13/25/2 次)
  → 内核 /sys/class/power_supply/LCBT/status 跟着跳
  → UPower 设备 State 30s 轮询采样,随机取到三态之一
  → Plasma pluggedIn 属性抖动
  → BatteryIcon.qml: if (pluggedIn) name += "-charging"   ← 闪电,零防抖
```

电量读数全程纹丝不动(实际电流≈0.1W 噪声),纯状态位噪声;UPower 全局 OnBattery
跟 AC 适配器走恒为 false,powerdevil 电源档不受影响,抽风的只有图标。

## 方案

给 `BatteryIcon.qml`(运行时解释执行的散装 QML,非编译产物)的 `pluggedIn` 加
**120 秒持续防抖**:新状态须持续 2 分钟才采纳;首个状态立即采纳不拖会话启动。
停在阈值点时图标彻底稳定;真充电时闪电延迟 2 分钟出现。

对比其它路线:EC 固件闭源不可修;UPower 无配置;内核 acpi_battery 防抖补丁要重编
内核包、每次更新重打——改 QML 是运行时文件,维护成本≈0。

## 前提

- KDE Plasma 6.8.x(补丁基于 6.8.2 的 BatteryIcon.qml,上游文件见
  plasma-workspace 包 `components/batterycontrol/` 与
  `qt6/qml/org/kde/plasma/workspace/components/`)
- 大版本升级 Plasma 前先看文末"升级注意"

## 安装

```bash
TARGET=/usr/lib/qt6/qml/org/kde/plasma/workspace/components/BatteryIcon.qml
sudo cp "$TARGET" "$TARGET.orig-6.8.2"   # 留原版备份
sudo install -Dm644 BatteryIcon.qml "$TARGET"
```

防止 pacman 升级 plasma-workspace 时覆盖(在 `/etc/pacman.conf` 的 `[options]` 里加,
注意**无前导斜杠**):

```
NoUpgrade = usr/lib/qt6/qml/org/kde/plasma/workspace/components/BatteryIcon.qml
```

清 QML 缓存并重启 plasmashell(面板会闪一下):

```bash
rm -rf ~/.cache/plasma ~/.cache/qmlcache
systemctl --user restart plasma-plasmashell.service
# 无 systemd 用户服务时: kquitapp6 plasmashell && sleep 2 && kstart plasmashell
```

验证:重启后 `journalctl --user -u plasma-plasmashell --since -2min | grep -i qml`
无报错;拔/插适配器,闪电应在持续 2 分钟后切换而非立即闪变。

## 恢复

```bash
TARGET=/usr/lib/qt6/qml/org/kde/plasma/workspace/components/BatteryIcon.qml
sudo cp "$TARGET.orig-6.8.2" "$TARGET"   # 或从 pacman 缓存解包:
# bsdtar -xOf /var/cache/pacman/pkg/plasma-workspace-6.8.2-*.pkg.tar.zst "$TARGET#" | sudo tee "$TARGET" >/dev/null
rm -rf ~/.cache/plasma ~/.cache/qmlcache
systemctl --user restart plasma-plasmashell.service
```

并删除 pacman.conf 里的 NoUpgrade 行。

## 升级注意

NoUpgrade 会在升级时**保留本补丁文件**。Plasma 大版本更新若改动了
BatteryIcon.qml 的接口(percent/pluggedIn 属性),旧文件可能不兼容——电池图标
异常时先按上面"恢复"回原版,再用新版本源文件重新套用补丁块(补丁块在本文件
里以 `==== 星耀14 局部补丁` 注释包裹,可整体迁移)。
