# XWayland 全屏黑屏：截图正常、肉眼全黑（AMD 直扫排障记）

> 结论先行：KDE Wayland 下 XWayland 处于「应用程序自行缩放」模式 + X 窗口铺满物理屏 → KWin 走硬件**直扫（direct scanout）**→ 该路径在 AMD 驱动上输出黑屏。合成器缓冲区是正常的（**截图有完整内容**），肉眼全黑，且窗口几何被 KWin 锁定。根治 = 显示配置里把旧式应用程序(X11)缩放改为「由系统缩放」。

2026-10 在本机（eDP 2880×1800 @160% 缩放，AMD 核显，KDE Plasma 6 Wayland）调试万兴 PDFelement（Bottles + soda runner）时撞上，耗费整场会话定位，记录于此。

## 症状

- wine/X11 应用**最大化或全屏**后整窗黑屏；普通小窗口正常
- `spectacle -b -n -o x.png` 截图**有内容**——不是死机、不是窗口没画
- 客户端的 move/resize 请求被拒（KWin 锁定直扫窗口几何，窗口"切不过去"）
- `xrandr` 改模式/缩放一律 BadMatch

## 根因链

1. KDE 显示缩放对 XWayland 有两档：**应用自行缩放**（向 X 客户端报告物理分辨率）与**由系统缩放**（报告逻辑分辨率）。默认前者
2. 程序按物理 2880×1800 建窗 → 铺满物理屏的 X 窗口满足 KWin 硬件直扫条件（合成被旁路）
3. AMD 驱动的直扫输出路径黑屏 → 截图（走合成器/截屏通道）正常，肉眼（走扫描输出）全黑

## 快速验证（不改任何配置）

把窗口从 fullscreen/maximized 状态拆下来、缩成普通窗口：

```bash
wmctrl -i -r <窗口id> -b remove,fullscreen,maximized_vert,maximized_horz
wmctrl -i -r <窗口id> -e 0,100,100,1300,870
```

窗口立刻可见 = 实锤直扫问题（当年 PDFelement 全黑就是靠缩到 1300×870 复明的）。

黑屏定量诊断（人眼读图会误判，用像素统计）：

```bash
spectacle -b -n -o /tmp/x.png
python3 - <<'EOF'
from PIL import Image
im = Image.open('/tmp/x.png').convert('L')
px = list(im.getdata())
print(f'平均亮度 {sum(px)/len(px):.1f} / 255，纯黑占比 {sum(p < 8 for p in px)/len(px):.1%}')
EOF
```

## 根治

KDE **系统设置 → 显示和监视器 → 显示配置 → 旧式应用程序(X11)缩放 → 由系统缩放**，注销重登。此后 XWayland 报告逻辑分辨率，X 窗口不再满足直扫条件。代价是 X 应用由合成器放大（轻微模糊），换来不再黑屏。

## 别浪费时间

- `xrandr` 在该环境改缩放/模式 → BadMatch 拒绝，死路
- 换 wine runner / 重装应用无关——这是显示层问题，不是应用问题
- 显示器/线材/合盖排线更无关：截图有内容就说明渲染管线是好的
