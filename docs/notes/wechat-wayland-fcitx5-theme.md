# 微信 Linux 原生 Wayland 排障笔记：输入法候选窗变样、渐变渗色与 Fcitx 5 皮肤编写指南

> **结论先行**：`wechat-universal-bwrap` 升级到 4.1.13.29 后，启动脚本默认改走原生 Wayland，输入法候选窗变样。
> 
> 根因是微信改用其二进制内静态链接的 **`fcitx5-qt` 客户端候选窗**自绘，不走系统的 fcitx 守护进程。内嵌客户端存在两大局限：
> 1. **不读 KDE 强调色**：忽略 `[AccentColorField]`，官方默认主题退化为死灰色（`#c0c0c0` 边框加 `#808080` 灰底）；
> 2. **SVG 解码插件缺失**：无法解码官方主题的矢量箭头 `prev.svg` / `next.svg`；
> 3. **无图纯色模式在高分屏下会产生"四周深中间浅的渐变晕光"**：`fillBackground()` 生成的 5×5 像素九宫格中白色中心仅 1 像素宽，在 1.6 倍屏幕缩放下受 `QPainter::SmoothPixmapTransform` 双线性平滑插值影响，蓝色边框大面积渗入中心白底。
> 
> **解法**：制作一套专供内嵌客户端的直角九宫格主题 `wechat-default`。使用带充足纯白缓冲区的 `panel.png`（消除双线性渗色）、纯蓝实心 `highlight.png`（边框贴合）、无损转换的 PNG 翻页图标。微信对配置文件支持**热监视重载**，修改立即生效，**全程无需重启微信**，规避风控风险。
> 
> 环境：CachyOS，KDE Plasma 6.8 Wayland，fcitx5 5.1.23，微信输入法引擎（fcitx5-wetypex）。日期：2026-11-24。

---

## 目录

- [一、现象与两次误区链](#一现象与两次误区链)
- [二、根因深入剖析](#二根因深入剖析)
  - [1. 渲染架构分流：微信自绘 vs 守护进程绘制](#1-渲染架构分流微信自绘-vs-守护进程绘制)
  - [2. 动态强调色丢失](#2-动态强调色丢失)
  - [3. 蓝色渐变晕光的数学与图形学成因](#3-蓝色渐变晕光的数学与图形学成因)
- [三、Fcitx 5 皮肤规范与微信内嵌端适配指南](#三fcitx-5-皮肤规范与微信内嵌端适配指南)
  - [1. 配置文件层级与路径](#1-配置文件层级与路径)
  - [2. 微信客户端候选窗认哪些键、忽略哪些键](#2-微信客户端候选窗认哪些键忽略哪些键)
  - [3. 九宫格切片与边距对齐公式](#3-九宫格切片与边距对齐公式)
  - [4. 如何消除双线性插值渗色](#4-如何消除双线性插值渗色)
- [四、完整皮肤资源与配置](#四完整皮肤资源与配置)
  - [1. 目录结构](#1-目录结构)
  - [2. theme.conf 完整内容](#2-themeconf-完整内容)
  - [3. 资源生成脚本 gen_theme.py](#3-资源生成脚本-gen_themepy)
- [五、安装、热重载与回退](#五安装热重载与回退)
- [六、排查经验与避坑要点](#六排查经验与避坑要点)

---

## 一、现象与两次误区链

升级 4.1.13.29 原生 Wayland 后，候选面板经历了四种外观变化：

| 阶段 / 版本 | 面板外观 | 选中词（首选词） | 其它候选词 | 状态与问题 |
|---|---|---|---|---|
| **原版（其它程序）** | 白底直角矩形，2px `#3daee9`（KDE 强调蓝）硬边框，无圆角无阴影 | 整块实心蓝底 `#3daee9`，纯白字 `#ffffff`，垂直贯通填充 | 纯黑字 `#000000` | 期望达到的标准外观 |
| **微信（旧默认配置）** | 白底直角矩形，2px `#c0c0c0` 灰边框 | 实心灰底 `#808080`，白字 `#ffffff` | 黑字 | 强调色失效，退化为死灰 |
| **微信（误判圆角版）** | 白底圆角，1px `#dcdcdc` 浅灰边框，外层带淡阴影 | 空心圆角蓝框（2px 描边无填充），淡蓝字 `#3daee9` | 黑字 | **误区一**：凭经验臆造圆角阴影线框，与原版完全相反 |
| **微信（无图纯色版）** | 白底直角矩形，2px 蓝色硬边框 | 实心蓝底 `#3daee9`，纯白字 `#ffffff` | 黑字，但**底色泛蓝、呈现四周深中间浅的渐变晕影** | **误区二**：去掉了图片改用纯色，却踩中 Qt 双线性滤波渗色 |
| **微信（最终优化版）** | 白底直角矩形，2px 蓝色硬边框，纯白底色 | 整块实心蓝底，纯白字，无缝贴合四周蓝边 | 纯黑字，**100% 纯白通透背景** | **最终方案**：大白芯缓冲区消除渗色，与原版 1:1 吻合 |

---

## 二、根因深入剖析

### 1. 渲染架构分流：微信自绘 vs 守护进程绘制

在 XWayland 下，微信输入框的焦点事件通过 X11 发送给 `fcitx5` 守护进程，浮动候选框是一个由 `fcitx5` 自身的 `libclassicui.so`（基于 Cairo 矢量绘制）直接向合成器提交的独立顶层窗口。

升级到原生 Wayland 后，由于沙箱和协议安全机制，微信静态链接了 `fcitx5-qt` 的 `fcitxcandidatewindow.cpp`。候选框变成了微信进程内部用 Qt `QPainter` 自行绘制并由其向 Wayland 合成器提交的 subwindow。两套代码、两套渲染引擎，表现截然不同。

### 2. 动态强调色丢失

在系统默认主题 `/usr/share/fcitx5/themes/default/theme.conf` 中：
```ini
[InputPanel/Background]
Color=#ffffff
BorderColor=#c0c0c0
BorderWidth=2

[InputPanel/Highlight]
Color=#808080

[AccentColorField]
0=Input Panel Border
1=Input Panel Highlight Candidate Background
```
fcitx5 守护进程读取 `[AccentColorField]`，从 KDE Plasma 的 `kdeglobals` 提取系统 Accent Color（`#3daee9`），动态将 `#c0c0c0` 和 `#808080` 替换掉。

但 `fcitx5-qt` 的主题解析器 `fcitxtheme.cpp` **完全没有实现 `[AccentColorField]` 相关逻辑**。它直读键值，因此直接使用系统主题时就会退化为灰色。

### 3. 蓝色渐变晕光的数学与图形学成因

当尝试不在主题中配置图片，仅配置纯色与边框：
```ini
[InputPanel/Background]
Color=#ffffff
BorderColor=#3daee9
BorderWidth=2
```
`fcitx5-qt` 源码中 `BackgroundImage::load` 会回退到 `fillBackground()`：
```cpp
void BackgroundImage::fillBackground(const QColor &border,
                                     const QColor &background,
                                     int borderWidth) {
    image_ = QPixmap(margin_.left() + margin_.right() + 1,
                     margin_.top() + margin_.bottom() + 1);
    borderWidth = std::min({borderWidth, margin_.left(), margin_.right(),
                            margin_.top(), margin_.bottom()});
    // ...
}
```
当 `margin` 为 `{2, 2, 2, 2}` 且 `borderWidth = 2` 时：
- `image_` 的尺寸仅为 `(2+2+1, 2+2+1) = 5 × 5` 像素。
- 外圈 2 像素填入边框色 `#3daee9`。
- 留给中间背景色 `#ffffff` 的区域**仅有 1 个像素宽、1 个像素高**（即 `QRect(2, 2, 1, 1)`）。

而微信在 `FcitxCandidateWindow::render` 中强制设置了：
```cpp
painter->setRenderHint(QPainter::SmoothPixmapTransform, true);
```
本机屏幕为 2880×1800，缩放倍率 `dpr = 1.6`。当 `drawPart` 使用双线性插值（Bilinear Interpolation）把这仅仅 **1×1 像素的白芯** 拉伸至几百像素宽的面板时，采样子像素会把与其紧邻的四周蓝色像素大比例混合进来。

结果就是：中心白色被污染为浅蓝，越靠边缘蓝色越浓，形成了一圈极其明显的蓝色渐变晕光。

---

## 三、Fcitx 5 皮肤规范与微信内嵌端适配指南

### 1. 配置文件层级与路径

Fcitx 5 用户主题置于 `~/.local/share/fcitx5/themes/<theme-name>/`。

全局界面配置为 `~/.config/fcitx5/conf/classicui.conf`。
> [!IMPORTANT]
> 微信内嵌的 `fcitx5-qt` 只能读取位于根部（无节头）的键，**切勿添加 `[ClassicUI]` 节头**，否则微信读不出该配置：
> ```ini
> Font=Sans 10
> Theme=wechat-default
> ```

### 2. 微信客户端候选窗认哪些键、忽略哪些键

来源：`fcitx5-qt` 源码 `fcitxtheme.cpp` 与 `fcitxcandidatewindow.cpp`：

| 配置段落 / 键 | 行为与特性 |
|---|---|
| `[InputPanel]` -> `NormalColor` | 未选中候选词文字颜色（黑字 `#000000`） |
| `[InputPanel]` -> `HighlightCandidateColor` | 选中项文字颜色（必须为白字 `#ffffff`，与实心高亮底色对比） |
| `[InputPanel]` -> `HighlightColor` / `HighlightBackgroundColor` | 仅控制**拼音预编辑区（Preedit）**的高亮文字色与高亮背景色 |
| `[InputPanel]` -> `ContentMargin` | 候选列表与窗口边缘的内间距。**用于对齐边框** |
| `[InputPanel]` -> `TextMargin` | 每个候选词文字的内边距 |
| `[InputPanel]` -> `PageButtonAlignment` | 翻页按钮对齐模式，常用 `Last Candidate` |
| `[InputPanel/Background]` | 九宫格背景。有 `Image` 时优先使用图片；若无 `Image` 才读 `Color`、`BorderColor`、`BorderWidth` |
| `[InputPanel/Highlight]` | 选中项高亮块九宫格。读 `Image`、`Margin` |
| `[InputPanel/PrevPage]` / `NextPage` | 翻页箭头。只在有上一页/下一页时才画，**仅支持 PNG**（SVG 无法解码） |
| **不支持的项目** | 强调色（`AccentColorField`）、深浅色跟随、序号专用字体/颜色、背景模糊、横向布局下的 `FullWidthHighlight` |

### 3. 九宫格切片与边距对齐公式

要让高亮实心色块与外边框无缝贴合、不留缝隙，必须满足边距几何公式：

在 `fcitxcandidatewindow.cpp` 中，首个候选项高亮块的绘制坐标计算如下：
$$\text{X}_{\text{hl}} = \text{ContentMargin.left} + \text{TextMargin.left} - \text{HighlightMargin.left}$$
$$\text{Y}_{\text{hl}} = \text{ContentMargin.top} + \text{TextMargin.top} - \text{HighlightMargin.top}$$

只要令：
$$\text{HighlightMargin} = \text{TextMargin} = 5$$
则高亮块的起始位置精确退化为：
$$\text{X}_{\text{hl}} = \text{ContentMargin.left}$$
$$\text{Y}_{\text{hl}} = \text{ContentMargin.top}$$

令 $\text{ContentMargin} = \text{BorderWidth} = 2$：
- 高亮色块左上角起始于 $(2, 2)$，刚好紧贴 2 像素的外边框内侧；
- 高亮色块高度为 $\text{candidatesHeight}$，面板总高为 $\text{candidatesHeight} + 2 + 2$；
- 高亮色块底边精确贴合底边框；
- 高亮色与边框颜色完全一致（`#3daee9`），左、上、下三边熔为一体，视觉上形成整块纯蓝实心高亮。

### 4. 如何消除双线性插值渗色

解决双线性滤波渗色的核心：**中心拉伸区（Part 5）必须拥有足够宽度的纯白缓冲区，且取样源完全处于纯白腹地**。

- `panel.png` 尺寸定为 64×64 像素。
- 边缘 2 像素为蓝色 `#3daee9`，其余 $60 \times 60$ 像素为 100% 纯白 `(255, 255, 255, 255)`。
- 切片参数 `[InputPanel/Background/Margin]` 设为 16：
  - 切出的中心区域取自 `(16, 16, 32, 32)`；
  - 该区域离外圈蓝色边框足足有 $16 - 2 = 14$ 像素的纯白距离；
  - 双线性插值滤波采样半径仅有 1 像素，绝对无法触及蓝边；
  - **中心区域 100% 呈现无暇纯白，彻底根除泛蓝渐变**。

---

## 四、完整皮肤资源与配置

### 1. 目录结构

完整的皮肤工程位于仓库的 `integrations/wechat/fcitx5-theme/`：

```
integrations/wechat/fcitx5-theme/
├── gen_theme.py           # 自动化生成/切图脚本
└── wechat-default/        # 皮肤资源本体
    ├── theme.conf         # 核心配置文件
    ├── panel.png          # 64x64 直角蓝边纯白缓冲区面板
    ├── highlight.png      # 32x32 纯蓝实心高亮块
    ├── prev.png           # 16x20 上一页箭头
    ├── next.png           # 16x20 下一页箭头
    ├── radio.png          # 单选按钮
    └── arrow.png          # 菜单子项箭头
```

### 2. theme.conf 完整内容

```ini
[Metadata]
Name=Default (WeChat Accent Blue)
Name[zh_CN]=默认（微信强调蓝）
Version=1
Author=Fcitx
Description=Default Theme with system accent color for WeChat embedded candidate window
ScaleWithDPI=True

[InputPanel]
NormalColor=#000000
HighlightColor=#ffffff
HighlightBackgroundColor=#3daee9
HighlightCandidateColor=#ffffff
PageButtonAlignment=Last Candidate

[InputPanel/TextMargin]
Left=5
Right=5
Top=5
Bottom=5

[InputPanel/ContentMargin]
Left=2
Right=2
Top=2
Bottom=2

[InputPanel/Background]
Image=panel.png

[InputPanel/Background/Margin]
Left=16
Right=16
Top=16
Bottom=16

[InputPanel/Highlight]
Image=highlight.png

[InputPanel/Highlight/Margin]
Left=5
Right=5
Top=5
Bottom=5

[InputPanel/PrevPage]
Image=prev.png

[InputPanel/PrevPage/ClickMargin]
Left=5
Right=5
Top=4
Bottom=4

[InputPanel/NextPage]
Image=next.png

[InputPanel/NextPage/ClickMargin]
Left=5
Right=5
Top=4
Bottom=4

[Menu]
NormalColor=#000000
HighlightCandidateColor=#ffffff
HighlightColor=#ffffff
HighlightBackgroundColor=#3daee9

[Menu/Background]
Image=panel.png

[Menu/Background/Margin]
Left=16
Right=16
Top=16
Bottom=16

[Menu/ContentMargin]
Left=2
Right=2
Top=2
Bottom=2

[Menu/CheckBox]
Image=radio.png

[Menu/SubMenu]
Image=arrow.png

[Menu/Highlight]
Image=highlight.png

[Menu/Highlight/Margin]
Left=5
Right=5
Top=5
Bottom=5

[Menu/Separator]
Color=#c0c0c0

[Menu/TextMargin]
Left=5
Right=5
Top=5
Bottom=5
```

### 3. 资源生成脚本 gen_theme.py

```python
#!/usr/bin/env python3
"""Generate the PNG assets of the WeChat-oriented fcitx5 theme.

WeChat's embedded candidate window (fcitx5-qt's client-side panel) has two limitations:
1. It does not support SVG icons (missing Qt SVG plugin).
2. Pure color background with BorderWidth creates a 5x5 pixmap in fillBackground().
   When upscaled under fractional scaling (e.g. 1.6x) with QPainter::SmoothPixmapTransform,
   bilinear filtering causes the blue border to bleed across the 1px white center,
   producing an unwanted blue gradient wash across the entire panel.

To eliminate the gradient and achieve the crisp, square-corner, solid-white look:
- panel.png: 64x64 square panel with 2px #3daee9 border and pure white center.
  With 16px 9-patch margin, the stretched center only samples pure white.
- highlight.png: 32x32 solid #3daee9 rectangle.
- SVG icons (prev, next, radio, arrow) are rendered into PNGs using rsvg-convert.
"""
import subprocess
import sys
from pathlib import Path
from PIL import Image

BLUE = (0x3D, 0xAE, 0xE9, 255)
WHITE = (255, 255, 255, 255)


def make_panel(out: Path, border: int = 2):
    """64x64 nine-patch panel with border px blue border and 100% white interior."""
    img = Image.new("RGBA", (64, 64), WHITE)
    for y in range(64):
        for x in range(64):
            if x < border or x >= 64 - border or y < border or y >= 64 - border:
                img.putpixel((x, y), BLUE)
    img.save(out / "panel.png")


def make_highlight(out: Path):
    """32x32 solid blue highlight rectangle."""
    img = Image.new("RGBA", (32, 32), BLUE)
    img.save(out / "highlight.png")


def convert_svgs(out: Path):
    """Convert default theme SVGs to pixel-perfect PNGs."""
    for name in ["prev", "next", "radio", "arrow"]:
        svg_path = Path(f"/usr/share/fcitx5/themes/default/{name}.svg")
        if svg_path.exists():
            subprocess.run(["rsvg-convert", str(svg_path), "-o", str(out / f"{name}.png")], check=True)


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "wechat-default")
    out.mkdir(parents=True, exist_ok=True)
    make_panel(out, border=2)
    make_highlight(out)
    convert_svgs(out)
    print(f"Generated clean assets in {out}")


if __name__ == "__main__":
    main()
```

---

## 五、安装、热重载与回退

### 安装部署

```bash
# 1. 部署主题目录
mkdir -p ~/.local/share/fcitx5/themes/
cp -r integrations/wechat/fcitx5-theme/wechat-default ~/.local/share/fcitx5/themes/

# 2. 配置微信与系统的主题绑定（注意：不能带 [ClassicUI] 节头）
printf 'Font=Sans 10\nTheme=wechat-default\n' > ~/.config/fcitx5/conf/classicui.conf
```

### 验证与热重载

- `fcitx5-qt` 内部维护了 `QFileSystemWatcher`，当检测到 `theme.conf` 变动时会立即热重载。
- **无需重启微信**，在微信输入框内再次打字即可看到即时生效的直角蓝边纯白面板。

### 回退

```bash
rm -rf ~/.local/share/fcitx5/themes/wechat-default
rm ~/.config/fcitx5/conf/classicui.conf
```

---

## 六、排查经验与避坑要点

1. **绝对不要随意重启或追踪（strace）微信**：
   频繁冷重启或挂载调试工具易触发微信安全风控机制导致封号。输入法皮肤是纯文件配置读取，完全支持热生效，只读手段与热重载完全足够。
2. **先读开源代码，不要盲测猜键名**：
   微信内嵌的候选窗来自 `fcitx5-qt` 开源代码。直接阅读 `fcitxtheme.cpp` 和 `fcitxcandidatewindow.cpp` 能在十分钟内厘清所有支持的键与不支持的键，避免浪费时间尝试无效参数。
3. **高分屏九宫格切片要防平滑插值渗色**：
   当缩放倍率非整数（如 1.6x）时，Qt 强制开启的双线性平滑插值会把九宫格边框色渗入紧邻的 1 像素中心。九宫格的中心取样区必须预留足够的单色安全距离（至少大于 4 像素）。
4. **看图要逐像素量化，不要相信第一印象**：
   排查初期曾把直角面板误判为圆角淡阴影，导致做出了南辕北辙的线框皮肤。通过 Python 读取局部像素 RGB 阵列能瞬间给出确切的硬边框坐标与背景色值。
