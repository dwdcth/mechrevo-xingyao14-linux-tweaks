# EasyEffects 扬声器与麦克风音效（纯原生 Linux 方案）

星耀 14（Realtek ALC256，子系统 ID `1D05E004`）内置扬声器与麦克风的**纯原生 EasyEffects 调校方案**（无需 Wine）：以原厂 `1D05E004_InternalSpeakers.nsx` 无损 32-bit float 双声道 FIR 硬件校正为地基，结合 EasyEffects 8.x 专业插件（`LSP` + `Calf` + `DeepFilterNet` + `Zita-Convolver`），实现**高响度、零破音、清晰人声与双向 AI 降噪**。

## 前提与依赖

- 验证基线：PipeWire + WirePlumber 0.5+ + EasyEffects 8.3.3（Qt6 版）
- 必需插件包（Arch / CachyOS）：
  ```bash
  sudo pacman -S easyeffects lsp-plugins calf deepfilternet-ladspa
  ```

## 文件说明

- `mechrevo-star14-nahimic-raw-f32.irs`：**核心卷积核**，从原厂 `1D05E004_InternalSpeakers.nsx` 直接无损导出的 2048-tap 32-bit IEEE Float 立体声 48 kHz 最小相位 FIR（配合 `autogain: false` 保留原厂 0 dB 单位增益基准）
- `output/`（5 套输出预设，覆盖原版 4 场景 + 免切换主力）：
  - **`机械革命原厂-全能均衡.json`**（**默认主推**，听歌/会议/网页视频免切换）：前置轻度心理声学虚拟低音 + `Deesser` 消齿音 + 原厂 FIR/人声曲线 + 4 段防拍边压缩 + 2x 过采样限幅
  - **`机械革命原厂-音乐精调.json`**：MaxxBass 原理虚拟低音（40–130 Hz 生成泛音后 80 Hz 高通防拍边）+ `Exciter` 高频泛音空气感 + 宽松 `1.8:1` 高保真动态 + 轻度声场（`0.18`）
  - **`机械革命原厂-观影视频.json`**：影院虚拟低音 + `Deesser` 柔化 UP 主麦齿音 + 强力 4 段音量平衡（拉齐 B 站/YouTube/电影音量差）+ 温和环绕（`0.22`）
  - **`机械革命原厂-通话.json`**：**输出端接入 `DeepFilterNet`（`18 dB` 温和 AI 降噪，实时过滤对面背景键盘/风扇声）** + `Deesser` 防耳麦刺耳 + 极致人声清晰度
  - **`机械革命原厂-游戏.json`**：去掉易模糊定位的混响，改为纯净宽声场（`0.28`）+ 脚步声/枪械高频强化 + 宽松动态保留远近距离感
- `input/`（1 套输入预设）：
  - **`机械革命原厂-麦克风降噪-DeepFilterNet.json`**：`90 Hz` 高通 + `DeepFilterNet`（`40 dB` 衰减，不开硬噪声门防吞字）+ 轻度人声 EQ + 语音稳压压缩（对应原厂 `CaptureCompressor`）+ `-1.5 dB` 限幅防爆麦
- `legacy/`：早期 16-bit 单声道 FIR 与旧版实验预设归档（历史参考，勿直接拷贝进 EasyEffects）
- `scripts/`：原厂 `.nsx` 解码与 FIR 分析脚本

## 安装

EasyEffects 8.x（Qt6）数据目录位于 `~/.local/share/easyeffects/`：

```bash
# 1. 复制输出预设、输入预设与无损原厂卷积核
mkdir -p ~/.local/share/easyeffects/{output,input,irs}
cp output/*.json ~/.local/share/easyeffects/output/
cp input/*.json ~/.local/share/easyeffects/input/
cp mechrevo-star14-nahimic-raw-f32.irs ~/.local/share/easyeffects/irs/

# 2. 登录自启（后台服务模式）
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/easyeffects.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=EasyEffects（扬声器音效服务）
Comment=登录时以后台服务模式启动 EasyEffects
Exec=easyeffects --service-mode
X-GNOME-Autostart-enabled=true
NoDisplay=true
EOF

# 3. 启动服务并加载默认预设
systemctl --user daemon-reload
systemctl --user restart app-easyeffects@autostart.service
easyeffects -l 机械革命原厂-全能均衡
easyeffects -l 机械革命原厂-麦克风降噪-DeepFilterNet
```

> **重要**：系统扬声器音量请保持在 **100% 以内**（推荐 50%–85%）。超过 100% 会在 EasyEffects 限幅器之后叠加 PipeWire 软件硬放大，造成数字削波破音。

## 调音排坑记录（相对旧版的核心改进）

1. **无损 32-bit 立体声 FIR + `autogain: false`**：原厂 FIR 原始峰值为 `1.02`，旧版转 16-bit 单声道并开启 `convolver autogain` 会额外把整体电平压低 3.6–6 dB。
2. **修正原厂 EQ 带宽（Q 值）**：原厂 `BassBoost`（`250 Hz`, `0.707 oct`）精确对应 `Q = 2.02`；`VoiceBoost`（`500–4000 Hz`, 3 oct）精确对应 `Q = 0.45`，并轻收 `1250 Hz` 叠加鼻音峰。
3. **补齐多段压缩参数与原厂 `Attenuator` 保护**：修正旧版 `dry: 0.0`（混入未压缩干声）、`knee: +6`（LSP 插件负值才是软拐点）与缺失 `ReferenceDB = -12 dB` 补偿增益的问题；用 4 段压缩（分频 `200 / 900 / 4500 Hz`）兼顾 `< 900 Hz` 扬声器振膜防拍边保护与人声响度抬升。
4. **关闭 `Limiter ALR` 释放有效 RMS**：开启 `alr: true` 会将整段输出平均响度压死在 `-14.4 dBFS`；改为 `alr: false` + `Herm Thin` + `2x 过采样` + `-1.0 dB` 限幅后，轻声会议（`-25.6 dBFS`）与正常视频（`-15.5 dBFS`）均可稳定输出约 `-6.9 dBFS` RMS 且实测 **0 削波采样点**。
5. **修正自启参数**：EasyEffects 8.x 中 `-p` 为列出预设而非加载预设，后台常驻应使用 `--service-mode`。

## 验证

```bash
easyeffects -p     # 列出 5 输出 + 1 输入预设
easyeffects -s     # 查看当前激活的输入/输出预设
easyeffects -b 3   # 输出 2 = 旁路关闭（音效生效中）
```

## 卸载

删除 `~/.config/autostart/easyeffects.desktop` 及 `~/.local/share/easyeffects/{output,input,irs}/` 中对应文件，运行 `easyeffects -q` 退出。

## 来源提示

IRS 卷积核与原厂参数参考衍生自机械革命 Windows 驱动包素材（研究/互操作用途），见根目录 [NOTICE.md](../../NOTICE.md)。
