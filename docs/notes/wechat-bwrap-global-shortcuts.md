# wechat-universal-bwrap 4.1.13.29：截图 / 显隐快捷键失效并误报"冲突"——门户按应用 ID 找不到 wechat.desktop

> 结论先行：`Alt+A` 截图、`Ctrl+Alt+W` 显示/隐藏窗口在微信没有焦点时失灵，设置里这两项标 ⚠「冲突」——**系统里并没有任何程序占用这两个键**。断点在注册快捷键之前的一步：新版微信走 `xdg-desktop-portal` 注册全局快捷键，注册前先调 `org.freedesktop.host.portal.Registry.Register("wechat")` 登记应用 ID，而门户规定**应用 ID 必须对应同名的 `.desktop` 文件**。`wechat-universal-bwrap` 只装了 `wechat-universal.desktop`，没有 `wechat.desktop`，门户回 `Could not register app ID: App info not found for 'wechat'`，微信随即放弃注册全局快捷键。补一个用户级 `wechat.desktop` 重启即愈。

> 环境：CachyOS，KDE Plasma 6.8 Wayland。验证日期：2026-11-24。
> 安装步骤：[integrations/wechat/INSTALL.md](../../integrations/wechat/INSTALL.md) 修复 C。

## 涉及的包

| 包 | 本机版本 | 在这件事里的角色 |
|---|---|---|
| **`wechat-universal-bwrap`**（AUR） | 4.1.13.26-1 → **4.1.13.29-1** | 官方 Linux 微信 + bwrap 沙箱的打包。出问题的包，缺的文件也在它这里 |
| `xdg-desktop-portal` | 1.22.1-2.1 | 门户前端。`Register` 的校验和报错都出自它 |
| `xdg-desktop-portal-kde` | 6.8.3-1.1 | KDE 后端：弹确认对话框，把快捷键写进 kglobalaccel |
| `kglobalacceld` | 6.8.3-1.1 | KDE 全局快捷键守护进程，真正监听按键的一方 |
| `kwin` / `plasma-workspace` | 6.8.3-2.1 / 6.8.3-1.1 | 会话环境 |
| `bubblewrap` | 0.13.1-1.1 | 沙箱本体。只贡献了"沙箱版"这个前提，不是断点 |

升级记录（`/var/log/pacman.log`）：

```
[2026-11-24T07:38:50+0800] [ALPM] upgraded wechat-universal-bwrap (4.1.13.26-1 -> 4.1.13.29-1)
```

## 症状

- 焦点不在微信上时按 `Alt+A`（截图）、`Ctrl+Alt+W`（显示/隐藏窗口）没有任何反应
- 设置 → 快捷键里这两项前面带 ⚠，悬停提示冲突；「恢复默认设置」无效
- 同一页的 `Ctrl+L`（锁定微信）没有 ⚠
- 重启微信无效；KDE 系统设置 → 快捷键里搜不到任何占用 `Alt+A` / `Ctrl+Alt+W` 的动作

## 排查链

### 1. 变更点：一次升级，两处同时变了

AUR 打包脚本的 git 历史（`~/.cache/paru/clone/wechat-universal-bwrap`）：

```
00a9a5b 2026-11-23 up to 4.1.13.29
00c7463 2026-11-18 more wayland icon fixups and narrower wayland fcitx5 fix
2cc046f 2026-11-17 native wayland support and fix kde notification
7c20ed0 2026-10-13 up to 4.1.13.26
```

`2cc046f` 把启动脚本的平台默认值整个翻了个面（节选）：

```diff
-    # wechat-universal only supports xcb
-    export QT_QPA_PLATFORM=xcb \
+    # WeChat uses native desktop notifications on Wayland.
+    if [[ -z "${QT_QPA_PLATFORM}" ]]; then
+        if [[ -S "${WAYLAND_SOCKET}" ]]; then
+            QT_QPA_PLATFORM=wayland
```

即 4.1.13.26 时期微信被按在 XWayland 上（`xcb`），全局快捷键走 X11 抓键；升到 4.1.13.29 后启动器默认 `wayland`，进程环境里 `QT_QPA_PLATFORM=wayland`。

### 2. 微信自己的日志读不了，换 D-Bus 看它对外做了什么

`~/文档/WeChat_Data/home/.xwechat/log/*.xlog` 文件头 magic 是 `0x06`（带 64 字节公钥的加密格式），明文工具解不开。好在全局快捷键要跨进程，一定经过会话总线。

### 3. 二进制里是一套基于门户的实现

```bash
strings -n 5 /opt/wechat-universal/wechat | grep '^GlobalShortcut'
```

能看到完整的流程日志模板，以及 `org.freedesktop.portal.GlobalShortcuts` 接口名：

```
GlobalShortcut: [D-Bus thread] RegisterHostApplication enter, application_id=%s
GlobalShortcut: [D-Bus thread] sending CreateSession (send_with_reply_and_block, timeout_ms=25000)
GlobalShortcut: [D-Bus thread] sending BindShortcuts (send_with_reply_and_block, timeout_ms=25000)
GlobalShortcut: [origin thread] portal registration failed (%s); trying GNOME GSettings fallback
```

三个要点：新版微信靠门户注册全局快捷键；`RegisterHostApplication` 排在 `CreateSession` 之前；`BindShortcuts` 是阻塞调用，**只等 25 秒**。

### 4. KDE 这边排除"真冲突"

```bash
# 守护进程里已登记的组件：没有微信
qdbus6 --literal org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel.allMainComponents \
  | tr -d '\n' | sed 's/}, {/}\n{/g' | grep -i wechat        # 无输出

# 谁占着 Alt+A（参数 = 键码 | 修饰符，Alt=0x08000000、Ctrl=0x04000000）
busctl --user call org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel.getGlobalShortcutsByKey i 134217793
```

`Alt+A`（134217793）、`Ctrl+Alt+W`（201326679）、`Ctrl+L`（67108940）三者都是 `a(ssssssaiai) 0`——没有任何持有者；对照 `Meta+V` 能查到 CopyQ，说明查询本身有效。`~/.config/kglobalshortcutsrc` 里也没有任何微信分组。冲突提示是假的。

### 5. 抓包：登记应用 ID 这一步就被拒了

```bash
dbus-monitor --session \
  "interface='org.freedesktop.portal.GlobalShortcuts'" \
  "interface='org.freedesktop.impl.portal.GlobalShortcuts'" \
  "interface='org.freedesktop.portal.Request'" \
  "interface='org.freedesktop.portal.Session'" \
  "type='error'"
```

挂着监听重启微信，启动后 20 秒内门户回给微信的报错只有两条，**没有任何一次 GlobalShortcuts 调用**：

```
error ... sender=:1.35 -> destination=:1.369 error_name=org.freedesktop.portal.Error.Failed
   string "Could not register app ID: App info not found for 'org.chromium.Chromium'"
error ... sender=:1.35 -> destination=:1.371 error_name=org.freedesktop.portal.Error.Failed
   string "Could not register app ID: App info not found for 'wechat'"
```

`:1.35` 是 `org.freedesktop.portal.Desktop`（门户前端），`:1.369` / `:1.371` 是微信启动时临时开的私有连接（报错内容就是它登记的应用 ID）。

### 6. 读门户源码：校验只有一条——按 ID 找 desktop 文件

xdg-desktop-portal 1.22.1，`src/registry.c` → `register_host_app_info_sync()`：

```c
app_info = xdp_app_info_new_for_registered_sync (invocation,
                                                 app_id,
                                                 cancellable,
                                                 error);
if (!app_info)
  return NULL;
...
g_dbus_method_invocation_return_error (invocation,
                                       XDG_DESKTOP_PORTAL_ERROR,
                                       XDG_DESKTOP_PORTAL_ERROR_FAILED,
                                       "Could not register app ID: %s",
                                       error->message);
```

`xdp_app_info_new_for_registered_sync()` → `xdp_app_info_host_new_registered()`（`src/xdp-app-info-host.c`），构造时带上 `XDP_APP_INFO_FLAG_REQUIRE_GAPPINFO`；回到 `src/xdp-app-info.c` 的 `xdp_app_info_initable_init()`：

```c
priv->gappinfo =
  XDP_APP_INFO_GET_CLASS (app_info)->create_gappinfo (app_info);   // g_desktop_app_info_new("<id>.desktop")

if ((priv->flags & XDP_APP_INFO_FLAG_REQUIRE_GAPPINFO) && !priv->gappinfo)
  {
    g_set_error (error, G_IO_ERROR, G_IO_ERROR_NOT_FOUND,
                 "App info not found for '%s'", priv->id);
    return FALSE;
  }
```

接口文档（`data/org.freedesktop.host.portal.Registry.xml`）写得直白：

> The application ID must be able to match the basename of a .desktop file that describes the application.

### 7. 为什么只有这个包中招

- 微信硬编码用 `wechat` 作应用 ID（打包脚本注释里提过它窗口上写的也是 `desktop-entry=wechat`）
- `wechat-universal-bwrap` 为了不与官方包冲突，菜单项改名为 `wechat-universal.desktop`，系统里不存在 `wechat.desktop`
- 包里其实备着这个别名：`wechat-universal-alias-icon.desktop`，但 `PKGBUILD` 只在构建时设了 `WECHAT_ALIAS_ICON=yes` 才安装（默认不装），且注释只说是给 Plasma 解析图标用的

官方非沙箱包的菜单项想必就是 `wechat.desktop`，应该不受影响（本机没装，未实测）。

## 根因链

1. 启动器默认改原生 Wayland → 微信走门户注册全局快捷键
2. 注册前先 `Register("wechat")` 登记应用 ID
3. 门户用 `g_desktop_app_info_new("wechat.desktop")` 校验 → 文件不存在 → 报错
4. 微信拿到登记失败，不再调用 `CreateSession` / `BindShortcuts`
5. KDE 守护进程里根本没有微信的组件，按键自然无人监听；从现象的对应关系看，微信是把"没注册上"显示成了 ⚠「冲突」

## 修复

[wechat.desktop](../../integrations/wechat/wechat.desktop) 放进用户级应用目录（与包内别名文件内容一致）：

```bash
install -Dm644 integrations/wechat/wechat.desktop ~/.local/share/applications/wechat.desktop
desktop-file-validate ~/.local/share/applications/wechat.desktop
```

用户级文件不需要 root，包升级也不会覆盖。然后**完全退出微信再启动**。

首次会弹出 KDE 的「全局快捷键请求」对话框（WeChat 想要注册以下 2 个快捷键：截图 `Alt+A`、显示/隐藏窗口 `Ctrl+Alt+W`），**25 秒内点「确定」**——过了这个时间微信的阻塞调用超时，本次注册照样算失败，再重启一次微信重来。确认后绑定写进 `~/.config/kglobalshortcutsrc`；按后端源码的逻辑，之后启动时这两个 ID 已存在，不再弹框（未专门重启验证）。

## 验证

修复后再抓一次，`App info not found for 'wechat'` 消失，完整序列出现：

```
method call  wechat -> portal   GlobalShortcuts.CreateSession
signal       portal -> wechat   Request.Response               (create)
method call  wechat -> portal   GlobalShortcuts.BindShortcuts
                                main_panel_visibility  "显示/隐藏窗口"  Ctrl+Alt+W
                                screenshot             "截图"           Alt+A
signal       portal -> wechat   Request.Response               uint32 0   ← 发出请求 13.5 秒后，点了「确定」
signal       portal -> wechat   GlobalShortcuts.Activated      "screenshot"
```

确认后 5 秒内门户向微信派发了 4 次 `Activated`，按键已经通了。KDE 侧：

```bash
grep -A3 '^\[wechat\]' ~/.config/kglobalshortcutsrc
```

```
[wechat]
_k_friendly_name=WeChat
main_panel_visibility=Ctrl+Alt+W,none,显示/隐藏窗口
screenshot=Alt+A,none,截图
```

`getGlobalShortcutsByKey 134217793` 现在返回 `screenshot / 截图 / wechat / WeChat`，`Ctrl+Alt+W`（201326679）返回 `main_panel_visibility`。

## 边界与遗留

- **换回官方非沙箱包时删掉这个用户级文件**：它带 `NoDisplay=true`，会盖住官方的 `wechat.desktop`，菜单项消失
- `org.chromium.Chromium` 的登记失败依然存在（同样找不到 `org.chromium.Chromium.desktop`）。它不影响全局快捷键，其余影响未深究
- 顺带观察：微信还拿沙箱内 PID 去问过 systemd 用户实例（`Process 76 is a kernel thread, refusing.`），这是 `--unshare-pid` 带来的错位，微信随后改走显式 `Register`，不是本次断点
- 想改键：系统设置 → 快捷键，组件名 `wechat`（显示为 WeChat）
- 门户外加 GNOME 后端等也实现了 GlobalShortcuts，`Register` 的校验在前端、与桌面无关，同理应需 `wechat.desktop`（未实测）
- 退路是 `QT_QPA_PLATFORM=xcb` 把微信按回 XWayland 走旧的 X11 抓键路径（启动脚本仍支持），**未在 4.1.13.29 上验证**
- 值得反馈给 AUR 打包者：4.1.13.29 起在带 `org.freedesktop.host.portal.Registry` 的 xdg-desktop-portal 上，`wechat.desktop` 不再只是图标别名，而是全局快捷键的前提
