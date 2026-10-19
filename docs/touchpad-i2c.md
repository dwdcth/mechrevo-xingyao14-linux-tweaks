# 触控板 I2C "lost arbitration" 根因分析

> 机型：星耀14 P916F-STX，触控板 BLTP7853（I2C-HID），CachyOS 内核 7.2
> 现象：dmesg 周期性刷 `i2c_designware AMDI0010:01: controller timed out` / `lost arbitration`；休眠时成为砖机链条一环（见 [hibernation.md](hibernation.md)）

## 结论：不是触控板故障，是 AMD 平台固件设计通病，三层叠加

### 1. 电气层：EC 与内核抢 I²C 总线

触控板 BLTP7853 挂在 `AMDI0010:01`（designware I²C 控制器）上；主 EC（ITE IT5571，逆向确认自带双通道 SMBus/I²C 主机）在**同一电气域**发起自己的事务（电池轮询等）。I²C 是多主机总线，两边同时发起即仲裁失败——内核侧表现为 `lost arbitration`，驱动自动重试，单次卡顿最长观测到 82 秒。

同类案例：CachyOS 论坛 Lenovo Yoga 7（AMD）相同报错；内核 bugzilla #221494 在跟进（EC/ACPI 电源域与 i2c 控制器竞态）。

### 2. 设计层：触控板中断天生是唤醒源

DSDT 里触控板 GpioInt 带 `ExclusiveAndWake`，中断路由到 pinctrl_amd（IRQ7）。休眠期间触控板的任何静电/微动都会被计为唤醒事件（一次实验累积 8727 次）——这是 platform 模式休眠回滚的直接导火索。

### 3. 编译层：改模块参数的路被堵死

CachyOS 内核 `CONFIG_I2C_DESIGNWARE_PLATFORM=y` 编译进内核（非模块），社区流传的「改成模块 + 开机重置控制器」方案不可行。论坛方案里 `$dev` 寄存器复位时序对 EC 在场的主板也未必安全。

## 日常影响与休眠影响

- **日常：零影响**。驱动自动重试，肉眼无感；不建议也没必要修
- **休眠：致命**。platform 模式下唤醒事件掐死断电流程 → 镜像残留 → 砖机。**解药是 `HibernateMode=shutdown`**（跳过全部唤醒检查，见 [hibernation.md](hibernation.md)）

## 为什么 Windows 没事

Windows 的 amdi2c 驱动对 EC 竞争无特殊时序配置，大概率同样撞车后静默重试，只是不往日志里刷。属于「没人看的墙缝」而非「已修复」。

## 素材

- DSDT 中触控板 GpioInt 定义：[../firmware/acpi/dsdt.dsl](../firmware/acpi/dsdt.dsl)
- EC（IT5571）能力考证：[hardware-map.md](hardware-map.md)
