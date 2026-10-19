# 机械革命星耀14（Uniwill/AIstone P916F）驱动仓库分析报告

- 分析日期：2026-10-17
- 对象仓库：
  - `refs/（本地克隆，见 README 链接）tuxedo-drivers/`（TUXEDOComputers/tuxedo-drivers，commit `a929f9c`，v4.25.0，2026-10-06）
  - `refs/（本地克隆，见 README 链接）uniwill-laptop/`（Wer-Wolf/uniwill-laptop，commit `5a24248`，2026-08-29）
- **仓库来源说明**：任务指定的 `pobrn/uniwill-wmi` 与 `thesofproject/uniwill-wmi` 在 GitHub 上均已 404 删除（GitHub API 确认，pobrn 名下现存的是 `pobrn/qc71_laptop`）。两者的实际继承项目是 **Wer-Wolf/uniwill-laptop**：其源码致谢 "Pőcze Barnabás"(=pobrn)，README 明言 "based on qc71_laptop and tuxedo-drivers"，且该驱动已随内核 6.19 进入主线（本仓库为实验分支，要求内核 ≥ 7.2，正好匹配本机 7.2.10-cachyos 的"内核自带 uniwill-laptop.ko"）。该仓库包含 `uniwill-wmi.c`/`uniwill-acpi.c`，即任务所指项目的全部内容，故以其代替。

---

## 0. 一句话结论

| 仓库 | EC 访问方式 | 对本机可用性 |
|---|---|---|
| tuxedo-drivers (uniwill) | ① WMI `ABBC0F6F` method 0x04（默认）② **ACPI EC 0x62/66 → EC 寄存器 0x8A~0x8E "Uniwill EC-RAM 桥"直通**（`ec_direct_io=1`） | ①不可用（无该 GUID）②**极可能可用，是最有价值的复用代码** |
| tuxedo-drivers (nb05) | **裸端口 I/O 0x4E/0x4F + ITE I2EC 间接寄存器** | 端口布局不同（本机在试 0x680），但协议时序可完全照抄 |
| uniwill-laptop (Wer-Wolf) | **纯 ACPI：INOU0000 设备下的 `ECRR`/`ECRW` 方法** + regmap | 本机无 INOU0000 → 整体不可用；但 0x07B9 充电阈值逻辑是权威参考实现 |

两个仓库都**没有** ABBC0F5B/WMAA 布局的任何支持，也没有 IT5571 / 0x680 / H2RAM 的任何提及。

---

## 1. tuxedo-drivers：uniwill 相关驱动清单

README 列出的模块中与 Uniwill 相关的：

| 模块 | 文件 | 作用 |
|---|---|---|
| `uniwill_wmi.ko` | `src/uniwill_wmi.c`（423 行） | WMI 绑定层 + EC RAM 读写的两条通路 |
| `tuxedo_keyboard.ko` | `src/tuxedo_keyboard.c` + `src/uniwill_keyboard.h`（2339 行，uniwill 主体逻辑都在这个头文件里） | 平台设备、键盘灯、风扇、TDP、充电优先级/配置、电池钩子、i8042 滤镜 |
| `uniwill_interfaces.h` | `src/uniwill_interfaces.h` | 接口抽象：GUID 定义、EC 寄存器符号表、`uniwill_interface_t` |
| `tuxedo_io.ko` | `src/tuxedo_io/tuxedo_io.c` | 用户空间 ioctl（TCC 风扇/功耗控制用），底层调 `uniwill_read/write_ec_ram` |
| `ite_8291` / `ite_8291_lb` / `ite_8297` / `ite_829x` | `src/ite_829x/` 等 | ITE **键盘背光 USB 控制器**驱动（非 EC，与 IT5571 无关） |
| `tuxedo_compatibility_check.ko` | `src/tuxedo_compatibility_check/` | DMI/CPU 白名单门卫 |
| `tuxedo_nb05_ec.ko` | `src/tuxedo_nb05/tuxedo_nb05_ec.c` | **裸端口 I/O 的 EC 驱动**（非 Uniwill-WMI 体系，仅 NB05 板） |

### 1.1 绑定的 WMI GUID 与 DMI

`src/uniwill_interfaces.h:26-32`：

```c
#define UNIWILL_WMI_MGMT_GUID_BA  "ABBC0F6D-8EA1-11D1-00A0-C90629100000"
#define UNIWILL_WMI_MGMT_GUID_BB  "ABBC0F6E-8EA1-11D1-00A0-C90629100000"
#define UNIWILL_WMI_MGMT_GUID_BC  "ABBC0F6F-8EA1-11D1-00A0-C90629100000"
#define UNIWILL_WMI_EVENT_GUID_0  "ABBC0F70-8EA1-11D1-00A0-C90629100000"
#define UNIWILL_WMI_EVENT_GUID_1  "ABBC0F71-8EA1-11D1-00A0-C90629100000"
#define UNIWILL_WMI_EVENT_GUID_2  "ABBC0F72-8EA1-11D1-00A0-C90629100000"
```

（注：当前版本管理块是 0F6D/E/F；任务描述里的 0F6B/C/D 是旧版命名。）

`uniwill_wmi.c:322-349` probe 要求 **6 个 GUID 全部存在**，缺一返回 `-ENODEV`；wmi_driver 的 id_table 只挂事件 GUID_2 (ABBC0F72)。

DMI 不是 probe 的硬条件，而是**特性开关**：`uniwill_keyboard.h:1693` 起的 `uniwill_get_device_features()` 先读 EC `0x0740`（裸机/项目 ID：PF=0x09、PH4TUX1=0x13…），再用几十条 `dmi_match`（BOARD_NAME 如 `PHxARX1_PHxAQF1`、`GMxXGxx`、PRODUCT_SKU 如 `STELLARIS1XI03` 等）决定 profile/风扇/TDP 特性。

另有一道总门：`tuxedo_compatibility_check.c:222-230` `tuxedo_is_compatible()` 要求 DMI vendor 含 "TUXEDO"，或 CPU 在白名单（AMD family 25 选定型号为止，**family 26 / Strix Point 不在**）→ 本机 Ryzen AI 9 365 连这关也过不了，`tuxedo_keyboard` 模块 init 直接 `-ENODEV`。

### 1.2 EC 访问实现（重点）

分派层 `uniwill_keyboard.h:113-125/157-170`：`uniwill_read_ec_ram()/uniwill_write_ec_ram()` → `uniwill_interfaces.wmi->read_ec_ram/write_ec_ram`。

**通路 A：WMI（默认）** — `uniwill_wmi.c:56-141`
- `uw_wmi_ec_evaluate(function, arg, ret)`：`wmi_evaluate_method(ABBC0F6F, instance=0, method_id=0x04, in, out)`；
- 输入是 `u32[10]` 缓冲：byte0-3 = arg（`data_hi<<24|data_lo<<16|addr_hi<<8|addr_lo`），byte5 = function（0=写，1=读，5=特性切换）；
- 返回 ACPI buffer，第一个 u32 即数据；`0xfefefefe` = 错误。
- `uw_ec_read_addr_wmi()` :106、`uw_ec_write_addr_wmi()` :126。

**通路 B：ACPI EC 直通（`ec_direct_io=1` 模块参数，:419-420）** — `uniwill_wmi.c:146-272`
- `uw_ec_read_addr_direct()` / `uw_ec_write_addr_direct()` 使用**内核 `ec_read()/ec_write()`**（即 ACPI EC 设备，端口 0x62/0x66）访问 ACPI EC 自己地址空间里的 5 个寄存器（:29-39）：

```
0x8A LDAT  地址低字节      0x8C FLAGS 状态/控制
0x8B HDAT  地址高字节      0x8D CMDL  数据低字节
                            0x8E CMDH  数据高字节
FLAGS: bit0 RFLG(读触发) bit1 WFLG(写触发) bit2 BFLG(占用)
       bit3 CFLG          bit7 DRDY(就绪)
```

- 读：置 BFLG → 写 LDAT/HDAT → 清 DRDY 置 RFLG → 轮询 DRDY（30 次 × 15ms）→ 读 CMDL/CMDH → 清 FLAGS。
- 写：置 BFLG → 写 LDAT/HDAT/CMDL/CMDH → 置 WFLG → 轮询 DRDY。
- **这就是"不依赖 ABBC0F6x WMI 的 EC 访问路径"**：Uniwill 的 WMI method 0x04 在别的机型上正是 AML 对这五个寄存器的封装。**本机逆向的最优先动作：dump DSDT 中 WMAA(ABBC0F5B) 的 AML，看它是否操作 EC 的 0x8A-0x8E 桥**（或等价物）。若是，直接照抄这 130 行即可从 Linux 全量读写 16 位 EC RAM（0x04xx/0x07xx），完全不需要任何 WMI 管理 GUID。验证成本极低：用 ectool/自带小模块对 ACPI EC 读 0x8C 看 FLAGS 行为即可。

### 1.3 充电控制：tuxedo 没有 0x07B9

tuxedo 的 Uniwill 电池功能是两档"优先级/三档配置"，走 sysfs 组（不是标准 `charge_control_end_threshold`）：
- **charging_priority**：`uniwill_keyboard.h:574-599`，EC `0x07CC` bit7（充电优先/性能优先）；
- **charging_profile**：`:713-741`，EC `0x07A6` bit4-5（0=高容量 1=均衡 2=固定）；能力探测 `0x078E` bit3（`:743-770`）；
- 电池钩子 `:1289-1301` 只导出 `raw_cycle_count`(0x04A6/0x04A7)、`raw_xif1/xif2` 三个只读属性。
- grep 全树 `0x07b9/charge_control_end_threshold`：uniwill 侧**零命中**（只有 clevo_keyboard.h:775-912 的 Clevo 版本）。**0x07B9 的 charge_control_end_threshold 实现在 uniwill-laptop 仓库里（见 §2.3）**。

### 1.4 tuxedo_nb05_ec.c：裸端口 I2EC（grep 命中 I2EC 的唯一位置）

`src/tuxedo_nb05/tuxedo_nb05_ec.c:35-97`：

```c
#define EC_PORT_ADDR 0x4e          // 窗口索引口
#define EC_PORT_DATA 0x4f          // 窗口数据口
#define I2EC_REG_ADDR 0x2e         // 选择 I2EC 子寄存器的选择器
#define I2EC_REG_DATA 0x2f
#define I2EC_ADDR_LOW  0x10        // I2EC 子寄存器: 地址低
#define I2EC_ADDR_HIGH 0x11        // 地址高
#define I2EC_ADDR_DATA 0x12        // 数据
```

时序（`nb05_read_ec_ram` :59-77）：`outb(0x2e,0x4e); outb(0x11,0x4f)`（选 addr_hi）→ 写地址高 → 选 0x10 写地址低 → 选 0x12 读/写数据。EC 固件版本在 `0x0400/0x0401`（:99-104）——**与用户本机观察到的 0x04xx 页语义一致**。
DMI 仅 PULSE1403/PULSE1404/IFLX14I01（:145-177）。这是 tuxedo 树里唯一不经 WMI/ACPI 的 EC 通路；对本机的意义：**ITE EC 间接访问协议的参考实现**（若 0x680 窗口行为类似——先写选择器再读写数据口——可对照移植），但端口与寄存器布局需按本机实测调整。

---

## 2. uniwill-laptop（Wer-Wolf，= 任务所指 uniwill-wmi 项目的现状）

### 2.1 架构

单个 ko（`uniwill-laptop.o = uniwill-acpi.o + uniwill-wmi.o`），两个子驱动：

1. **`uniwill-wmi.c`（95 行）— 纯事件驱动**：绑定 WMI 设备 **ABBC0F72**，`notify` 收 u32 事件码后走 `blocking_notifier_chain` 分发（:47-62）。**不做任何 EC 访问**。:64-70 注释明确：该 GUID 是 Uniwill 从 Windows 示例驱动抄的、不可信，不能用于模块自动加载，只能 DMI 匹配后手动注册驱动。事件码表在 `uniwill-wmi.h`（OSD_CAPSLOCK=0x01 … FN_LOCK=0xB8、BAT_CHARGE_FULL_24_H=0xBE 等，可作为解析本机 ABBC0F5C 事件码的先验参考——但 WMAA 是新布局，码表未必一致）。

2. **`uniwill-acpi.c`（3386 行）— 主驱动**：`platform_driver` + ACPI ID 匹配（`uniwill_id_table` :2648-2651）：

```c
static const struct acpi_device_id uniwill_id_table[] = {
    { "INOU0000" },
};
```

   - DMI 表（:2863-3320）只做两件事：模块自动加载 + 为每机型附带 `uniwill_device_descriptor`（特性位图、键盘灯类型/亮度上限、可选 probe 回调）。
   - `force` 模块参数（:437-439，`module_param_unsafe`）可绕过 DMI，但即便 force 也**刻意不启用** `BATTERY_CHARGE_LIMIT`（:3351-3353），文档警告部分机型阈值接口实现有缺陷、强开可能损坏电池（引 XMG reddit 案例，`uniwill-laptop.rst` 尾注 [1]）。
   - DMI 表**第一条就是 AiStone**：`SYS_VENDOR="AiStone" + BOARD_NAME="X4SP4NAL"`（:2864-2871）→ 说明该驱动已覆盖一块 Aistone 公模（X4SP4NAL，AMD、白色单色键盘 2 档），是离本机最近的已支持机型。

### 2.2 EC 访问：ACPI ECRW / ECRR + regmap

`uniwill-acpi.c:524-593`：

```c
static int uniwill_ec_reg_write(void *context, unsigned int reg, unsigned int val)
{
    ...params = { ACPI_TYPE_INTEGER(reg), ACPI_TYPE_INTEGER(val) }...
    status = acpi_evaluate_object(data->handle, "ECRW", &input, NULL);   // :547
    usleep_range(UNIWILL_EC_DELAY_US, UNIWILL_EC_DELAY_US * 2);          // 6ms，模拟 OEM 行为 :346
}
static int uniwill_ec_reg_read(void *context, unsigned int reg, unsigned int *val)
{
    ...
    status = acpi_evaluate_integer(data->handle, "ECRR", &input, &output); // :574
    ...
}
static const struct regmap_bus uniwill_ec_bus = { .reg_write=..., .reg_read=... };
```

- `data->handle` = INOU0000 设备的 ACPI 句柄；**ECRR(addr)->byte、ECRW(addr,val)** 即 Uniwill BIOS 在 DSDT 里提供的官方 EC RAM 读写原语。
- 封装为 regmap：16 位寄存器/8 位值、`max_register=0xFFF`、maple 缓存、readable/writeable/volatile 白名单（:595-711）。
- probe（:2305-2379）：取 ACPI 句柄 → 建 regmap → `uniwill_ec_init`（:2286-2303）读 `0x0740` 项目 ID 并向 `0x0741` 置 `ENABLE_MANUAL_CTRL`(bit0) → 按特性初始化电池钩子/LED/键盘灯/hwmon/cTGP/USB-C 优先级/输入设备。
- 附注：`:330-338` 注释指出 `0x1804/0x1809`（PWM 写寄存器）"**only accessible when using the WMI interface**"——Uniwill 16 位 EC 空间存在权限分层，0x18xx 页 ECRW/ECRR 摸不到，只有 tuxedo 的 ABBC0F6F 通路可达。对本机逆向 0x07xx vs 0x18xx 页的可达性有参考意义。

### 2.3 charge_control_end_threshold（0x07B9）— 权威实现

定义 `uniwill-acpi.c:294-296`：

```c
#define EC_ADDR_CHARGE_CTRL 0x07B9
#define CHARGE_CTRL_MASK    GENMASK(6, 0)   // bit0-6: 阈值百分比
#define CHARGE_CTRL_REACHED BIT(7)          // bit7: 已达阈值指示
```

- **读**：`uniwill_get_property()` :1954-1961 — `regmap_read(0x07B9)` → `FIELD_GET(GENMASK(6,0))` → `uniwill_sanitize_battery_threshold()`（:1869-1877：0 视为"未激活"显示 100，>100 截断）。
- **写**：`uniwill_set_property()` :2013-2018 — 校验 0-100 后 `regmap_update_bits(0x07B9, GENMASK(6,0), max(val,1))`（保 bit7 不被破坏）。
- 挂接方式：`devm_battery_hook_register`（:2164-2168）扩展标准 power_supply，`charge_control_end_threshold` 属性即出现在 `/sys/class/power_supply/BAT*/`；另一批机型（互斥）用 `charge_types`（0x07A6 三档：Standard/Long Life/Trickle，:1879-1986）。
- 健壮性处理值得照抄：EC 上电会把阈值初始化为 0，probe 时读回并"洗净"（:2123-2141）；suspend 保存/resume 恢复（:2424-2435、:2546-2556，因 bit7 波动只能标 volatile 不能靠 regcache）；DC 插拔事件（0xAB）时重写（:2191-2209）。

### 2.4 其他功能与 EC 寄存器图（与本机 16 位空间对照）

hwmon：CPU 温 `0x043E`、GPU 温 `0x044F`、风扇 RPM `0x0464/0x046C`（hi/lo）；fn_lock `0x074E` bit4；键盘灯 `0x078C`+RGB `0x0769-0x076B`+trigger `0x0767`；lightbar `0x0748-0x074B`/`0x07E2-0x07E5`；cTGP `0x0743-0x0746`；USB-C 优先 `0x07CC`；风扇曲线表 `0x0F00-0x0F5F`。与用户已知的 0x04xx 电池页（0x0402 设计容量等）完全同族。

### 2.5 对"新 Uniwill 布局（ABBC0F5B/WMAA）"的支持与路线图

**没有**。全仓库（代码+README+rst+sysfs 文档）grep `ABBC0F5B/ABBC0F5/WMAA/0x680/H2RAM/IT5571` 零命中；EC 访问硬依赖 INOU0000.ECRW/ECRR，事件硬依赖 ABBC0F72。README/rst 中无任何关于新布局、ITE 芯片型号、I2EC 端口的讨论或 roadmap。（作者 Armin Wolf 是内核平台驱动评审者，主线上游化走 DMI+INOU0000 路线，可向其邮件列表提交新机型 DMI 条目，但前提是机器有 INOU0000。）

---

## 3. 关键词检索汇总（两仓库）

| 关键词 | tuxedo-drivers | uniwill-laptop |
|---|---|---|
| `I2EC` | `tuxedo_nb05_ec.c:38-43`（定义）、`:66-93`（读写时序），端口 0x4E/0x4F | 无 |
| `0x680` | 无 | 无 |
| `H2RAM` | 无 | 无 |
| `IT5571` | 无（`ite_829x` 等是 USB 键盘灯控制器，非 EC） | 无 |
| `ECRR` | 无 | `uniwill-acpi.c:574`（`acpi_evaluate_integer(handle,"ECRR",addr)`） |
| `ECRW` | 无 | `uniwill-acpi.c:547`（`acpi_evaluate_object(handle,"ECRW",addr,val)`） |

---

## 4. 对本机（无 INOU0000、无 ABBC0F6x、仅 ABBC0F5B/WMAA+ABBC0F5C）的可复用代码路径

按价值排序：

1. **`tuxedo-drivers/src/uniwill_wmi.c:146-272`（`uw_ec_read_addr_direct`/`uw_ec_write_addr_direct`）**
   走内核 `ec_read/ec_write`（ACPI EC 0x62/66）操作 EC 偏移 0x8A-0x8E 的 Uniwill EC-RAM 桥，零 WMI 依赖、零新驱动框架，130 行可整体搬走。前提是本机 EC 固件同样实现该桥（Uniwill/AIstone 传统布局，大概率存在）。**验证顺序：① ectool 读 ACPI EC 0x8C 观察 FLAGS；② 反编译 DSDT 看 WMAA 的 AML 是否就是 0x8A-0x8E 序列（或改用了别的桥寄存器/0x680 窗口）。**
2. **`uniwill-laptop/uniwill-acpi.c:1869-2168`（0x07B9 充电阈值全套）**
   位定义、sanitize、power_supply_ext 挂接、suspend/resume/插拔恢复，是 0x07B9 的完整语义参考；换掉底层 regmap_bus 即可在任意通路复用。
3. **`uniwill-laptop/uniwill-acpi.c:524-711`（regmap 框架）**
   把 ECRW/ECRR 替换成任意后端（ACPI 方法/端口 I/O/I2EC）即可继承缓存、volatile、PM 同步等全部基础设施——自定义驱动的最佳骨架。
4. **`tuxedo-drivers/src/tuxedo_nb05/tuxedo_nb05_ec.c:35-97`（I2EC 间接访问）**
   ITE EC 主机接口协议参考；若本机 0x680 窗口验证可用，按此时序封装 read/write_ec_ram 即可。
5. **`uniwill-laptop/uniwill-wmi.h`（OSD 事件码表）+ `uniwill_keyboard.h:300-330`（事件处理）**
   解析本机 ABBC0F5C 事件码的对照表起点。

## 5. "让 tuxedo 驱动在本机工作"补丁评估

结论：**不值得直接 patch tuxedo；更优路径是 fork uniwill-laptop 或写最小自定义驱动**。具体：

改 tuxedo 需要动三处：
1. `tuxedo_compatibility_check.c`：DMI 白名单加本机（或 CPU 表加 family 26），否则模块 init 即 `-ENODEV`；
2. `uniwill_wmi.c:331-342`：probe 的 6-GUID 检查在本机必然失败；需新增一个不经 WMI 的接口实现（把 `uw_ec_read/write_addr_direct` 直接挂进 `uniwill_interface_t` 并模拟 probe 成功）；
3. `uniwill_keyboard.h` 特性 DMI 表加 XINGYAO 条目，否则即便跑起来也是全特性关闭（features 检测靠 0x0740 + DMI）。
工作量中等，而收益有限：tuxedo 的 uniwill 侧**没有** 0x07B9 阈值（只有 0x07A6 三档 + 0x07CC 优先级），其价值主要在风扇控制（0x0751/0x1804/0x1809，其中 0x18xx 页可能仍需 WMI 权限）和键盘灯。

推荐方案（结合两仓库之长）：
- **方案 A（最小可用）**：写 ~200 行独立驱动或 fork uniwill-laptop：regmap 骨架（抄 §4.3）+ 后端先试 ACPI EC 0x8A-0x8E 桥（抄 §4.1），失败再试 0x680 I2EC（抄 §4.4 时序）；上层只接 `charge_control_end_threshold`（抄 §4.2）。事件侧另外绑定 ABBC0F5C 自行解析。
- **方案 B（上游化）**：若 DSDT 里找到任何对象提供 ECRW/ECRR 等价方法，给 Wer-Wolf/uniwill-laptop 提 DMI+绑定补丁是长期正解；其 force 参数路径也可先做快速试验（但注意 force 不开充电阈值，且本机无 INOU0000，需改 `_HID` 匹配）。
