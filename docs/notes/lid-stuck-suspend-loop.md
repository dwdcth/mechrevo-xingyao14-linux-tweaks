# EC 盖子状态卡死 → logind 30 秒重挂起循环（2026-11-04）

体感"反复注销"：开机约 30 秒后开始，每半分钟被黑屏打断一次；图形会话登录不进去
（登录界面 compositor 被首挂打残）；tty 控制台也被反复挂起打断。EC 状态位卡死
可跨温重启存活，直到自行恢复或断电复位。

## 事故链

```
(前情: 一次冻死强制断电后, EC 盖子状态位卡在 closed——物理上是开盖的)
logind 启动: 从 /dev/input/event0 读到 SW_LID 初始值 = closed
  → 全程没有任何 "Lid closed" 事件(卡死是初值, 不是翻转)
  → ~30s 后 HoldoffTimeoutSeconds 到点, 发起挂起(HandleLidSwitch=suspend)
  → s2idle 又被 pending 的 IRQ1/IRQ7 几秒内唤醒
  → 唤醒后 30s 复查 lid 仍"闭合" → 再挂起 …… 无限循环
  → 11-04 21:26 EC 自愈发出 "Lid opened." 循环才停(历时 12 分钟)
```

本次 16 轮循环的唤醒方式大部分是用户按电源键（journal 里一串
`Power key pressed short`），曾误导排查方向。

## 为什么各种表面解释都不成立

- **不是谁在注销**：logind 一路 `Suspending...` 主动发起，节奏精确 30s——
  这是 logind 唤醒后复查盖子的 holdoff 特征，不是任何桌面组件的行为。
- **不是电源键触发**：`HandlePowerKey=poweroff` 默认却从未关机 → 那些"短按"
  都发生在睡眠中（叫醒机器），不是挂起发起者。
- **不是盖子传感器误报"开"**：`/proc/acpi/button/lid` 里就是 closed，与 logind
  一致——错的是 EC 上报本身。

## 排查探针（下次直接用）

```bash
journalctl -b -u systemd-logind | grep -aE 'Lid|Susp|Power key'
# 签名: 没有任何 "Lid closed" 事件, 却有一串 Suspending... = 初始值卡死
cat /proc/acpi/button/lid/LID*/state     # 当前 EC 上报的盖子状态
```

## 处置

1. **根治：关机并断电 ≥30 秒**（EC 掉电复位）。温重启不清 EC——11-04 的坏状态
   就这样跨了一轮重启（boot -1 强关 → boot 0 开机 30 秒后照旧循环）。
2. **保险丝**：suspend-guard 检查 1——lid=闭合但本 boot 最后一条 lid 事件不是
   `Lid closed.` → 拒绝挂起 + 弹窗指引断电复位。
3. **临时绕过**：`touch /run/suspend-guard/skip-lid`（tmpfs，重启自清）。
4. **连带伤**：开机 30s 内的首挂会把 plasmalogin greeter 的 kwin 打成
   `Failed to create framebuffer` + placeholder 永久黑屏（eDP connector 本身是
   connected，硬件没坏），`systemctl restart plasmalogin` 即恢复；保险丝拦下
   首挂后此连带伤从源头消失。

## 遗留疑点

入睡瞬间 `PM: Triggering wakeup from IRQ 7 / IRQ 1`（pending 唤醒中断导致
秒醒）与 mechrevo-osd 过滤的 input8 幽灵键扫是否同源，待正常挂起窗口取证。
