# UWACPIDriver.sys 电池限充（养护模式）静态逆向分析报告

- 分析对象：`UWACPIDriver.sys`（49,096 字节，PE32+ x86-64，KMDF 1.15 内核驱动）
- 驱动版本：DriverVer 08/27/2024, 11.2.18.347（Uniwill Technology Inc. 出品，TONGFANG HONGKONG LIMITED 代码签名）
- PDB 路径：`F:\UWACPIDriver2\x64\Release\UWACPIDriver.pdb`
- 分析方式：radare2 纯静态分析（`aaa` 全量分析 + 逐函数反汇编），未在任何环境下执行该 PE
- 交叉参考：本机 `dsdt.dsl`（31 个 ACPI 表全量）、主线内核 `drivers/platform/x86/uniwill/uniwill-acpi.c`、开源 `pobrn/qc71_laptop`

---

## 1. 核心结论（先说答案）

**电池限充 = 通过 ACPI 方法 `ECRW` 写 EC RAM 16 位地址 `0x07B9`（CHARGE_CTRL 寄存器）：**

```
低 7 位 (mask 0x7F)：充电阈值百分比 1–100（写 0 = 关闭限充，读回 0 按 100 处理）
bit 7   (0x80)：    "阈值已到达"状态位，写入时必须保留原值（读-改-写）
```

- 设 80% 养护：`ECRW(0x07B9, (旧值 & 0x80) | 80)`
- 关闭养护：`ECRW(0x07B9, 旧值 & 0x80)`
- 读当前阈值：`ECRR(0x07B9) & 0x7F`

`ECRW(reg, val)` / `ECRR(reg)` 是 INOU0000 ACPI 设备命名空间内的控制方法，该 Windows 驱动把用户态 IOCTL 翻译成对它们的 `IOCTL_ACPI_EVAL_METHOD` 调用。

该结论由三方独立来源交叉证实（本驱动二进制 + Linux 主线 uniwill-acpi 驱动 + qc71_laptop 驱动），置信度：**极高**。

⚠️ 但有一个关键的平台性发现：**本机固件 ACPI 命名空间中没有 INOU0000 设备，也没有 ECRR/ECRW 等方法**（详见第 6 节）。

---

## 2. 驱动架构

| 组件 | 地址 | 说明 |
|---|---|---|
| DriverEntry | `0x140008000`（PAGE 段） | WdfDriverCreate → 分配 0x58 字节 ACPI_CONTEXT → 创建符号链接 `\DosDevices\ACPIDriver`（用户态经 `\\.\ACPIDriver` 打开）→ 建队列 |
| EvtDeviceAdd | `0x140008b08` | WDF_IO_QUEUE_CONFIG（DispatchSequential, Size=0x60），注册 EvtIoDeviceControl |
| **EvtIoDeviceControl** | `0x140002870` | IOCTL 分发（约 2100 字节，二分跳表） |
| WDF DDI 跳板 | `0x140003xxx` 系列 | 驱动无静态 WDF 导入，经 `0x140006358` 处函数表按索引调用（KMDF 运行时绑定），如 `fcn.140003298`=WdfIoTargetSendIoctlSynchronously、`fcn.1400031b0`=WdfDeviceGetIoTarget、`fcn.1400033bc/140003420`=WdfRequestRetrieveInput/OutputBuffer |
| ECRR worker | `0x140002130` | 内嵌方法名 "ECRR"，1 个整数参数 |
| ECRW worker | `0x1400025b8` | 内嵌方法名 "ECRW"，2 个参数 |
| SMRW worker | `0x140002370` | 内嵌方法名 "SMRW"，1 个缓冲区参数 |
| 命名 worker ×3 | `0x140001784`(1×int) / `0x140001a98`(2×int) / `0x140001dc0`(3×int) | 方法名由调用方传入 |

**重要：驱动本体不含任何 I/O 端口指令、不含 MMIO 映射、不含内嵌 AML。** 全文件唯一的 `out dx, al`（0x140007124）位于附加签名的证书数据区，不是代码。ntoskrnl 导入仅 5 个无害函数（RtlInitUnicodeString 等）。所有 EC 访问 100% 经由 ACPI 方法求值。

---

## 3. 全部 IOCTL 一览

格式：`CTL_CODE(DeviceType=0x9C40(40000), Function, METHOD_BUFFERED, FILE_WRITE_ACCESS)`
即 `code = 0x9C40A000 + Function*4`（Function 0x920–0x941，中间有空缺）。

输入参数均为 IOCTL 输入缓冲区里的**原样小端 DWORD**（每 4 字节一个整数参数）；方法名经大端拼装成 NameSeg。

| IOCTL 码 | Function | ACPI 方法 | 参数（来自输入缓冲区） | 用途（/为推断） |
|---|---|---|---|---|
| 0x9C40A480 | 0x920 | SMCR | 2×int: in[0..3], in[4..7] | SMC 读（/系统管理控制器） |
| 0x9C40A484 | 0x921 | SMCW | 3×int: in[0..3], in[4..7], in[8..11] | SMC 写 |
| **0x9C40A488** | **0x922** | **ECRR** | 1×int: in[0..3] = **EC 寄存器号（16 位有效）** | **读 EC RAM** |
| **0x9C40A48C** | **0x923** | **ECRW** | in[0..3] = **寄存器号**, in[4] = **写入字节** | **写 EC RAM** |
| 0x9C40A490 | 0x924 | BRMM | 1×int | Buffer 读 MMIO（/） |
| 0x9C40A494 | 0x925 | DRMM | 1×int | Dword 读 MMIO（/） |
| 0x9C40A498 | 0x926 | BWMM | 2×int | Buffer 写 MMIO（/） |
| 0x9C40A49C | 0x927 | DWMM | 2×int | Dword 写 MMIO（/） |
| 0x9C40A4A0 | 0x928 | DRCP | 1×int | 读 CP（/CMOS/PCI 配置） |
| 0x9C40A4A4 | 0x929 | DWCP | 2×int | 写 CP |
| 0x9C40A4C0 | 0x930 | DROI | 1×int | Dword 读 I/O 端口（/） |
| 0x9C40A4C4 | 0x931 | DWOI | 2×int | Dword 写 I/O 端口（/） |
| 0x9C40A4C8 | 0x932 | POIR | 2×int | Port IO 读（/） |
| 0x9C40A4CC | 0x933 | POIW | 3×int | Port IO 写（/） |
| 0x9C40A4D0 | 0x934 | DR1T | 1×int | 读寄存器 bank1（/） |
| 0x9C40A4D4 | 0x935 | DR2T | 1×int | 读 bank2（/） |
| 0x9C40A4D8 | 0x936 | DR3T | 1×int | 读 bank3（/） |
| 0x9C40A4DC | 0x937 | RW1T | 3×int | 读-改-写 bank1（/） |
| 0x9C40A4E0 | 0x938 | RW2T | 3×int | 读-改-写 bank2（/） |
| 0x9C40A4E4 | 0x939 | RW3T | 3×int | 读-改-写 bank3（/） |
| 0x9C40A500 | 0x940 | SMRW | 1×buffer: in[0..] | SMC 缓冲区命令（/） |
| **0x9C40A504** | **0x941** | **任意** | in[0..3]=方法名（ASCII），in[4..7]=LE DWORD 参数 | **通用任意方法求值逃逸口** |

### ACPI 求值细节（以 ECRW 为例，0x1400025b8）

1. 在栈上构造 `ACPI_EVAL_INPUT_BUFFER_COMPLEX`：
   - `[+0x00] = 'AeiC'`（0x43696541，ACPI_EVAL_INPUT_BUFFER_COMPLEX_SIGNATURE）
   - `[+0x04] = 'ECRW'`（MethodNameUlong，0x57524345）
   - `[+0x08] = 0x28`（Size）
   - `[+0x0C] = 2`（ArgumentCount）
   - Argument[0].Type=0(INTEGER), DataLength=4, Data=LE DWORD@in[0..3] → **reg**
   - Argument[1].Type=0(INTEGER), DataLength=4, Data=in[4] 零扩展 → **val**
2. `WdfIoTargetSendIoctlSynchronously(WdfDeviceGetIoTarget(dev), NULL, 0x32C004, ...)`：
   **0x32C004 = IOCTL_ACPI_EVAL_METHOD**（`CTL_CODE(FILE_DEVICE_ACPI=0x32, 0x1, METHOD_BUFFERED, FILE_READ|FILE_WRITE_ACCESS)`）
3. 校验输出 `ACPI_EVAL_OUTPUT_BUFFER`：Signature=='AeoB'（0x426f6541）
4. 方法返回 `ACPI_TYPE_BUFFER`，形如 `{u16 status; u16 length; u8 payload[length]}`（与 qc71_laptop 注释 "returned ACPI_TYPE_BUFFER is 40 bytes" 吻合）。驱动检查 `status==0`，把 `length` 字节 payload 拷入 IOCTL 输出缓冲区
5. 失败返回 `0xC014000F`（STATUS_ACPI_INVALID_REQUESTTYPE 一族）

---

## 4. EC 寄存器映射（自 Linux 主线 uniwill-acpi.c / qc71_laptop，与本驱动 ECRW/ECRR 通道对应）

ECRR/ECRW 访问的是 EC 的 **16 位 RAM 地址空间**（与 DSDT 的 256 字节 MMIO ERAM 窗口不是同一视图）：

| EC 地址 | 名称 | 说明 |
|---|---|---|
| 0x0400–0x04FF | 电池数据区 | 0x0402 设计容量、0x0404 满充容量、0x0432 电池状态、0x0436 剩余容量、0x0438 电压、0x0494 电池告警、0x04A6 循环次数 |
| 0x043E / 0x044F | CPU / GPU 温度 | |
| 0x0464 / 0x046C | 风扇 1 / 风扇 2 RPM（16 位） | |
| 0x0740 | PROJECT_ID | 机型项目号 |
| 0x0742 | SUPPORT_5 | bit4=风扇Turbo支持、bit5=风扇支持 |
| 0x0765 / 0x0766 | SUPPORT_1 / 2 | 功能支持位图（0x0766 bit6=MY_BATTERY 等） |
| **0x07A6** | **OEM_4** | **低 2 位 = 充电档案（模式机型用）：0=高容量/标准，1=均衡/长寿命，2=固定使用/慢充** |
| **0x07B9** | **CHARGE_CTRL** | **bit6..0 = 充电阈值百分比；bit7 = 阈值已到达（只读状态）** |
| 0x07C6 | AP_OEM_6 | bit3=24 小时充满、bit4=ERM 状态 |
| 0x0F00+ | 风扇温度/转速曲线表 | |
| 0x1804 / 0x1809 | PWM1 / PWM2（可写） | |

### 电池限充候选实现（按置信度排序）

**候选 1（置信度：极高，三方独立证实）—— EC 0x07B9 阈值百分比**
- 写：`ECRW(0x07B9, (ECRR(0x07B9) & 0x80) | pct)`，pct∈[1,100]；关闭时写 0（读回 0 上层按 100% 处理）
- 依据：
  - 本驱动：ECRR/ECRW 通道存在且为唯一 EC 出口（本文第 2、3 节）
  - Linux 主线 `uniwill-acpi.c`：`POWER_SUPPLY_PROP_CHARGE_CONTROL_END_THRESHOLD` 的 get/set 即 `regmap_update_bits(0x07B9, GENMASK(6,0), max(val,1))`
  - `pobrn/qc71_laptop`：`BATT_CHARGE_CTRL_ADDR = ADDR(0x07,0xB9)`，`VALUE_MASK=GENMASK(6,0)`，`REACHED=BIT(7)`，`charge_control_end_threshold` sysfs 读写完全同构

**候选 2（置信度：高，仅适用于"模式型"机型）—— EC 0x07A6 充电档案**
- 写：`ECRW(0x07A6, (旧 & ~3) | mode)`，mode：0=标准/1=长寿命(≈80%养护)/2=慢充
- 主线驱动对不支持阈值接口的机型改暴露 `charge_types`（Standard/Long Life/Trickle）。若本机 EC 固件不支持 0x07B9 阈值，Windows Control Center 的"养护模式"开关即走此路径

**候选 3（置信度：已排除）—— WMI WMAA / burst ECMD / MMIO ERAM**
- WMAA（MFID=3）子功能仅含 LED 灯效(G/SFLS,G/SCLS)、Turbo(G/STPS)、性能模式(G/SPFM)、键盘背光(G/SKBT)，无电池项——证实"限充不走 WMI"
- DSDT burst `ECMD(cmd)` 全表仅出现 4 个命令码：0x90/0x8F（Turbo 开/关）、0x94/0x95（性能模式 1/2），无电池命令
- 驱动二进制内无端口 I/O、无 MMIO 0xFEEC2300 常量、无 0x9x/0xDx/0xEx 之类新 EC 命令码立即数

---

## 5. 与本机 DSDT 的交叉验证

| DSDT 内容 | 与驱动的关系 |
|---|---|
| `EC0._CRS`: IO 0x62/0x66；`IOPT` 区: EDAT@0x62、EADD@0x66 | 驱动不使用（无端口 IO）。这是 ACPI 侧单字节 burst 命令接口（WIBE=等 IBF 空、WOBE=等 OBF 满并读） |
| `ECMD(cmd)`（3668 行） | 仅 0x90/0x8F/0x94/0x95 四个调用点；ECRR/ECRW 与此无关 |
| `ERAM` MMIO 0xFEEC2300（256B） | 0x32 TPST、0x33 KBBL、0x35 KBST、0x3B/0x3D FNS0/FNS1、0x3F FTVL、0x80+ 电池数据——与已知一致；**不含 0x07B9**（另一地址空间） |
| WMAA（约 4530 行） | 无电池子功能，证实用户判断 |
| 电池设备 LCBT（PNP0C0A） | 控制方法电池，当前 Linux 下即 `/sys/class/power_supply/LCBT` |

---

## 6. 关键异常：本机固件没有 INOU0000！

已穷尽验证（均可复现）：

```bash
grep -rin "INOU" 项目目录/*.dsl        # 无
for f in *.aml; do grep -qa "INOU" $f; done   # 原始 AML 二进制也无
ls /sys/bus/acpi/devices/ | grep -i inou     # 无（实时系统）
ls /sys/firmware/acpi/tables/                # 31 表齐全（DSDT+SSDT1-30），dynamic/tables 为空
grep -c "Load (\|LoadTable" dsdt.dsl         # 0（无动态表加载）
```

驱动二进制内亦无内嵌 SSDT（无 "SSDT" 签名字节、无 AML `Device()`(0x5B82)/`Scope()`(0x5B10) 操作码、无 `AcpiLoadTable`/`Zw*` 解析）。

**结论与推论：**
1. ECRR/ECRW/SMCR… 方法与 INOU0000 设备必须在固件 ACPI 表里由 OEM SSDT 提供（QC71/AC71H 等 Uniwill 公模确实内置，主线 uniwill-acpi 驱动即按 `{"INOU0000"}` 匹配）。
2. 因此本 INF（绑定 `ACPI\INOU0000`）在**当前这份固件转储**对应的机器上不可能枚举出设备、驱动不会加载、`\\.\ACPIDriver` 不存在。
3. 可能的解释（按可能性排序）：
   - 本机 BIOS 版本/机型实际不使用该通路（Control Center 的养护开关走 BIOS 设置或其它服务），UWACPIDriver 只是预装残留；
   - Windows 侧存在我们未发现的表注入组件（如另一驱动加载 SSDT）；
   - 用户转储 ACPI 时的 BIOS 版本与 Windows 日常使用时不同（更新过）。
4. **建议在 Windows 侧一锤定音**：`pnputil /enum-devices | findstr /i INOU`、设备管理器查看隐藏设备、以及 Control Center 里养护开关是否真的可用。

---

## 7. Linux 下的安全验证方案（只读优先）

### 7.1 立即可做（零风险，只读）

```bash
# 1) 确认 INOU 缺失（预期无输出）
ls /sys/bus/acpi/devices/ | grep -i inou

# 2) 主线驱动是否存在/可加载（本机预期 probe 失败：无 INOU0000）
modinfo uniwill_acpi 2>/dev/null; dmesg | grep -i uniwill

# 3) 观察 DSDT 已知窗口（只读 MMIO；确认映射存在）
sudo busybox devmem 0xFEEC2300   # 或 cat /dev/mem 借助 devmem2 扫 0x00-0xFF
# 期望：+0x32 TPST、+0x3F FTVL 等与已知一致 → 证明 EC 视图正确

# 4) 充电状态基线记录（写养护前先记档）
grep -H . /sys/class/power_supply/LCBT/{status,capacity,voltage_now,current_now} 2>/dev/null
```

### 7.2 判定"限充是否存于 EC"（跨系统实验，仍然只读）

EC 阈值写入后**持久保存在 EC/其 RAM 中，跨重启、跨 OS 生效**：

1. 进 Windows，用 Control Center（若可用）设 80% 养护；
2. 回 Linux，插电观察：`watch -n30 cat /sys/class/power_supply/LCBT/{status,capacity}`；
3. 若电量到 80% 附近变为 `Not charging`/`Full` 且电流≈0 → 证明限充逻辑在 EC 固件内，与驱动/OS 无关；
4. 反之若 Windows 里 Control Center 根本无法开启养护（开关灰掉/报错），则印证第 6 节"本机无 INOU 通路"，养护只能依赖 BIOS Setup 选项。

### 7.3 若将来需要直接读写 0x07B9（有风险，谨慎）

- 标准 EC 接口（`ec_sys`/端口 0x62,0x66）只覆盖 256 字节页，**到不了 0x07B9**；
- 16 位 EC RAM 访问依赖厂商私有协议：QC71 走 WMI `WMBC`，INOU 机型走 ECRW/ECRR —— 本机两者皆无；
- 可行路线 A（推荐）：等/刷含 INOU SSDT 的 BIOS，然后用主线 `uniwill_acpi`（未来并入的 uniwill-laptop 完整功能）直接得到 `charge_control_end_threshold`；
- 可行路线 B（高级）：用 `acpi_configfs` 手工挂载自制 SSDT 定义 INOU0000+ECRR/ECRW（burst 协议需从其他 Uniwill 机器固件移植），先只实现 ECRR 只读验证 0x07B9 数值合理性（1–100 或 0），再考虑写；
- **不要**向 0x66/0x62 盲发未知 burst 命令码（可能触发 EC SMI 动作）；**不要**用 inb/outb 工具写 EC。

### 7.4 与本报告结论等价的快速复核（他山之石）

```bash
# 主线驱动源码（含全部 EC 寄存器与 ECRW/ECRR 语义）
curl -s https://raw.githubusercontent.com/torvalds/linux/master/drivers/platform/x86/uniwill/uniwill-acpi.c | less
# 关键词：EC_ADDR_CHARGE_CTRL 0x07B9、uniwill_ec_reg_write、ECRW、ECRR

# qc71 参考（WMI 版 16 位 EC 访问 + 电池阈值 sysfs）
git clone https://github.com/pobrn/qc71_laptop
```

---

## 8. 附：驱动字符串与结构证据

- `0x140005120` 附近：WDF 上下文 GUID `{E8695F63-1831-4870-A8CF-9C2F03F9DCB5}` + "ACPI_CONTEXT"
- `0x140008B80`: `"\DosDevices\ACPIDriver"`（UTF-16，符号链接）
- `0x1400050F5` 附近: `"DriverEntry failed 0x%x for driver %wZ\n"`
- 方法名常量表：`0x140004290`"SMCR" … `0x1400043A0`"RW3T"（.rdata，连续存放）
- 结构签名：`'AeiC'`×6、`'ECRR'`、`'ECRW'`、`'SMRW'`、`'AeoB'`（输出校验）
- IOCTL 立即数：0x32C004 ×6（ACPI eval），0x9C40A480…0x9C40A504 ×22（分发）

## 9. 参考来源

- 本机 DSDT/SSDT 反编译（firmware/acpi/*.dsl（本仓库））
- Linux 主线驱动：<https://git.kernel.org/…/drivers/platform/x86/uniwill/uniwill-acpi.c>（GitHub 镜像 torvalds/linux）
- uniwill-laptop 内核文档：<https://docs.kernel.org/admin-guide/laptops/uniwill-laptop.html>
- qc71_laptop（QC71 公模 EC 访问与电池阈值）：<https://github.com/pobrn/qc71_laptop>
- LWN 补丁系列 "Add support for Uniwill laptop features"：<https://lwn.net/Articles/1032291>
- Treexy 驱动库中 INOU0000 设备记录：<https://treexy.com/products/driver-fusion/database/system/uniwill/acpidriver>
