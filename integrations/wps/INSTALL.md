# WPS Office 流氓进程治理

Arch 系上 WPS（wps-office-cn）的两宗罪与本仓库的对策。**完整故事与诊断过程见 [forum-post.zh.md](forum-post.zh.md)**（论坛分享稿）。

## 问题

1. **合盖假死根因**：WPS 自带全盘索引守护 `EverythingDaemon`（/usr/lib/office6/）爬 FUSE 云盘挂载时，若 FUSE 守护丢回复（云盘网络抽风），线程永久卡 D 状态 → 挂起冻结失败（`1 tasks refusing to freeze`，栈里 `fuse_readdir`）→ 合盖假死 + 锁屏 greeter 连环重启闪屏。它**没有任何目录排除配置**
2. **退出不清场**：关闭所有 WPS 窗口后全家桶常驻（wpsoffice 首页入口 / wpscloudsvr / 4×CEF）

## 方案

- `NoExtract = usr/lib/office6/EverythingDaemon`（pacman.conf）→ 索引守护永不再落地。代价仅 WPS 首页失去本机搜索
- `wps-launch` 包裹器 + 用户级 desktop 覆盖（`~/.local/share/applications/`，升级冲不掉）→ 全局盯 `wps/wpp/et/wpspdf` 文档进程，连续 60 秨全无即停整个 WPS scope

## 安装（一键）

```bash
sudo bash integrations/wps/wps-clean-install.sh
```

（脚本自动处理 NoExtract、包裹器安装、desktop 覆盖、KDE 菜单缓存刷新；支持普通用户 sudo 调用，自动定位真实用户家目录。）

## 验证

从菜单/Dolphin 打开 WPS 文档 → 关闭所有窗口 → 约 70 秨后：

```bash
ps -eo comm | grep -iE 'office6|wps'    # 应为空
systemctl --user list-units 'app-wps*'  # 应无输出
```

## 卸载

```bash
rm -f ~/.local/bin/wps-launch ~/.local/share/applications/wps-office-*.desktop
# 再删 /etc/pacman.conf 里的 NoExtract 行
```

## 边界与替代

- 终端裸敲 `wps` 不经包裹器（只有 desktop 入口走 wrapper）
- WPS 首页开着且零文档超 60 秨会被连坐关闭
- 不用 WPS 账号/云文档的懒人方案：`sudo chmod -x /usr/lib/office6/wpsoffice`（首页/登录本体，禁了永不驻留，代价是不能登录账号）
- FUSE 卡 D 急救（通用）：重启你的 FUSE 守护（如 `systemctl --user restart qrypt`），连接中止会立刻唤醒卡死线程
- 合盖假死通用诊断口诀：`sudo journalctl -k | grep -A5 'refusing to freeze'`——拦路者是谁，答案在栈里
