# 盒盖睡死事故排查与挂起保险丝（2026-10-24 / 10-25）

两天两次盒盖后黑屏假死、强制断电。根因不同，死法相同。

## 死法链（共同机制）

```
用户进程卡 D 状态(不可中断 IO 等待)
  → systemd 冻结 user.slice 60s 超时: "Failed to freeze unit 'user.slice': Connection timed out"
  → systemd-sleep 无视冻结失败仍写 /sys/power/state
  → 内核 "Freezing user space processes failed after 20.00s (N tasks refusing to freeze)"
  → 正常时以 -EBUSY 体面放弃(黑屏 1-2 分钟后恢复,系统活着)
  → 若中止窗口内恰有开盖唤醒事件 → 永久卡死(printk 环形缓冲随断电丢失)
```

两次致命案例都在合盖后 2-3 秒开盖——正好落在挂起中止的竞态窗口。开盖/电源键全程被
logind 忽略（"挂起进行中"），表象即"睡死开不了机"。

## 两次事故的 D 状态来源

- **10-24 14:50**：`SiYuan-Kernel` / `siyuan` / `Chrome_ChildIOT`（pacman siyuan-bin，非
  AppImage），栈 `filemap_fault → folio_wait_bit`——QLC 盘（Crucial P3 Plus，无 DRAM）
  重载日可执行页读回停顿 >20s。
- **10-25（全天 7 次挂起全部失败）**：云盘 FUSE 挂载的守护进程停止响应（同类工具：
  rclone mount、WebDAV 挂载、qrypt 等，此处不点名实际服务）。一条 `find`（pid 225749）
  从 24 日 23:27 卡到 25 日 13:06+（≥13.5h），栈 `fuse_readdir → fuse_chan_send`；该
  守护自己的 ioq-0..6 工作线程与一条 python3 后来也卡进去。守护一般在云后端断连
  （或代理链路异常）时挂。

## 排查探针（下次直接用）

```bash
journalctl -b -1 -g 'refusing to freeze|Failed to freeze|PM: suspend|Performing sleep'
journalctl -b -1 -k | grep -a 'task:'        # 内核点名拒绝冻结的进程+完整栈
ps -eo state,comm | grep -c '^D'             # 合盖前自检,非 0 先别合盖
```

## 处置

1. **挂起保险丝**：`tools/suspend-guard/`——ExecStartPre 检测持续 D 状态任务则中止挂起
   （睡死 → 没睡成），弹窗点名。已装于 /usr/local/sbin + 三个睡眠服务 drop-in。
2. 云盘挂载点无响应：`systemctl --user restart <对应 FUSE 服务>`（设计良好的 unit 应
   自带残留挂载清理）。
3. 卡死客户端可直接 `kill -9`（FUSE 等待是可杀的）。
4. 复发取证：强制断电前先 Alt+SysRq+REISUB 或有线 SSH（WiFi 已被 NM 睡掉）。

## find 与死挂载

`find` 没有全局排除配置（无 /etc/findrc、无 FIND_OPTIONS）。可用手段：

- 交互 shell 里包函数注入 `-prune`（注意 `-delete`/`-depth` 下 prune 失效）；
- `find -xdev` 不跨文件系统——云盘/网络 FUSE 挂载（rclone、WebDAV、qrypt 之类）天然
  绝缘（代价：其它 btrfs 子卷
  挂载也会被跳过）；
- `locate` 用户：`/etc/updatedb.conf` 的 `PRUNEPATHS` 才是真正的全局排除。

经验：跑大范围 find 前先看挂载点 `ls` 是否秒回；挂死时 walker 会全部陪葬。

**已选方案（2026-10-25）**：日常改用 `fd`，把云盘挂载目录写进 `~/.config/fd/ignore`
全局禁入（fd 默认同样不绕开 FUSE 死挂载，必须显式排除；注意 `fd -I/--no-ignore` 会连
全局 ignore 一起关掉）。
