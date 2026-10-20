# 机身 AI 键语音听写（NPU Whisper）

按一下机身 AI 键说话，松开，**简体中文直接出现在光标处**。链路：

```
AI 键(EC 固定发 LeftMeta+KEY_F23) → voxtype(evdev 直读热键)
  → flm serve(NPU 上跑 Whisper large-v3-turbo q4nx，OpenAI 兼容 API :52625)
  → 文本经 opencc 繁转简 → 剪贴板 → uinput 纯键码 Ctrl+V
```

在 Strix Point（Ryzen AI 9 365）+ CachyOS + KDE Wayland 上验证。输出通道的排雷过程（为什么必须 paste 模式）见 [docs/notes/asr-dictation.md](../../docs/notes/asr-dictation.md)。

## 前提

- **flm**：`fastflowlm` 包（`sudo pacman -S fastflowlm`，上游 [fastflowlm.com](https://www.fastflowlm.com/)；NPU 编译产物 xclbins 随包在 `/usr/share/flm/xclbins/`）
- **voxtype**：上游二进制放 `~/.local/bin/voxtype`（fastflowlm 生态配套听写前端，`chmod +x`）
- **ydotool**、**opencc**：官方仓库 `pacman -S ydotool opencc`
- 当前用户在 `input` 组（`sudo usermod -aG input $USER` 后重新登录）
- NPU 固件/XRT 就绪（与 [tools/xrun-rs](../../tools/xrun-rs/INSTALL.md) 同前提）

## 模型下载（HF 被污染时的绕行）

flm 是 C++ 自带 HTTP 客户端，**不读代理环境变量**；直连 huggingface 在 DNS 污染网络下会卡死（假 IPv6 `2001:2::/48`）。走 ModelScope 镜像：

1. 看 `~/.config/flm/models/model_list.json`，每个模型条目带 `ms_url`，格式：
   `https://modelscope.cn/api/v1/models/amd/<Name>/repo?FilePath=<文件>&Revision=master`
2. 逐个下载，放进 `~/.config/flm/models/<Name>/`（本机用的是 `Whisper-V3-Turbo-NPU2`）
3. `flm check` 校验完整性

## 安装

```bash
# 1. uinput 规则（voxtype/ydotool 注入键码需要）
sudo install -Dm644 60-uinput.rules /etc/udev/rules.d/60-uinput.rules

# 2. flm 服务（系统级；LimitMEMLOCK 是 NPU 运行必需，默认 8MB 不够；
#    $USER 在写入时展开——systemd 系统单元不展开 shell 变量）
sed "s/\$USER/$USER/g" flm-asr.service | sudo tee /etc/systemd/system/flm-asr.service >/dev/null
sudo systemctl daemon-reload && sudo systemctl enable --now flm-asr

# 3. ydotool 守护（用户级，提供 uinput 键码注入）
systemctl --user enable --now ydotool

# 4. voxtype 配置 + 服务（用户级）
mkdir -p ~/.config/voxtype
cp voxtype-config.toml ~/.config/voxtype/config.toml
mkdir -p ~/.config/systemd/user
cp voxtype.service ~/.config/systemd/user/
systemctl --user daemon-reload && systemctl --user enable --now voxtype
```

（`flm-asr.service` 里的 `$USER` 已由上面的 sed 在写入时展开；`voxtype.service` 用 `%h`/`%U` 通配，无需改。）

## 验证

```bash
# API 直测（OpenAI 兼容）
curl -s http://127.0.0.1:52625/v1/audio/transcriptions \
     -F file=@test.wav -F model=whisper-v3
# 服务状态
systemctl status flm-asr --no-pager
systemctl --user status voxtype ydotool --no-pager
```

端到端：任意输入框聚焦 → 按 AI 键说「今天天气不错」→ 松开 → 简体文本出现在光标处（繁体说明 opencc 没跑，查 config.toml 的 `post_process`）。

## 卸载

```bash
systemctl --user disable --now voxtype ydotool
sudo systemctl disable --now flm-asr
rm ~/.config/systemd/user/voxtype.service ~/.config/voxtype/config.toml
sudo rm /etc/systemd/system/flm-asr.service /etc/udev/rules.d/60-uinput.rules
```

## 边界

- 热键是**机身 AI 键**（EC 固定 `LeftMeta+KEY_F23`，见 [docs/hardware-map.md](../../docs/hardware-map.md)）；其他键改 config.toml 的 `[hotkey]`
- KDE Wayland 下**不要**尝试 wtype/虚拟键盘协议路线（KDE 不开），也**不要**用 ydotool 直接敲 CJK（不支持，见排雷记）
- memlock：服务内已 `LimitMEMLOCK=infinity`；手动跑 flm 时 `ulimit -l unlimited`（或 `sudo prlimit --memlock=8G -p $$`）
