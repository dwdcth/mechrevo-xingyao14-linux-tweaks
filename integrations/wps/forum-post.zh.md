# [治理帖] WPS Office Linux 版流氓进程实录：EverythingDaemon 卡死 FUSE 挂载导致合盖假死，附退出自动清场方案

## TL;DR

Arch 系（CachyOS + KDE）上 WPS 关窗不退进程是老生常谈，但这次它玩出了新高度：自带的 **EverythingDaemon**（全盘索引守护）爬到我的 rclone 类 FUSE 云盘挂载时，云盘守护丢了一个 readdir 回复，该线程从此卡死在内核 D 状态——**之后每次合盖挂起都会假死**（冻结失败），还连累锁屏 greeter 反复重启闪屏。分享完整诊断链 + 一键治理脚本。

## 症状

- 合盖再开盖：**假死**几十秒到一分多钟，试了两次都一样
- 之后锁屏界面**反复闪烁**

## 诊断（这段是重点，方法可复用）

合盖假死的第一反应不该是重启，而是看谁拦住了挂起：

```bash
sudo journalctl -k | grep -A5 'refusing to freeze'
```

关键输出：

```
Freezing user space processes failed after 20.007 seconds (1 tasks refusing to freeze, wq_busy=0):
task:ChangeMonitor   state:D stack:0     pid:125753 ...
Call Trace:
  fuse_chan_send+0x32c/0x570
  fuse_readdir+0x9f4/0x10b0
```

**D 状态任务 + 调用栈里有 `fuse_readdir`** = 有进程在 FUSE 挂载上做目录枚举，而 FUSE 守护进程没回话。顺藤摸瓜：

- 该线程属于 `EverythingDaemon`（`/usr/lib/office6/`，WPS 启动时拉起的全盘索引）
- 我挂了 qrypt（云盘聚合 FUSE 挂载 ~/Qrypt），晚高峰云端 API 抽风时守护丢回复，这个 readdir 就永垂不朽了
- systemd 侧同时能看到 `Failed to freeze unit 'user.slice': Connection timed out`

时间线对照也完美吻合：索引守护没启动的时间段挂起全部正常，它起来之后两次合盖全灭。

## 根因链

```
WPS 启动 → 拉起 EverythingDaemon 全盘爬索引
  → 爬进 FUSE 云盘挂载，readdir 丢回复
    → 线程永久 D 状态（不可中断）
      → 挂起冻结失败（EBUSY）= 合盖假死
        → 余波：锁屏 greeter 的 PAM worker 连环超时被杀 = 屏幕闪烁
```

三个槽点：退出 WPS 窗口后全家桶常驻（wpsoffice/wpscloudsvr/4×CEF/EverythingDaemon）；索引守护**没有任何目录排除配置**；卡死后无法自救。

## 治理方案（一键脚本见楼下/附件）

1. **永久禁 EverythingDaemon**：`/etc/pacman.conf` 加 `NoExtract = usr/lib/office6/EverythingDaemon` + 删现存二进制。代价：WPS 首页失去本机文件搜索，文档打开/编辑/另存完全不受影响（文件对话框是 Qt 自己的）
2. **退出自动清场**：`wps-launch` 包裹器 + 用户级 desktop 覆盖（`~/.local/share/applications/`，升级冲不掉）——全局盯 `wps/wpp/et/wpspdf` 文档进程，连续 60 秨一个都没有就 `systemctl --user stop` 整个 WPS scope。实测：关文档 → 70 秨后进程全空、包裹器自己退场、零常驻
3. **FUSE 卡 D 的急救**（通用技巧）：重启你的 FUSE 守护进程（如 `systemctl --user restart qrypt`），连接中止会让卡死的系统调用立刻返回错误，D 状态解除

## 附：一键脚本

`wps-clean-install.sh`（见附件），Arch 系 + wps-office-cn 验证通过：

```bash
bash wps-clean-install.sh   # 需要 sudo；自动处理 NoExtract + 包裹器 + desktop 覆盖
```

卸载：删 `~/.local/bin/wps-launch`、`~/.local/share/applications/wps-office-*.desktop`、pacman.conf 里那行 NoExtract。

## 适用范围与边界

- 验证环境：CachyOS（内核 7.2）+ KDE Plasma 6.8 + wps-office-cn 12.1.2
- 其他发行版：deb 系用 dpkg-divert 替代 NoExtract 思路，路径类比 `/opt/kingsoft/wps-office/office6/`
- 边界：终端裸敲 `wps` 不经包裹器；WPS 首页开着且零文档超 60 秒会被连坐关掉
- 不用 WPS 账号/云文档的同学有更狠的懒人方案：`sudo chmod -x /usr/lib/office6/wpsoffice`（首页/登录本体，禁了永不驻留，代价是不能登录账号）

有任何一台机器合盖假死的，先跑那句 `journalctl -k | grep -A5 'refusing to freeze'`——不管拦路的是 WPS、网盘还是别的什么，答案都在栈里。
