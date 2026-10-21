# KDE 剪贴板：拖选把历史刷成"逐字变短"的一串碎片

> 环境：KDE Plasma 6.8 Wayland（CachyOS）。验证日期：2026-10-21。
> 同主题：herdr/SSH 会话贴图失败见 [herdr-wayland-clipboard.md](herdr-wayland-clipboard.md)。

## 症状

鼠标拖选一段文本（或复制一个词）后打开剪贴板历史，发现同一句话出现 N 条，每条
比上一条**左边少一个字符**：

```
herdr server stop
erdr server stop
rdr server stop
…
server stop
erver stop
…
```

看起来像"单词被拆成字母拼接"，其实不是——那是**整个拖选动作的每一帧中间态**。
历史新的在上，从上往下读正好是把选区从行首逐字"收回去"的倒放轨迹。在 TUI
（herdr / 终端）里拖选尤其明显，每个字符位置的变化都触发一次选区更新。

## 根因

`~/.config/klipperrc`：

```ini
[General]
SaveSelection=true      ← 把 PRIMARY 选区也存进历史；拖选过程中每个中间值各记一条
```

## 修复

```bash
kwriteconfig6 --file klipperrc --group General --key SaveSelection false
```

或 UI：剪贴板弹窗 → ⚙ 配置剪贴板 → 取消「**将选区内容保存到历史记录**」
（UI 里点即时生效；直接改文件要等组件重读配置）。

关掉后：历史只记录显式复制（Ctrl+C）；**中键粘贴选区不受影响**（PRIMARY 与
Klipper 历史是两回事），只是旧选区不再出现在历史列表里。

## 相关开关速查

| klipperrc 键 | 含义 | 本机取值 | 备注 |
|---|---|---|---|
| `SaveSelection` | 选区存入历史 | `false`（本次改） | 拖选刷屏的元凶 |
| `SyncClipboards` | 剪贴板 ⇄ 选区互相同步 | `false` | 开了会出现"复制的内容被选区顶掉"，保持关 |
| `MaxClipItems` | 历史条数上限 | `2000` | 关掉选区记录后无所谓，想清爽可改几十 |
| `SaveImages` | 历史存图片 | `true` | 贴图工作流（见同主题笔记）依赖它，保持开 |
