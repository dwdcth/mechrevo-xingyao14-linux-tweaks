# 星耀14 休眠（Hibernate）踩坑实录与定稿方案

> 机型：MECHREVO 星耀14 XINGYAO P916F-STX（Ryzen AI 9 365）
> 环境：CachyOS，内核 7.2，btrfs + Limine 引导，Plasma 6.8
> 结论先行：**必须用 `HibernateMode=shutdown`，platform 模式在本机会砖机**。内核源码可证实两种路径的行为差异。

## 为什么 platform 模式会砖机（证据链）

本机只有 s2idle（无 S3 深睡），休眠走磁盘镜像 + 断电。`platform` 断电模式下，内核在写完镜像后、真正断电前会做**唤醒事件检查**，链条如下（全部有 journal 实证）：

1. 触控板中断经 pinctrl_amd（IRQ7）注册为唤醒源，休眠过程中累积 8000+ 次事件（DSDT 里触控板 GpioInt 带 `ExclusiveAndWake`，天生唤醒源）
2. 镜像写完 → 内核检测到唤醒事件 → `Wakeup event detected during hibernation, rolling back` → **镜像已写入但机器没断电**
3. I2C 总线（AMDI0010:01，触控板所在）此时持续 `lost arbitration`——主 EC（ITE IT5571，自带双通道 I2C 主机）与内核 designware 控制器抢总线，单次卡顿可达 82 秒
4. 多次中止后 PM 子系统卡死（后续 `echo disk > /sys/power/state` 全部秒拒 EBUSY）
5. **最危险状态**：镜像 100% 写入成功但回滚未断电 → swap 里残留不一致现场 → 重启时内核尝试恢复 → **卡死开不了机，需长按电源 30 秒清 EC 才能冷启动**

## 为什么 shutdown 模式安全（内核源码证据）

`kernel/power/hibernate.c` 的 `power_down()`：

- `HIBERNATION_PLATFORM` 路径：调用平台固件接口前有 wakeup-check，命中即回滚（上述砖机链）
- `HIBERNATION_SHUTDOWN` 路径：直接 `kernel_power_off()`，**没有任何唤醒检查**——镜像写完必然断电，不存在残留状态

## 定稿配置（逐步可复现）

### 1. btrfs 上的 swapfile（NOCOW 子卷）

swapfile 必须整块、无 CoW、无快照干扰，放独立子卷：

```bash
btrfs subvolume create /@swap            # 挂载点如 /swap（按你的子卷布局调整）
mount /swap                              # fstab 加对应条目
chattr +C /swap                          # 禁 CoW（创建文件后再设无效，务必先设）
btrfs filesystem mkswapfile --size 20G /swap/swapfile
swapon /swap/swapfile
```

fstab 建议 `pri=10`，让 zram（pri=100）继续领跑日常换页：

```
/swap/swapfile  none  swap  defaults,pri=10  0  0
```

### 2. 计算恢复偏移并写入内核参数

```bash
btrfs inspect-internal map-swapfile -r /swap/swapfile   # 输出即 resume_offset
findmnt -no UUID /swap                                   # 所在分区 UUID
```

- **Limine**（CachyOS 默认）：编辑 `/etc/default/limine`，在 `KERNEL_CMDLINE[default]` 追加 `resume=UUID=<分区UUID> resume_offset=<偏移>`，然后 `sudo limine-update`
- grub 用户：`GRUB_CMDLINE_LINUX_DEFAULT` 加同样参数 + `grub-mkconfig -o /boot/grub/grub.cfg`
- mkinitcpio 带 `systemd` hook 即可原生支持恢复，零改动

### 3. 关键：shutdown 模式

`/etc/systemd/sleep.conf.d/hibernate.conf`：

```ini
[Sleep]
HibernateMode=shutdown
```

**勿改回 platform（会砖）。** 不要设置 HibernateDelaySec 之类的延迟衍生玩法。

### 4. 手动触发与验证

```bash
sudo systemctl hibernate
```

验证（恢复后查 journal）：

```bash
journalctl -b -1 -k | grep -iE 'hibernat|snapshot|swap'
# 健康特征：PM: hibernation entry → 写镜像 → Power down → （本次开机）resume 正常
# 病态特征（platform 模式）：Wakeup event detected / rolling back / Freezing of tasks failed
```

两个 boot 的时间应有一段「断电空窗」，说明真的断电了（不是 0 秒返回的假休眠）。

## 已放弃的方案：Mac 式合盖自动转休眠

尝试过 `suspend-then-hibernate`（合盖先睡、延迟后转休眠）。实测出现失控双休眠 + 锁屏密码循环（叠加 Plasma 已知 bug 375536/490924 家族），已全部拆除。**本机休眠定位为纯手动**（菜单/命令行触发），配置里不要留任何自动休眠路径。

## 快照保底

动休眠配置前先做 btrfs 快照（snapper 或 `btrfs subvolume snapshot -r`）；引导配置改动前备份 `/etc/default/limine`。恢复现场优先整包还原而非抽单文件。

## 相关

- 触控板 I2C 仲裁的完整根因：[touchpad-i2c.md](touchpad-i2c.md)
- 本机 EC / ACPI 素材：[../firmware/](../firmware/)
