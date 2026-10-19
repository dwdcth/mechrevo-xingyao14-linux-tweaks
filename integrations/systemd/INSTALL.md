# systemd 单元安装

五个部署物，三个主题：FN 键 OSD、充电阈值恢复、锁屏指纹配套。**安装前提：`/usr/local/bin/mechrevo-ctl` 已就位**（[../../tools/mechrevo-ctl/INSTALL.md](../../tools/mechrevo-ctl/INSTALL.md)）。

## 文件去向

| 仓库文件 | 目标路径 | 用途 |
|---|---|---|
| `mechrevo-osd.service` | `/etc/systemd/system/` | FN 键 OSD 守护（触控板/键盘灯/麦克风/飞行模式/性能模式/刷新率） |
| `mechrevo-charge.service` | `/etc/systemd/system/` | 开机幂等恢复充电阈值 80/100（电池耗尽断电 EC 会清零） |
| `kde-lock-raise.service` | `/etc/systemd/system/` | 挂起恢复后自动抬升锁屏密码框（指纹配套） |
| `system-sleep/fprintd-release.sh` | `/usr/lib/systemd/system-sleep/` | 挂起前 stop fprintd 释放指纹器（s2idle 机型 S0ix 必需） |

```bash
sudo install -Dm644 mechrevo-osd.service mechrevo-charge.service kde-lock-raise.service /etc/systemd/system/
sudo install -Dm755 system-sleep/fprintd-release.sh /usr/lib/systemd/system-sleep/fprintd-release.sh
sudo systemctl daemon-reload
sudo systemctl enable --now mechrevo-osd mechrevo-charge kde-lock-raise
```

## 需按本机适配的三处（`mechrevo-osd.service`）

1. `User=` / `Group=`：改成你的桌面用户（守护要连该用户的 session DBus 发 OSD/调 kscreen）
2. `DBUS_SESSION_BUS_ADDRESS` 的 uid：`/run/user/<UID>/bus`
3. `XDG_RUNTIME_DIR`：`/run/user/<UID>`

权限说明：`CAP_SYSLOG` 为读 `/dev/kmsg` 抓 WMI 事件（`dmesg_restrict=1` 环境）；`CAP_NET_ADMIN` 为 rfkill 无线电开关。dbus-broker 拒绝异 uid 连用户总线，所以必须以该用户身份跑而非 root。

## 验证

```bash
systemctl status mechrevo-osd mechrevo-charge   # active (running) / exited
sudo mechrevo-ctl charge status                 # T2=100 T1=80 之类的期望值
journalctl -u mechrevo-osd -f                   # 按 Fn 键应看到事件码日志
```

挂起恢复链验证：`systemctl suspend` → 唤醒 → 锁屏密码框自动出现（kde-lock-raise）+ 指纹可摸（fprintd 已被 pre-suspend 释放又自动重启）。

## 卸载

```bash
sudo systemctl disable --now mechrevo-osd mechrevo-charge kde-lock-raise
sudo rm /etc/systemd/system/{mechrevo-osd,mechrevo-charge,kde-lock-raise}.service /usr/lib/systemd/system-sleep/fprintd-release.sh
sudo systemctl daemon-reload
```
