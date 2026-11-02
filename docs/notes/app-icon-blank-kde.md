# 手动安装的应用在 KDE 任务栏 / Alt+Tab 图标空白：KSycoca 进程级缓存

> 结论先行：图标文件、`.desktop` 语法、窗口 WM_CLASS 三样都对，图标仍然空白，根因几乎都在 **KSycoca（KDE 桌面服务数据库）的进程级缓存**。`kbuildsycoca6` 只重建磁盘上的数据库文件；**每个进程持有自己的 mmap 缓存**，不重启对应进程就拿不到新数据。任务栏图标走 plasmashell → 重启 plasmashell；Alt+Tab 切换器图标走 KWin → 重启 kwin_wayland。两个都要重启，只做一个就会出现"任务栏好了、Alt+Tab 还空"或反之的半修复状态。

2026-11 在本机（CachyOS，KDE Plasma 6 Wayland）给一个手动安装到 `~/.local/share` 的应用补图标时撞上：任务栏和 Alt+Tab 两处空白，逐步排掉三层假线索后定位到缓存层，记录于此。

## 症状

- 应用（Wayland 原生窗口）在 Plasma 任务栏、Alt+Tab 切换器里图标空白
- 应用安装程序**已经**把图标装到了 `~/.local/share/icons/hicolor/scalable/apps/<app>.svg`，`.desktop` 里 `Icon=<app>` 指向正确
- 重启应用、重开窗口无效

## 排查链（三层假线索，逐层排除）

### 1. 图标文件与 desktop 条目本身

```bash
grep ^Icon ~/.local/share/applications/<app>.desktop     # 确认 Icon= 指向的名字
desktop-file-validate ~/.local/share/applications/<app>.desktop
find ~/.local/share/icons /usr/share/icons -iname "<app>*"   # 图标文件在不在
```

只有 SVG 也算合法（KDE 支持 scalable）。三项都正常，就别在文件层继续挖了。

### 2. 窗口 WM_CLASS 是否匹配 StartupWMClass

任务栏/切换器靠 `StartupWMClass` ↔ 窗口类匹配。注意 `wmctrl -lx` 是 X11 工具，**看不到 Wayland 原生窗口**——查 Wayland 窗口类要用 KWin 脚本：

```bash
cat > /tmp/list-wmclass.kwin <<'EOF'
workspace.windowList().forEach(function(c) {
    print("KWINWM|" + c.resourceClass + "|" + c.resourceName + "|" + String(c.caption).substring(0, 40));
});
EOF
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript /tmp/list-wmclass.kwin wm-list
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start
sleep 1
journalctl --user _COMM=kwin_wayland --no-pager -n 40 | grep KWINWM
```

输出里 `KWINWM|<窗口类>|...` 与 `.desktop` 的 `StartupWMClass` 一致即匹配。Plasma 6 的 KWin 脚本 API 是 `workspace.windowList()`（旧教程里的 `clientList()` 已删除，用了报 "is not a function"）。

### 3. 到这里都没问题 → 根因就是 KSycoca 进程缓存

KDE 匹配"窗口类 → `.desktop` → 图标"全走 KSycoca。新装的 desktop 文件/图标需要重建索引，而**重建 ≠ 进程拿到**：

| 进程 | 缓存来源 | 是否吃到 `kbuildsycoca6` 结果 |
|---|---|---|
| plasmashell（任务栏） | KSycoca | 只在重启后才吃到 |
| kwin_wayland（Alt+Tab） | KSycoca | 只在重启后才吃到 |

两个进程从开机跑到现在，手里都是旧的（没有新应用条目的）数据库。任何"关开窗口 / 重开应用"的操作都无效，因为做匹配查询的是这两个常驻进程，不是应用本身。

## 修复

```bash
# 1.（可选加固）只有 SVG 时补全套位图：KWin 切换器按具体像素尺寸请求图标，
#    位图路径兼容性最好。rsvg-convert 来自 librsvg 包
SRC=~/.local/share/icons/hicolor/scalable/apps/<app>.svg
for s in 16 24 32 48 64 128 256; do
  d=~/.local/share/icons/hicolor/${s}x${s}/apps
  mkdir -p "$d"
  rsvg-convert -w $s -h $s "$SRC" -o "$d/<app>.png"
done

# 2. 重建桌面数据库
kbuildsycoca6 --noincremental

# 3. 重启两个缓存持有者（都会闪一下重载，所有窗口保留，应用不退出）
systemctl --user restart plasma-plasmashell.service
systemctl --user restart plasma-kwin_wayland.service
```

重启后**应用窗口不用动**：匹配由 KWin/plasmashell 在下次请求时用新缓存完成，已存在的窗口即刻获得图标。

`gtk-update-icon-cache` 在用户级 hicolor 目录（无 `index.theme`）会报 "No theme index file"，属正常——KDE 的 KIconLoader 找不到缓存文件时会直接扫目录，无需处理。

## 验证

- 任务栏：应用图标出现
- Alt+Tab：切换器里图标出现
- 都不出现才回头查图标主题 fallback（基本轮不到）

## 别浪费时间

- **只重启 plasmashell 不重启 kwin**（或反之）：正好复现"任务栏好了、Alt+Tab 还空"的半修复状态，不是新问题
- 关开应用窗口、重装应用、换图标格式：都没用，查询方是常驻进程
- 用 `wmctrl -lx` 找 Wayland 窗口类：列表里根本没有它，别以为窗口不存在
- 用 KWin 脚本里 `c.icon.width` 探测图标状态：属性存在但读不出有效值（`undefined`），拿不到决定性证据，直接肉眼看切换器更快
- 旧教程的 `workspace.clientList()`：Plasma 6 已删，换 `windowList()`
