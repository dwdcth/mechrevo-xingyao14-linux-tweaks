# 电池图标闪电防抖 v2（限充阈值点状态抖动）

## 问题与 v1 仍抖动的两个根因

开启 80% 充电阈值（`mechrevo-ctl charge`）后，电池停在阈值点时，托盘电池图标的闪电随机闪烁。完整因果链（经反汇编 `libbatterycontrol.so.6.8.2` 与 `libworkspacecomponents.so` 核实）：

```
EC 状态位在阈值点横跳 Charging / Discharging / Not charging（且会连续数分钟停在单态）
  → /sys/class/power_supply/LCBT/status 跟着跳（而适配器 ACAD/online 恒为 1）
  → UPower 设备 State 轮询采样随机命中三态之一（UPower 全局 OnBattery 恒为 false）
  → KDE BatteryControlModel::updateOverallBattery() 用 !anyDischarging 覆盖 pluggedIn
      · 采到 Charging / Not charging → pluggedIn = true（画闪电）
      · 采到 Discharging             → pluggedIn = false（去闪电）
  → BatteryIcon.qml: if (pluggedIn) name += "-charging"
```

此前 v1（仅替换磁盘 `BatteryIcon.qml` + 120 秒 `Timer` + `pacman.conf` `NoUpgrade`）仍然抖动，有两个独立原因：

1. **`qmldir` 的 `prefer` 行导致磁盘 QML 完全未生效（主因）**
   `/usr/lib/qt6/qml/org/kde/plasma/workspace/components/qmldir` 含：
   ```
   prefer :/qt/qml/org/kde/plasma/workspace/components/
   ```
   Qt 6 的 `qt_add_qml_module` 将原版 `BatteryIcon.qml` 与 AOT 预编译代码一并打进了同目录的 `libworkspacecomponents.so`；只要 `prefer` 行存在，QML 引擎永远加载 `.so` 内嵌资源，**磁盘上的 `BatteryIcon.qml` 根本不会被读取**（同仓库 [`integrations/kde-calendar`](../../integrations/kde-calendar/INSTALL.md) 踩过同款坑）。
2. **120 秒定时器本身既压不住长驻单态、又拖慢真拔插**
   - 本机 EC 在阈值点并非均匀高频白噪声，`Discharging` 占约 62.5%（连续 4 次 30s 采样达 120s 的概率 `0.625^4 ≈ 15.3%`），且实测会连续数分钟停在 `Not charging` 或 `Discharging`，超过窗口后闪电照样来回翻；
   - 初版在启动恰好撞上 `pluggedIn == false` 时 `plugSynced` 未置位，首次跳变直接穿透防抖；
   - 真正插拔电源适配器时图标要傻等 2 分钟才变。

## v2 方案：去掉 `prefer` + 直绑 `UPower.OnBattery` + Pacman Hook

1. **解锁磁盘 QML**：移除 `qmldir` 中的 `prefer :/qt/qml/...` 行，让 `plasmashell` 与锁屏进程真正加载磁盘上的 `BatteryIcon.qml`（同目录 `BadgeOverlay.qml`、`KeyboardLayoutSwitcher.qml` 本就随包安装在磁盘上，不受影响）。
2. **零延迟根治**：直接用 `plasma-workspace` 自带的 `org.kde.plasma.workspace.dbus` 模块订阅 SystemBus 上 `org.freedesktop.UPower`（`/org/freedesktop/UPower`）的全局 `OnBattery` 属性，并配 `DBusServiceWatcher` 处理 `upower` 服务重启：
   - 主电池（`batteryType` 为 `""` 或 `"Battery"`，覆盖托盘总图标、锁屏图标、弹窗内置电池行）改看 `!OnBattery`；
   - **停在 80% 阈值点时**：`ACAD/online=1` → `OnBattery` 恒为 `false`，闪电**永久稳定常亮、零轮询、零闪烁**；
   - **真正插拔电源适配器时**：DBus `PropertiesChanged` 信号推送，闪电**毫秒级即时切换**（不再等 2 分钟）；
   - **蓝牙鼠标/耳机等外设电池**（`batteryType` 非主电池）：保持原 `pluggedIn` 逻辑不变。
3. **升级自维护**：对齐仓库 `integrations/kde*` 惯例，改用幂等补丁脚本 + `pacman` `PostTransaction` hook（取代 `NoUpgrade` 冻结旧文件），`plasma-workspace` 升级后自动重打；若未来大版本上游 QML 结构变化，脚本报错提示而非静默留旧文件。

## 文件

| 仓库文件 | 去向 | 用途 |
|---|---|---|
| `kde-battery-icon-debounce` | `/usr/local/bin/` | 幂等补丁脚本：去 `qmldir` `prefer` 行 + 打 `BatteryIcon.qml` v2 补丁 |
| `kde-battery-icon-debounce.hook` | `/etc/pacman.d/hooks/` | `plasma-workspace` 安装/升级后自动重打 |
| `BatteryIcon.qml` | 仓库留档对照 | Plasma 6.8.2 打完补丁后的完整参考副本 |

## 前提

- KDE Plasma 6.8.x（`plasma-workspace` 包提供 `org.kde.plasma.workspace.components` 与 `org.kde.plasma.workspace.dbus`）
- `upower`、`python`（脚本运行依赖）

## 安装

```bash
sudo install -Dm755 kde-battery-icon-debounce /usr/local/bin/kde-battery-icon-debounce
sudo install -Dm644 kde-battery-icon-debounce.hook /etc/pacman.d/hooks/kde-battery-icon-debounce.hook
sudo /usr/local/bin/kde-battery-icon-debounce
```

（若之前装过 v1，顺手删掉 `/etc/pacman.conf` 里的 `NoUpgrade = usr/lib/qt6/qml/org/kde/plasma/workspace/components/BatteryIcon.qml` 行。）

清理 QML 缓存并重启 `plasmashell`：

```bash
rm -rf ~/.cache/plasma* ~/.cache/qmlcache*
systemctl --user restart plasma-plasmashell.service
```

## 验证

```bash
# 1. qmldir 已无 prefer 行（无输出即正确）
grep '^prefer ' /usr/lib/qt6/qml/org/kde/plasma/workspace/components/qmldir

# 2. v2 标记已写入且无 QML 报错
grep -c 'kde-battery-icon-debounce-v2' /usr/lib/qt6/qml/org/kde/plasma/workspace/components/BatteryIcon.qml
journalctl --user -u plasma-plasmashell --since -2min | grep -iE 'qml|batteryicon|error'
```

实测表现：
- 插电停在 79%–80% 阈值点时，闪电稳定常亮不再闪烁；
- 拔掉/插上电源适配器，托盘闪电**立即**消失/出现。

> 💡 **附注（整颗电池图标偶发收进托盘折叠区 `▲`）**：
> 在默认的「平衡（balanced）」电源档且未手动阻止休眠时，上游 `org.kde.plasma.battery` 的 `main.qml` 会在 `state == NoCharge`（`Not charging`）时把 `Plasmoid.status` 设为 `PassiveStatus`，导致整颗电池图标偶发躲进托盘隐藏抽屉 `▲`。若希望电池图标始终固定在任务栏上，右键系统托盘 `▲` → **配置系统托盘 → 条目 → 电源和电池** → 设为 **「总是显示」** 即可。

## 卸载 / 恢复

```bash
sudo rm -f /etc/pacman.d/hooks/kde-battery-icon-debounce.hook /usr/local/bin/kde-battery-icon-debounce
sudo pacman -S plasma-workspace   # 重装还原原厂 qmldir 与 BatteryIcon.qml
rm -rf ~/.cache/plasma* ~/.cache/qmlcache*
systemctl --user restart plasma-plasmashell.service
```
