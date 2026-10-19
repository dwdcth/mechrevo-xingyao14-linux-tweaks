# EasyEffects 扬声器校正预设

星耀14 四扬声器 Nahimic 卷积校正的开源替代：原厂测量 IR 卷积 + 配套预设。实测听感与 Windows 原厂音效接近。

## 文件说明

- `机械革命原厂-扬声器校正-参数EQ版.json` / `mechrevo-star14-nahimic-speaker.irs(.wav)`：**主推**——卷积校正（IRS 来自原厂驱动包的测量数据）
- `机械革命原厂-音乐/电影/游戏/通话/国乐.json`：场景预设（音乐/电影/游戏/通话/国乐）
- `机械革命原厂-Mac曲线*.json`：MacBook 频响曲线移植（三种变体）
- `mic-noise-reduction.json`：麦克风降噪
- `scripts/`：IR 提取/转换脚本（研究用）

## 安装

```bash
# 预设 → EasyEffects 输出预设目录
cp *.json ~/.config/easyeffects/output/
# 卷积核 → IRS 目录（扬声器校正预设引用它）
mkdir -p ~/.config/easyeffects/irs
cp mechrevo-star14-nahimic-speaker.* ~/.config/easyeffects/irs/

# 登录自启（后台加载校正预设）
mkdir -p ~/.config/autostart
cat > ~/.config/autostart/easyeffects.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=EasyEffects（扬声器校正）
Exec=easyeffects -w -p 机械革命原厂-扬声器校正-参数EQ版
X-GNOME-Autostart-enabled=true
NoDisplay=true
EOF
```

> 若沿用原名 `speaker-correction.json`，请保持文件名与预设内部 name 字段一致，`-p` 参数填预设名。

## 验证

```bash
pgrep -a easyeffects            # 进程在
easyeffects -b 3                # 输出 2 = 旁路关闭（音效生效中）
```

## 卸载

删 `~/.config/autostart/easyeffects.desktop`、`~/.config/easyeffects/output/` 与 `irs/` 中对应文件，`easyeffects -q` 退出。

## 来源提示

IRS/预设衍生自机械革命 Windows 驱动包素材（研究/互操作用途），见根目录 [NOTICE.md](../../NOTICE.md)。
