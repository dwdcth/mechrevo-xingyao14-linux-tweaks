# suspend-guard —— 挂起保险丝

防止"盒盖睡死"与"幽灵盖子挂起循环"的最后一道闸门。挂在 `systemd-suspend.service`
（及 hibernate 两兄弟）的 `ExecStartPre`，**只在睡眠/合盖请求瞬间运行，零常驻**：

- 发现**持续 D 状态的线程** → 先自动 `kill -9` 卡死进程（SIGKILL 关闭 /dev/fuse，
  内核 abort 在途请求，D 等待随之释放）→ 清干净则放行，照常入睡；
- 杀不动（D 在真实硬件 IO 上）→ 挂起服务直接失败，"睡死强制断电"降级为
  "没睡成，系统保持醒着"，桌面弹窗点名卡死线程；
- 发现 **EC 假闭合**（盖子状态卡死）→ 拒绝挂起，弹窗指引断电复位 EC。

## 背景与原理

D 状态（不可中断 IO 等待）任务无法被挂起冻结器冻结。睡死事故链：

```
用户线程卡 D(QLC 读停顿 / FUSE 守护挂死…)
  → user.slice cgroup 冻结 60s 超时
  → systemd-sleep 无视失败仍写 /sys/power/state
  → 内核冻结 2×20s 失败中止(正常时以 -EBUSY 体面放弃,黑屏 1-2 分钟)
  → 若中止窗口内恰有开盖唤醒 → 永久卡死 → 强制断电
```

三次事故：2026-10-24（QLC 盘读停顿，三进程卡 NVMe 页错误）、2026-10-25 与
11-05（云盘 FUSE 守护挂死——rclone mount / WebDAV / qrypt 之类，不点名实际
服务）。详见 `docs/notes/suspend-freeze-deadlock.md`。

EC 假闭合事故（2026-11-04，体感"反复注销"）见
`docs/notes/lid-stuck-suspend-loop.md`。

## 判定与处置逻辑

**检查 1（幽灵盖子）**：ACPI lid 当前=闭合，但本 boot 最后一条 lid 事件不是
`Lid closed.`（含"从无事件"）→ 判 EC 状态位卡死，拒绝挂起。真实合盖必有翻转
事件；卡死闭合是 logind 启动时的初始值，不产生事件。journal 不可用时放行
（fail-open）。

**检查 2（线程级 D 状态）**：

1. 采样**全部线程**的 D 状态（`ps -eLo`，含非 leader 线程）——内核冻结器按
   线程冻结，"主线程健康、工作线程卡 D"的 FUSE 守护对 `ps -e` 级（仅主线程）
   采样完全不可见（11-05 三次假阴性放行的根因）；内核线程 cmdline 为空，跳过；
2. `RECHECK_SEC=2` 秒后二次采样，取**线程 TID** 交集 = 持续卡死（瞬时 IO 不误伤）；
3. **自动清理**：对交集线程的主进程 `kill -9`（TGID ≥ `KILL_PROTECT_MIN_TGID`
   才杀，系统进程只点名不动手），等 `KILL_WAIT_SEC` 秒复查——清干净放行挂起；
4. 仍有幸存者 → 中止挂起 + 弹窗点名（此时 kill 已无效，D 在硬件 IO 上）。

两项都干净时放行仅耗一次 `ps` + 一次 `journalctl`（百毫秒级），不影响正常
挂起速度。

## 安装

```bash
sudo install -m755 suspend-guard.sh /usr/local/sbin/suspend-guard
for u in systemd-suspend.service systemd-hibernate.service systemd-suspend-then-hibernate.service; do
  sudo mkdir -p "/etc/systemd/system/$u.d"
  printf '[Service]\nExecStartPre=/usr/local/sbin/suspend-guard\n' | \
    sudo tee "/etc/systemd/system/$u.d/guard.conf" >/dev/null
done
sudo systemctl daemon-reload
```

## 验证

```bash
/usr/local/sbin/suspend-guard                        # 干净路径: PASS, exit=0
SUSPEND_GUARD_DEMO=1 /usr/local/sbin/suspend-guard   # D 状态拦截演示: 弹窗+exit=1(不 kill)
SUSPEND_GUARD_DEMO_LID=1 /usr/local/sbin/suspend-guard  # 幽灵盖子演示: 弹窗+exit=1
journalctl -u systemd-suspend.service -n 5           # 真实拦截/自动清理记录
```

## 参数（脚本头部）

| 变量 | 默认 | 说明 |
|---|---|---|
| `NOTIFY_USER` / `NOTIFY_UID` | banshan / 1000 | 弹窗接收者 |
| `RECHECK_SEC` | 2 | D 状态持续判定窗口 |
| `KILL_WAIT_SEC` | 5 | 自动清理后等待 D 线程退出再复查 |
| `KILL_PROTECT_MIN_TGID` | 1000 | TGID 小于该值只点名不 kill（系统进程保护网） |
| `RATE_LIMIT_SEC` | 90 | 弹窗限频（防挂起重试风暴刷屏） |
| `LID_SKIP_STAMP` | /run/suspend-guard/skip-lid | touch 此文件临时停用幽灵盖子检查 |

## 已知限制

- **kill 的边界**：自动清理只对 FUSE 类卡死有效（连接关闭 → 内核 abort）；
  D 在真实硬件 IO 上（QLC 读停顿）时 kill 无效，走中止拦截。
- **误拦面**：盖子合着开机（翻盖外接屏使用）会被幽灵盖子检查拦住自动挂起——
  `touch /run/suspend-guard/skip-lid` 绕过，开盖后自动恢复正常判定。
- 守卫只管"睡前急救"；根因（云盘 FUSE 守护挂死、QLC 高负载读停顿）仍需单独
  治理：挂载点无响应时 `systemctl --user restart <对应的云盘 FUSE 服务>`。
- 卸载：删除 `/etc/systemd/system/systemd-*suspend*.service.d/guard.conf`（及
  hibernate 对应项）与 `/usr/local/sbin/suspend-guard`，`daemon-reload`。
