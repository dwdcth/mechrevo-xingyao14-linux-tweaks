# 微信（wechat-universal-bwrap）沙箱路径双向修复

Arch 系 AUR `wechat-universal-bwrap`（官方 Linux 微信 + bwrap 沙箱，4.1.13.26 验证）的两个高频故障，根因相同：**沙箱把数据目录映射为家目录，沙箱内外路径互为死区**。

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

## 验证

- 发送：拖一个 `~/下载` 里的文件进聊天窗，正常发出
- 接收：微信里复制图片 → 粘贴进 LibreOffice，出图不报错
- 剪贴板诊断：`wl-paste --list-types`（应见 `image/png` 与 `text/uri-list` 并存）

## 卸载 / 复发

- 删掉 binds.list（或其中某些行）→ 重启微信即回退
- 微信重装/换包/`WeChat_Data` 结构变化后修复 B 会失效（symlink 悬空），重建该 symlink 即可
- 微信 temp 文件（RWTemp）生命周期短，复制图片后尽快粘贴；快照数据在剪贴板里不受影响

## 边界

- 绑定是**读写**且同权：微信能读写这些真实目录，只绑你愿意暴露的目录
- 绑定 `~/文档` 会遮住沙箱假 home 里原有的 `文档/`（只含初始化空目录，无害）；自定义绑定在 bwrap 参数序列里位于主绑定之后，后挂载者生效
