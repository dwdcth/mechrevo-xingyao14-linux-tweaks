# CopyQ 自启弹主窗口：Exec 尾巴上的 show 命令压过 hide_main_window

> 环境：CachyOS，KDE Plasma 6.8 Wayland，CopyQ 16.0.0。验证日期：2026-11-09。
> 同主题：[clipboard-copyq-takeover.md](clipboard-copyq-takeover.md)（迁移与点选即贴）、
> [klipper-selection-history.md](klipper-selection-history.md)（历史被拖选刷屏）。

## 现象

每次登录 CopyQ 主窗口必弹出。期望：自启只驻留托盘，要窗口时点托盘或 `copyq show`。

## 根因：自启条目不是"启动"，是"启动 + 弹窗"

`~/.config/autostart/com.github.hluk.copyq.desktop` 与
`/usr/share/applications/com.github.hluk.copyq.desktop` **md5 相同**——自启条目
就是上游菜单 desktop 的整份拷贝（无论 CopyQ 首选项的"自动启动"开关写入，还是
手动拷贝，落地的都是这份文件）：

```ini
Exec=copyq --start-server show
```

对照 `copyq --help` 的语义分解：

| 片段 | 语义 |
|---|---|
| 裸 `copyq` | "Starts server if no command is specified"——只起服务器 |
| `--start-server` | 确保服务器先在后台起来再执行后续命令 |
| `show` | **显式**"Show main window"，弹主窗口 |

也就是说自启时多执行了一个弹窗命令。而首选项里的 Hide main window
（= `copyq.conf` 的 `hide_main_window`）只决定**默认启动弹不弹**，压不过显式
`show`——只勾它治不了这个症状。

上游文件身兼两职：既是菜单启动器（要 show），又带着 autostart 键
（`X-GNOME-Autostart-Delay=3`、`X-KDE-autostart-after=panel`，注释写明是 GNOME
托盘图标的 issue #1526 workaround），被原样拷去做自启，`show` 就这么带进来了。

## 修法（双保险）

```bash
# 1. 根因：自启条目去掉 show，只起服务器
sed -i 's/^Exec=copyq --start-server show$/Exec=copyq --start-server/' \
  ~/.config/autostart/com.github.hluk.copyq.desktop

# 2. 兜底：默认启动也不弹窗（= 首选项 → General → Hide main window）
copyq config hide_main_window true
```

## 验证（实测通过）

```bash
copyq exit; sleep 1
setsid nohup copyq --start-server >/dev/null 2>&1 </dev/null &   # 模拟下次登录的 Exec
sleep 3
pgrep -ax copyq        # 主进程 + --clipboard-access monitorClipboard 子进程都在
copyq config hide_main_window   # → true
```

之后要窗口：托盘图标、`copyq show` 或全局快捷键；`copyq toggle` 反向。

## 注意

- **修改可能被覆盖回去**：在 CopyQ 首选项里重新开关"自动启动"会重新拷贝系统
  源文件，`show` 回魂。届时重删一次即可；`hide_main_window=true` 不受影响仍
  兜底。再遇自启弹窗，先 `grep ^Exec ~/.config/autostart/*copyq*.desktop` 查这条。
- 包管理器升级 copyq 只换 `/usr/share/applications` 源文件，`~/.config/autostart`
  下已拷出的独立副本不会被 pacnew/pacsave 动到。
- `copyq exit` 重启期间当前剪贴板内容可能被清一次；历史条目在磁盘，不丢。
