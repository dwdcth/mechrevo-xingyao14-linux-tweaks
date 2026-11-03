# suspend-guard —— 挂起保险丝

防止"盒盖睡死"的最后一道闸门。挂在 `systemd-suspend.service`（及 hibernate 两兄弟）的
`ExecStartPre`：发现**持续 D 状态**的用户进程时直接让挂起服务失败——结果从"睡死强制断电"
降级为"没睡成，系统保持运行，按键唤屏"，并桌面弹窗点名卡死进程。

## 背景与原理

D 状态（不可中断 IO 等待）进程无法被挂起冻结器冻结。事故链：

```
用户进程卡 D(QLC 读停顿 / FUSE 守护挂死…)
  → user.slice cgroup 冻结 60s 超时
  → systemd-sleep 无视失败仍写 /sys/power/state
  → 内核冻结 2×20s 失败中止(正常时以 -EBUSY 体面放弃,黑屏 1-2 分钟)
  → 若中止窗口内恰有开盖唤醒 → 永久卡死 → 强制断电
```

两次事故：2026-10-24（SiYuan 三进程卡 NVMe 页错误）、2026-10-25（云盘 FUSE 守护挂死
——rclone mount / WebDAV / qrypt 之类——`find` 卡 13.5 小时）。详见
`docs/notes/suspend-freeze-deadlock.md`。

## 判定逻辑

1. 采样所有 D 状态**用户态**进程（内核线程 cmdline 为空，跳过）；
2. `RECHECK_SEC=2` 秒后二次采样，取 PID 交集 = 持续卡死（瞬时 IO 不误伤）；
3. 干净时放行仅耗一次 `ps`（毫秒级），不影响正常挂起速度。

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
/usr/local/sbin/suspend-guard            # 干净路径: PASS, exit=0
SUSPEND_GUARD_DEMO=1 /usr/local/sbin/suspend-guard   # 演示拦截: 弹窗 + exit=1
journalctl -u systemd-suspend.service -n 5           # 真实拦截后查点名名单
```

## 参数（脚本头部）

| 变量 | 默认 | 说明 |
|---|---|---|
| `NOTIFY_USER` / `NOTIFY_UID` | banshan / 1000 | 弹窗接收者 |
| `RECHECK_SEC` | 2 | D 状态持续判定窗口 |
| `RATE_LIMIT_SEC` | 90 | 弹窗限频（防 powerdevil 重试风暴刷屏） |

## 已知限制

- `-delete` / `-depth` 场景与本工具无关，但同理 `find -prune` 在 `-depth` 下失效（见笔记）。
- 守卫只拦"已卡死"的挂起；根因（云盘 FUSE 守护挂死、QLC 高负载读停顿）仍需单独治理：
  挂载点无响应时 `systemctl --user restart <对应的云盘 FUSE 服务>`。
- 卸载：删除 `/etc/systemd/system/systemd-*suspend*.service.d/guard.conf`（及 hibernate 对应项）
  与 `/usr/local/sbin/suspend-guard`，`daemon-reload`。
