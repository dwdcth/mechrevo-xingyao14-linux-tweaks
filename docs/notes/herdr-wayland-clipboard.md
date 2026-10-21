# herdr / SSH 会话里 Claude Code 无法粘贴图片（KDE Wayland 剪贴板）

> 环境：CachyOS，KDE Plasma 6.8 Wayland，herdr 0.9.5（mise 安装），Claude Code 2.1.300。
> 验证日期：2026-10-21。

## 症状

在 KDE 里截图（Spectacle → 复制到剪贴板）或网页里"复制图像"后，到 herdr 里跑的
Claude Code 按 `Ctrl+V` 想贴图，没有任何反应；文字剪贴板正常。本地终端里跑的
Claude Code 一切正常。

## 根因

**herdr server 是从 SSH 会话启动的，pane 里的 shell 继承不到 `WAYLAND_DISPLAY` /
`DISPLAY`，Claude Code 因此连不上任何剪贴板。**

证据链：

1. herdr 的架构是 client/server：agent（Claude Code）跑在 server 持有的 PTY 里，
   环境从 server 进程继承。server 可以从任何地方启动并常驻（这正是它的卖点）。

2. 本机实测，server 进程环境：

   ```
   $ tr '\0' '\n' < /proc/$(pgrep -xf 'herdr server' | head -1)/environ | grep -E 'DISPLAY|SESSION'
   SSH_CONNECTION=10.126.126.4 50466 10.126.126.6 22   ← 从 SSH 启动
   XDG_SESSION_TYPE=tty                                  ← 不是图形会话
   XDG_RUNTIME_DIR=/run/user/1000
   （WAYLAND_DISPLAY、DISPLAY 均缺失）
   ```

   而 pane 里的 bash（server 的直接子进程）同样没有这两个变量——herdr 没有
   config 级的 pane 环境注入项，环境就是继承链。

3. Claude Code（2.1.300）在 Linux 上读剪贴板图片的逻辑（二进制 strings 证据）：
   有 `WAYLAND_DISPLAY` → `wl-paste --type image/png`；否则退到
   `xclip -selection clipboard -t image/png`。两个变量都没有时两条路全断，
   贴图静默失败（文字是从终端 bracketed paste 来的，所以看起来"剪贴板正常"）。

4. 对照实验（同一用户）：

   ```
   $ wl-copy --type image/png < test.png
   $ env -i XDG_RUNTIME_DIR=/run/user/1000 wl-paste --list-types
   Failed to connect to a Wayland server ... WAYLAND_DISPLAY is unset
   $ env -i XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 wl-paste --list-types
   image/png                                              ← 只要变量在就通
   ```

   Wayland 剪贴板按 socket 权限访问（同 UID 即可），不需要在图形会话里启动——
   缺的只是那个环境变量。

## 修复

在 `~/.bashrc` 末尾追加（herdr pane 的 shell 是交互式非登录 bash，会读它）：

```bash
# >>> herdr/SSH: 找回图形会话剪贴板（KDE Wayland）>>>
if [ -z "${WAYLAND_DISPLAY:-}" ] && [ -n "${XDG_RUNTIME_DIR:-}" ]; then
  _wl=$(systemctl --user show-environment 2>/dev/null | sed -n 's/^WAYLAND_DISPLAY=//p')
  if [ -z "$_wl" ]; then  # 用户管理器不可用时的兜底：扫 socket
    for _s in "$XDG_RUNTIME_DIR"/wayland-*; do
      [ -S "$_s" ] && _wl="${_s##*/}" && break
    done
  fi
  [ -n "$_wl" ] && export WAYLAND_DISPLAY="$_wl"
  unset _wl _s
fi
if [ -z "${DISPLAY:-}" ] && [ -n "${XDG_RUNTIME_DIR:-}" ]; then
  _d=$(systemctl --user show-environment 2>/dev/null | sed -n 's/^DISPLAY=//p')
  [ -n "$_d" ] && export DISPLAY="$_d"
  unset _d
fi
# <<< herdr/SSH: 找回图形会话剪贴板（KDE Wayland）<<<
```

要点：

- **值从 `systemctl --user show-environment` 取**——那是 Plasma 登录时注册进用户
  systemd 管理器的权威值（实测 `WAYLAND_DISPLAY=wayland-1`、`DISPLAY=:0`），
  比 socket 扫描猜测可靠；socket 扫描只作兜底。
- 图形会话里本来就带这些变量，整段空操作，无副作用。
- 对任何 SSH 登录同样生效（顺带修复"SSH 里手敲 `wl-paste` 报连不上"）。

## 验证

完全模拟 pane 环境（`env -i` 剥掉图形变量）跑交互式 bash：

```
$ env -i HOME=... TERM=xterm-256color XDG_RUNTIME_DIR=/run/user/1000 \
    PATH=... /bin/bash -ic 'echo $WAYLAND_DISPLAY $DISPLAY; wl-paste --list-types'
WAYLAND_DISPLAY=wayland-1 DISPLAY=:0
image/png        ← wl-copy 先放了张 PNG 进剪贴板，roundtrip 字节一致
```

## 生效方式

bashrc 只对新 shell 生效：

- herdr 里**已开着**的 pane：`exec bash` 后重新运行 `claude`；
- 或者开新 pane / 新 tab；
- 或者干脆 `herdr server stop` 后从 KDE 本地终端重新 `herdr`（server 直接继承
  图形会话环境，pane 全体生效）——但 server 迟早可能又从 SSH 拉起，bashrc 方案
  才是一劳永逸的。

## 边界与已知限制

- Wayland socket 按 UID 授权：**只对同一用户的会话**有效。SSH 用别的账号进来
  依然读不到剪贴板（也不应该读到）。
- 远程机器上的 herdr server（`herdr --remote`）连的是远端剪贴板，和本机 KDE
  无关，此修复不适用。
- 变量存在但图形会话已注销时，socket 消失，`wl-paste` 自然失败——属于预期。
