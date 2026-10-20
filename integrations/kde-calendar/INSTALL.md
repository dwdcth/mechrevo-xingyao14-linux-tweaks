# KDE 时钟日历：中国节假日数据 + 补班红点

KDE 数字时钟的日历弹窗（Plasma 6.8 验证）显示中国法定节假日的三个痛点与对策：

1. **数据过期/错误**：Arch 的 kholidays 包把节假日数据**编译进 `libKF6Holidays.so` 的 Qt 资源**（硬盘上搜不到 `.holiday` 文件），包冻结时的快照可能落后于上游——2026 年 10 月本机显示的仍是"中秋国庆连休"旧预测数据（多出 9/13、9/28–30 等幽灵事件）
2. **补班日无法区分**：调休补班和假期在日历上都是同色蓝点
3. **事件名带英文**："中秋(Mid-autumn Festival)"

## 原理（三个关键事实，均有源码依据）

- **KHolidays 磁盘优先**：`HolidayRegion` 构造时先查 `~/.local/share/kf5/libkholidays/plan2/holiday_<区域码>`（注意 `kf5` 目录名是 KF6 沿用的），找不到才用内嵌资源 → 放用户级文件即可覆盖，无 root、不怕包升级
- **qmldir 的 `prefer` 行是坑**：`org.kde.plasma.workspace.calendar` 模块的 qmldir 写着 `prefer :/qt/qml/...`——QML 优先从 .so 内嵌的预编译资源加载，**只改磁盘 `DayDelegate.qml` 无效**，必须删掉这行
- **补班标记走 `subDayLabel`**：事件标题进不了 QML（`CalendarEvents::EventData` 无 Q_PROPERTY，DaysModel 子行只暴露 `eventColor`），但 holidays 插件会把假期名写进天级 `subDayLabel`（单日条目每天有）。数据文件把补班条目统一命名为"补班"，QML 判断 `subDayLabel` 含"补班"即染红

数据源：上游 [kholidays `holiday_cn_zh-cn`](https://invent.kde.org/frameworks/kholidays/-/blob/master/holidays/plan2/holiday_cn_zh-cn)（实际维护：[kholidays_cn](https://github.com/0xcccccccccccc/kholidays_cn)，国务院每年 11 月发布次年安排后更新）。

## 文件

| 仓库文件 | 去向 | 用途 |
|---|---|---|
| `kde-calendar-workday-dot` | `/usr/local/bin/` | 幂等补丁脚本：删 qmldir prefer 行 + DayDelegate 事件点染红 |
| `kde-calendar-workday-dot.hook` | `/etc/pacman.d/hooks/` | plasma-workspace 升级后自动重打 |
| `update-cn-holiday-data.sh` | 随意（建议 `~/.local/bin/`） | 下载最新数据 + 去英文化 + 重启 plasmashell（每年 11 月跑一次） |

数据文件不入库（上游持续更新，避免快照过期），由安装/更新命令现场下载。

## 前提

- KDE Plasma 6.8（`plasma-workspace` 提供 `org.kde.plasma.workspace.calendar`；上游 QML 结构变化时补丁脚本会**报错退出而非静默失效**，届时需更新匹配串）
- `kholidays`、`curl`、`python`（脚本依赖）
- 时钟设置已启用"假日"日历插件并选择中国区域（配置键 `selectedRegions=cn_zh-cn`，位于 `~/.config/plasma_calendar_holiday_regions`）

## 安装

```bash
# 1. 补丁脚本 + pacman hook（plasma-workspace 升级后自动重打）
sudo install -Dm755 kde-calendar-workday-dot /usr/local/bin/kde-calendar-workday-dot
sudo install -Dm644 kde-calendar-workday-dot.hook /etc/pacman.d/hooks/kde-calendar-workday-dot.hook
sudo /usr/local/bin/kde-calendar-workday-dot

# 2. 节假日数据（下载最新 + 去英文化 + 重启日历）
install -Dm755 update-cn-holiday-data.sh ~/.local/bin/update-cn-holiday-data
~/.local/bin/update-cn-holiday-data
```

## 验证

打开任务栏时钟的日历，切换到最近的法定假期月份：

- 节日名只有中文（"中秋"、"国庆节"），日期下方出现；补班日显示"补班"
- 补班日圆点为**红色**（`negativeTextColor`），假期圆点保持主题色
- 无幽灵事件（对照官方安排：如 2026 年中秋仅 9/25–27 三天）

命令行核对数据（可选）：

```bash
grep -c 补班 ~/.local/share/kf5/libkholidays/plan2/holiday_cn_zh-cn   # >0
grep '^prefer ' /usr/lib/qt6/qml/org/kde/plasma/workspace/calendar/qmldir  # 应无输出
```

## 更新数据（每年 11 月国务院发布次年安排后）

```bash
~/.local/bin/update-cn-holiday-data
```

## 卸载

```bash
sudo rm /etc/pacman.d/hooks/kde-calendar-workday-dot.hook /usr/local/bin/kde-calendar-workday-dot
sudo pacman -S plasma-workspace        # 重装还原 qmldir 与 DayDelegate.qml（备份 .orig-* 一并消失）
rm -rf ~/.local/share/kf5/libkholidays/plan2 ~/.local/bin/update-cn-holiday-data
systemctl --user restart plasma-plasmashell
```

## 边界与已知问题

- plasma-workspace 大版本升级后若 QML 结构变化，hook 会报 `pattern not found` 并以非零码退出（pacman 会显示但**不影响升级本身**），需人工更新脚本里的匹配串
- 数据覆盖对全系统 KHolidays 生效（不只日历弹窗）；上游数据文件本身有少量重复条目（如 10/1 同时命中"国庆节"固定规则与 2026 年度条目 → 双点），属上游数据形态
- 网络不通时给 `update-cn-holiday-data.sh` 里的 curl 挂代理（`-x http://127.0.0.1:7897`）
- 备份说明：qmldir 备份 `.orig-prefer`、DayDelegate 备份 `.orig-workday-dot`、数据备份 `.orig-english`（均在原目录旁）
