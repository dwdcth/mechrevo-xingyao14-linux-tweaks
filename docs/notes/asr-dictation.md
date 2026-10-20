# NPU 听写排雷记：从 AI 键到简体进光标

[integrations/asr](../../integrations/asr/INSTALL.md) 的操作手册之外，这里记录 2026-10-17 搭链路时试出来的结论——尤其是 **Wayland 下中文注入没有一条"正路"**，以下每条都是实测淘汰出来的。

## 输出通道排雷（核心结论）

目标：识别出的中文文本进入当前聚焦的输入框。KDE Wayland 下：

| 方案 | 结果 |
|---|---|
| wtype（Wayland 虚拟键盘协议） | **必败**——KDE 不开虚拟键盘协议，`zwp_virtual_keyboard_manager_v1` 无门 |
| ydotool 直接敲 CJK 字符 | **不支持**——voxtype 二进制内字符串明示 ydotool 驱动只收键码，中文字符被丢弃，**连剪贴板都不进** |
| voxtype `mode="paste"`（文本→剪贴板→uinput 自动 Ctrl+V） | ✅ 最终方案——Ctrl+V 是纯键码，无 CJK 限制；ydotool 守护提供 uinput 通道 |

所以配置里 `[output] mode="paste"` 不是可选项，是 Wayland CJK 的唯一通路。

## 繁简问题

Whisper 中文解码**常出繁体**（同音异写偏好）。用确定性转换兜底：

```toml
[output.post_process]
command = "opencc -c t2s.json"
```

依赖 `opencc` 包。不要试图用提示词/prompt 纠——识别端不稳定，后处理端才可靠。

## 机身 AI 键考证

- 星耀14 的 AI 键由 EC 固定发出 `LeftMeta+KEY_F23` 组合，内核侧无开关
- 抓键用 `sudo evtest /dev/input/event1`；**注意** evtest 输出头部的设备能力表会污染按键计数，统计时须 `grep 'value'` 过滤
- CachyOS 未随包带 libinput CLI 工具，别找 `libinput debug-events`，直接 evtest
- voxtype 自己 evdev 直读热键（`[hotkey] key="KEY_F23"`），需要用户在 input 组 + `/dev/input/event*` ACL

## ydotool 部署细节

- 用户级守护：`systemctl --user enable --now ydotool`，socket 在 `/run/user/<UID>/.ydotool_socket`
- voxtype 服务需要 `YDOTOOL_SOCKET` 环境变量指对 socket（见组件内 voxtype.service，用 `%U` 通配 UID）
- `/dev/uinput` 读写：`/etc/udev/rules.d/60-uinput.rules` 写 `KERNEL=="uinput", MODE="0660", GROUP="input"` + 用户入 input 组

## NPU 服务的两个坑

1. **memlock**：默认 8MB 远不够 NPU 编译产物加载，`flm check`/serve 会莫名失败。系统服务里 `LimitMEMLOCK=infinity`；手动跑用 `ulimit -l unlimited`
2. **模型下载**：flm 的 C++ HTTP 客户端**不读代理环境变量**。HF 域名被 DNS 污染时（假 IPv6 `2001:2::/48`）表现为无限卡死而非报错。解法：ModelScope 镜像手动下载（`model_list.json` 里每模型有 `ms_url`），落位 `~/.config/flm/models/<Name>/` 后 `flm check` 校验

## 顺带：大文件下载

GitHub release 大文件走代理易截断（`curl --max-time` 中断式失败），在 URL 前加 `https://ghfast.top/` 前缀镜像可加速。CachyOS 版 paru 不认 `--noedit`/`--use` 参数，写自动化脚本时注意。
