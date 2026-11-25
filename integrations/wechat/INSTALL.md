# 微信（wechat-universal-bwrap）沙箱与 Wayland 修复：发文件 / 收图片 / 全局快捷键 / 输入法皮肤

Arch 系 AUR **`wechat-universal-bwrap`**（官方 Linux 微信 + bwrap 沙箱）的四个故障。前两个（4.1.13.26-1 验证）根因相同：**沙箱把数据目录映射为家目录，沙箱内外路径互为死区**；后两个（4.1.13.29-1 验证）源于默认走原生 Wayland 后的外部接口与内嵌客户端差异。

| 修复 | 症状 | 验证的包版本 |
|---|---|---|
| A | 往聊天窗发不出文件 | `wechat-universal-bwrap` 4.1.13.26-1 |
| B | 微信里复制的图片贴到别的应用报错 | `wechat-universal-bwrap` 4.1.13.26-1 |
| C | 截图 / 显隐快捷键失灵，设置里提示冲突 | `wechat-universal-bwrap` 4.1.13.29-1 |
| D | 输入法候选框样式走样（死灰/线框/底色泛蓝） | `wechat-universal-bwrap` 4.1.13.29-1 |

## 问题与根因

启动脚本（`/usr/bin/wechat-universal`）的关键绑定：

- 宿主 `~/文档/WeChat_Data/home` ↔ 沙箱 `/home/$USER`（假 home，只放微信自身数据）
- 宿主机的 `~/下载`、`~/桌面`、`~/文档`（其余部分）**不进沙箱**

于是两个方向各断一路：

| 症状 | 机理 |
|---|---|
| 从文件管理器**拖拽 / 复制粘贴文件**到聊天窗 → 无反应或发送失败 | 微信拿到宿主机路径（如 `/home/$USER/下载/x.pdf`），该路径在沙箱内不存在 |
| 在微信里**复制图片**，粘贴到浏览器/LibreOffice 报错或粘出路径文本 | 微信往剪贴板放 `text/uri-list`，URI 是沙箱视角路径，宿主机上不存在（取 `image/png` 的应用如 GIMP 不受影响） |

这不是微信账号限制——账号受限的表现是发出后红叹号/转圈，与上面两类"根本没发出去"不同。

## 修复 A：发文件 —— 包内置 binds.list 机制

包官方支持自定义绑定：`~/.config/wechat-universal/binds.list` 每行一个目录，重启微信后以**读写**方式按同路径挂进沙箱。

```bash
mkdir -p ~/.config/wechat-universal
# heredoc 不加引号：$USER 在写入时展开为你的真实用户名
cat > ~/.config/wechat-universal/binds.list <<EOF
/home/$USER/下载
/home/$USER/桌面
/home/$USER/图片
/home/$USER/文档
EOF
```

按需增删目录；也可临时用 `wechat-universal --bind <目录>`（或环境变量 `WECHAT_CUSTOM_BINDS`，冒号分隔）。

然后**完全退出微信再启动**：托盘图标右键 → 退出（只关窗口进程仍在，新绑定不生效）。启动日志出现 `Custom bind: '/home/...'` 即已挂载。

生效后：拖拽、粘贴、微信内文件选择器三条路都通；微信「另存为」也能直接写进这些真实目录。

## 修复 B：收图片 —— 宿主机侧 symlink

```bash
ln -s ~/文档/WeChat_Data/home/xwechat_files ~/xwechat_files
```

宿主机上的死路径经 symlink 复活，微信无感；粘图片到 URI 优先的应用不再报错。

## 修复 C：全局快捷键失效 / 误报冲突 —— 补 `wechat.desktop`

升到 **`wechat-universal-bwrap` 4.1.13.29-1** 后，截图（`Alt+A`）、显示/隐藏窗口（`Ctrl+Alt+W`）在微信没有焦点时失灵，设置里这两项标 ⚠ 提示冲突——系统里其实没有任何程序占用这两个键。

根因与路径无关：启动脚本这次改成默认原生 Wayland，微信改经 `xdg-desktop-portal` 注册全局快捷键；注册前它先调 `org.freedesktop.host.portal.Registry.Register("wechat")` 登记应用 ID，门户要求应用 ID 对应同名的 `.desktop` 文件，而包里只装了 `wechat-universal.desktop`：

```
Could not register app ID: App info not found for 'wechat'
```

微信拿到登记失败就不再注册快捷键。完整证据链与抓包方法见 [docs/notes/wechat-bwrap-global-shortcuts.md](../../docs/notes/wechat-bwrap-global-shortcuts.md)。

```bash
install -Dm644 integrations/wechat/wechat.desktop ~/.local/share/applications/wechat.desktop
desktop-file-validate ~/.local/share/applications/wechat.desktop
```

本目录的 `wechat.desktop` 正文与包内备着的 `wechat-universal-alias-icon.desktop` 相同（那份只在构建时设 `WECHAT_ALIAS_ICON=yes` 才安装）。用户级文件不需要 root，包升级不覆盖。

然后**完全退出微信再启动**。首次会弹出 KDE 的「全局快捷键请求」对话框（WeChat 想要注册以下 2 个快捷键），**25 秒内点「确定」**——微信那边是 25 秒超时的阻塞调用，错过就再重启一次微信。确认一次后绑定记在 `~/.config/kglobalshortcutsrc` 的 `[wechat]` 下。

## 修复 D：输入法候选窗变样 / 泛蓝渐变 —— 部署 wechat-default 皮肤

升级到 **4.1.13.29-1** 原生 Wayland 后，微信内嵌了静态链接的 `fcitx5-qt` 客户端自绘浮动候选窗。由于内嵌客户端不读 KDE 强调色（导致默认主题退化为死灰）、不支持 SVG 图标，且在 1.6x 高分屏下无图纯色模式会因双线性插值渗色出现泛蓝晕影。

根因分析、图形学机理与皮肤编写指南见完整笔记 [docs/notes/wechat-wayland-fcitx5-theme.md](../../docs/notes/wechat-wayland-fcitx5-theme.md)。

部署预制好的 `wechat-default` 皮肤（直角 2px 蓝边、大白芯缓冲区消除渗色、实心蓝底白字、PNG 翻页图标）：

```bash
# 1. 复制皮肤目录
mkdir -p ~/.local/share/fcitx5/themes/
cp -r integrations/wechat/fcitx5-theme/wechat-default ~/.local/share/fcitx5/themes/

# 2. 写入全局主题指定并通知 fcitx5 热重载
# 注意：微信内嵌客户端只读无节头的根键，切勿加 [ClassicUI] 节头；同时锁定 DarkTheme 并关闭 UseDarkTheme
printf 'Font=Sans 10\nTheme=wechat-default\nDarkTheme=wechat-default\nPreferTextIcon=False\nUseAccentColor=True\nUseDarkTheme=False\n' > ~/.config/fcitx5/conf/classicui.conf
busctl --user call org.fcitx.Fcitx5 /controller org.fcitx.Fcitx.Controller1 ReloadAddonConfig s "classicui"
```

**无需重启微信**：`fcitx5-qt` 监听主题文件，修改后即时热重载生效。在微信输入框打字即可看到 1:1 还原的白底蓝框实心高亮面板。

> **注意 `fcitx5-wetypex` 覆写问题**：如果装了微信输入法引擎（`fcitx5-wetypex`），在 `fcitx5-wetypex-settings` 里改动主题或字号会触发插件覆写 `classicui.conf` 为 `wetypex-light`。若发现候选框突然变成绿白圆角样式，重跑上面两行命令即可立即恢复。

## 验证

- 发送：拖一个 `~/下载` 里的文件进聊天窗，正常发出
- 接收：微信里复制图片 → 粘贴进 LibreOffice，出图不报错
- 剪贴板诊断：`wl-paste --list-types`（应见 `image/png` 与 `text/uri-list` 并存）
- 快捷键（修复 C）：焦点在别的窗口时按 `Alt+A` 出截图框、`Ctrl+Alt+W` 切出/收起主窗口；`grep -A3 '^\[wechat\]' ~/.config/kglobalshortcutsrc` 能看到两条绑定
- 输入法（修复 D）：微信内输入候选框呈直角纯白底色、2px `#3daee9` 蓝色边框、首选词实心蓝底白字、翻页小三角显示正常，无任何圆角阴影或蓝色渐变晕光

## 卸载 / 复发

- 删掉 binds.list（或其中某些行）→ 重启微信即回退
- 微信重装/换包/`WeChat_Data` 结构变化后修复 B 会失效（symlink 悬空），重建该 symlink 即可
- 微信 temp 文件（RWTemp）生命周期短，复制图片后尽快粘贴；快照数据在剪贴板里不受影响
- 删掉 `~/.local/share/applications/wechat.desktop` → 重启微信后修复 C 回退（快捷键再次失效并误报冲突）
- 删掉 `~/.local/share/fcitx5/themes/wechat-default` 与 `~/.config/fcitx5/conf/classicui.conf` → 修复 D 回退
- **换用官方或非 bwrap 打包版本时务必先删这个用户级文件**：它带 `NoDisplay=true` 且用户级优先，会盖住对方自己的 `wechat.desktop` 菜单项

## 边界

- 修复 C 的文件名不能改：门户按应用 ID `wechat` 精确找 `wechat.desktop`
- 绑定是**读写**且同权：微信能读写这些真实目录，只绑你愿意暴露的目录
- 绑定 `~/文档` 会遮住沙箱假 home 里原有的 `文档/`（只含初始化空目录，无害）；自定义绑定在 bwrap 参数序列里位于主绑定之后，后挂载者生效
