# 剪贴板整体迁移 CopyQ：点选即贴 + Klipper 退役（含"退不干净"实验）

> 环境：CachyOS，KDE Plasma 6.8 Wayland，CopyQ 16.0.0，ydotool 1.0.4。验证日期：2026-10-21。
> 同主题：[herdr-wayland-clipboard.md](herdr-wayland-clipboard.md)（贴图环境变量）、
> [klipper-selection-history.md](klipper-selection-history.md)（历史被拖选刷屏）。

## 目标

Windows Win+V 式体验：`Meta+V` 弹历史 → 点条目 → **自动粘贴**到之前的焦点窗口（文字、图片都要）。KDE 自带的 Klipper 做不到（点条目只换剪贴板内容，Linux 没有通用"插入"事件，详见 [KDE 论坛讨论](https://discuss.kde.org/t/cannot-paste-items-directly-from-clipboard-meta-v-menu-without-also-using-ctrl-v/40638)），换 CopyQ。

## 为什么这么绕：三重障碍

1. **Klipper 无注入**：Plasma 剪贴板弹窗点条目=仅置为当前剪贴板，不合成按键。
2. **CopyQ 官方不支持 Wayland 自动粘贴**（FAQ 明说：仅 Windows/macOS/X11，X11 走
   XTEST）。实测 `copyq paste` 报 `Failed to paste clipboard`。
3. **wtype 被 KWin 拒**：实测报 `Compositor does not support the virtual keyboard
   protocol`——KWin 不对普通客户端开放虚拟键盘协议，注入只剩 uinput 一条路
   （ydotool）。

## 方案架构

```
点 CopyQ 条目 ─→ CopyQ 复制到剪贴板 ─→ 隐藏主窗口(焦点回目标窗口)
                                          │
             wayland-support 脚本覆盖 paste() ─→ sleep(100ms)
                                          │
                                    ydotool key Shift+Insert
                                   (uinput 内核级注入, 走 ydotoold 守护)
```

- 注入键用 **Shift+Insert** 而不是 Ctrl+V：GUI 应用和终端都认它是粘贴，比 Ctrl+V
  通用（Ctrl+V 在终端里是字面量）。这是官方脚本的选择，别改。
- ydotool 走内核 uinput，不依赖合成器协议，KWin 拒不掉。

## 落地清单（全部可复现）

| 组件 | 操作 |
|---|---|
| Klipper 历史上限 | `kwriteconfig6 --file klipperrc --group General --key MaxClipItems 1` |
| Klipper 选区记录 | `... --key SaveSelection false`（见同主题笔记） |
| Klipper 的 Meta+V 解绑 | `kwriteconfig6 --file kglobalshortcutsrc --group plasmashell --key show-on-mouse-pos 'none,none,在鼠标位置显示剪贴板项目'` + 重启 plasmashell |
| Klipper 托盘 applet 移除 | 见下节"退役实验"；等效于 UI 里 系统托盘→条目→剪贴板→已禁用 |
| CopyQ 安装/自启 | `pacman -S copyq ydotool wtype`；desktop 文件拷入 `~/.config/autostart/`（wtype 装了也没用，可省） |
| ydotoold 守护 | `systemctl --user enable --now ydotool.service`（ydotool 包自带用户级单元；用户需在 `input` 组） |
| wayland-support 命令 | 从 [copyq-commands 仓库](https://github.com/hluk/copyq-commands/blob/master/commands/wayland-support.ini)取 ini，合并进 `~/.config/copyq/copyq-commands.ini`（`N\Command=` 多行 + `N\IsScript=true` + `size` 计数 +1），重启 CopyQ。**必须打本机补丁，见下节** |
| CopyQ 的 Meta+V | CopyQ 首选项→常规→"显示/隐藏主窗口"快捷键设 Meta+V（会存成 copyq-commands.ini 里 `IsGlobalShortcut` 命令） |
| 点选即贴 | CopyQ 默认已开（`activate_pastes=true`），无需动 |

## 验证方法（自动化，不用手点）

核心技巧：**zenity --entry 当靶子 + ydotool 当手**，注入结果从 stdout 收回：

```bash
wl-copy "TEST" ; setsid zenity --entry --text=t >/tmp/out 2>/dev/null &
sleep 1.5                                   # 等窗口聚焦
copyq show; sleep 1.2                       # 弹 CopyQ(模拟 Meta+V)
ydotool key 28:1 28:0                       # Enter 激活条目 → 应自动粘贴
sleep 1.5; ydotool key 28:1 28:0            # Enter 关掉 zenity
cat /tmp/out                                # 有 TEST = 全链路通
```

分层定位时用同法测原料：裸 `ydotool key 42:1 110:1 110:0 42:0`（Shift+Insert）
进 zenity 通 = 注入层 OK；`copyq eval 'paste()'` 通 = 脚本覆盖层 OK。

**坑**：CLI 的 `copyq paste` 走原生路径、**绕过脚本覆盖**，永远报错，别拿它当
验收标准；真实用户路径（窗口内 Enter/点条目）走脚本层，是好的。

## 官方脚本在本机的坑：onClipboardChanged 覆盖会弄断入库

**症状**：装上 wayland-support 后"点选即贴"正常，但 CopyQ 不再收录新剪贴板
（`copyq count` 不涨）。禁用该命令立即恢复 → 锁定脚本。

**根因**：脚本对入库回调的覆盖

```js
const onClipboardChanged_ = onClipboardChanged;   // 闭包捕获原始回调
onClipboardChanged = function () {
    ...取窗口标题(kdotool)...
    onClipboardChanged_();                        // ← 这条链路在 CopyQ 16 命令
};                                                //   脚本环境里断了,入库中止
```

其声明用途是给条目记录来源窗口标题（KDE 依赖 AUR 的 kdotool）。实测在本机
（CopyQ 16 / Plasma 6.8 / 无 kdotool）该覆盖使入库完全失效——先给取标题段包
try/catch 无效，说明不止是 kdotool 缺失的异常问题，闭包引用本身就不可靠。

**补丁（本机已打）**：把整个 onClipboardChanged 覆盖块删掉，其余功能
（paste/copy/focusPrevious/截图）不受影响。放弃的唯一功能 = 条目窗口标题。

**打补丁的坑**：CopyQ 保存时会把 `copyq-commands.ini` 里的多行 Command 重写为
`\n` 转义的**单行**——补丁脚本必须按转义形式匹配（`\n` 是两个字符），按原始
多行文本 sed/python 匹配会静默失败。**且每次 CopyQ 退出都会重新序列化**
（`};` 变 `}`、引号风格变），打补丁前先 `copyq exit` 再 `grep` 当前字节形态，
凭记忆写 pattern 必挂。

**验收教训**：当时只验了"点选即贴"（读路径），没验"新条目入库"（写路径），
带病上线半天才暴露。两个方向都要测：

```bash
c0=$(copyq count); wl-copy TEST; sleep 1.2; c1=$(copyq count)   # 写:count 应 +1
# + 上节的 zenity 全链路（读）
```

## 第二个本机补丁：按内容选粘贴键（图片一步进 Claude Code）

原始设计只发 Shift+Insert——GUI 应用没问题，但**终端里的 Claude Code** 收到
Shift+Insert 只能文本粘贴，图片进不去（终端的物理限制）；而 Claude Code 自带
的 Ctrl+V 会自己调 wl-paste 读图。补丁让 `global.paste` 看内容下菜：

```js
global.paste = function () {
    var isImg = false;
    try { isImg = str(read('image/png', 0)).length > 0; } catch (e) {}
    if (isImg) {
        sendShortcut('29:1', '47:1', '47:0', '29:0');    // Ctrl+V：Claude Code 贴图
    } else {
        sendShortcut('42:1', '110:1', '110:0', '42:0');  // Shift+Insert：终端文本安全
    }
};
```

图片→Ctrl+V 对 GUI 应用同样成立（Word/浏览器照贴），两臂各自最优。
代价：**纯 shell 终端**里点选图片条目会注入 Ctrl+V → 命令行出现字面 `^V`，
退格删掉即可（罕见场景）。

**验证方法论**（这轮踩出来的）：

- **探针法最可靠**：临时在 `sendShortcut` 里加一行
  `run('sh','-c','echo k=... >> /tmp/_pastekeys')` 记录键码，激活条目后看
  日志即知选了哪臂，**不依赖任何焦点行为**
- GUI 靶子焦点不可靠：zenity 能抢到 KWin 焦点，konsole/alacritty 的新窗口
  抢不到（焦点防抢）——拿它们当注入靶子会静默失败、误判补丁
- `cat -v > file` 是块缓冲，两个字符的 `^V` 会在 pkill 时丢在缓冲里，
  要用 `stdbuf -o0`

## Klipper 退役实验：引擎退不掉

问题：能不能把 Klipper 彻底关掉？实验（2026-10-21，Plasma 6.8）：

1. 从 `plasma-org.kde.plasma.desktop-appletsrc` 的 `extraItems` 移除
   `org.kde.plasma.clipboard` 并删除对应 Applet 块（等效 UI"已禁用"），重启
   plasmashell；
2. 结果 A：**`org.kde.klipper` DBus 服务仍在**（plasmashell 注册，对象树完好）；
3. 结果 B：停掉 CopyQ 后杀 `wl-copy`（模拟源应用退出），**剪贴板内容仍存活**
   ——持久化引擎照常工作。

结论：**Klipper 引擎编在 plasmashell 内，托盘 applet 只是 UI**；配置层面无法
卸载引擎，只能让它惰性化。彻底移除需改 plasma-workspace 源码重编译，每次更新
都要重打——不值。

惰性化后的 Klipper 剩余行为：历史 1 条、不记选区、无快捷键、无 UI；唯一活跃
功能是剪贴板持久化——与 CopyQ 互为双保险（任一活着，应用退出后内容就不丢），
**无害，留着**。

## 已知边界

- 终端里 Shift+Insert 是粘贴 ✓（这正是选它不选 Ctrl+V 的原因）
- 极端个别的应用若不认 Shift+Insert，把 wayland-support 脚本里 `global.paste`
  的键码 `42:1 110:1` 换成 `29:1 47:1`（Ctrl+V），但终端会失效
- 条目窗口标题功能已随补丁放弃（kdotool 也因此**不必装**——装了反而要把补丁
  回退才用得上）
- `copyq paste` CLI 命令不可用（见上）

## 回滚

```bash
cp ~/.config/plasma-org.kde.plasma.desktop-appletsrc.bak-klipper ~/.config/plasma-org.kde.plasma.desktop-appletsrc
kwriteconfig6 --file klipperrc --group General --key MaxClipItems 2000
kwriteconfig6 --file kglobalshortcutsrc --group plasmashell --key show-on-mouse-pos 'Meta+V,Meta+V,在鼠标位置显示剪贴板项目'
systemctl --user restart plasma-plasmashell.service
# CopyQ: 删 autostart 项 + copyq exit;ydotool: systemctl --user disable --now ydotool
```
