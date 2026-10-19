# IT5570/IT5571 I2EC 接口启用条件研究

日期：2026-10-17
对象：机械革命星耀14（Uniwill/AIstone 公模 P916F-STX，IT5571 rev 0x07，PNPCFG @ 0x4E/0x4F，ChipID 0x5571）
现象：0x680+1/+2/+3 端口写入地址后读数据口内容不随地址变化（"固定窗口"）

---

## 0. 结论摘要（TL;DR）

1. **0x680–0x683 上读到的"固定窗口"根本不是 I2EC，而是 RTCT。**
   PNPCFG LDN 0x10 是 RTC-like Timer（RTCT），不是 I2EC 逻辑设备。LDN 0x10 的
   IOAD0/IOAD1 = 0x680/0x682 对应 RTCT 的两对 index/data 端口
   （legacy 70/71h 与 72/73h 的等价物）：`0x680`=bank0 index、`0x681`=bank0 data、
   `0x682`=bank1 index、`0x683`=bank1 data。往 0x681/0x682 写"EC 地址"实际在操作
   RTCT 的 192B SRAM 索引，读 0x683 返回的是 RTCT SRAM/时间寄存器内容——内容固定、
   与 EC 16 位地址无关，与观察完全吻合。**IT5570 的 PNPCFG 里没有任何 I2EC 使能位，
   I2EC 也不占用任何 LDN。**

2. **I2EC 的总开关在 EC 侧 GCTRL 寄存器，不在 PNPCFG**：
   `XRAM[0x200D] SPCTRL1.I2ECCTRL[1:0]`（00=disabled / 10=read-only / 11=read-write）。
   datasheet 原文："I2EC is not enabled until its controlled register in the EC side
   register is written"。专用端口路径另需 `XRAM[0x2012] SPCTRL2.PI2ECEN=1` 并由
   `XRAM[0x2014]/[0x2015] PI2ECH/PI2ECL`（复位默认 0x0380）决定基址。
   这些都是 EC 域寄存器，主机无法直接写——若固件未启用，主机侧不存在"解锁寄存器"。

3. **但 I2EC 有两条主机传输路径，我们只试过一条**。除了专用端口（0x380 试验已失败），
   还有 **PNPCFG depth-2 路径**：在 0x4E/0x4F 上以 index 0x2E/0x2F（D2ADR/D2DAT）
   进入第二层地址空间，其 offset 0x10/0x11/0x12 即 I2EC_ADDR_L / I2EC_ADDR_H /
   I2EC_DATA。**depth-2 只受 SPCTRL1.I2ECCTRL 门控，不需要 PI2ECEN**。
   这条路径本项目（embedded-controller.md）尚未测试——它是下一步唯一零成本动作。

4. 判定逻辑：用 depth-2 读 `0x200D`。低两位为 `10`/`11` → I2EC 已启用（只是专用端口
   没开/基址不同），此后可直接经 depth-2 读写全 16 位 EC memory，还能顺带读
   `0x2012/0x2014/0x2015` 查明专用端口的实际配置；若 `0x200D` 读值恒为 0xFF 而
   depth-2 的 0x10/0x11 写读回环正常 → I2ECCTRL=00，固件未启用，主机侧无解，
   只能改 EC 固件初始化表（参考 minortex/ec_reverse 的两字节补丁方法）。

---

## 1. Datasheet 证据（IT5570 A 版 V0.3.1，本地 ref/ 与 archive.org 同一文件）

公开渠道只有 IT5570 V0.3.1（archive.org，575 页）；ITE 官网 IT5570/5571 均在 NDA 后。
社区与实测（见 §3、§4）表明 IT5571 与 IT5570 同 die 变体，GCTRL/I2EC 行为一致
（it87 issue #49 亦提到 IT5571≈IT8571 关系）。

### 1.1 PNPCFG depth-2：6.3.2.9 Depth 2 I/O Address (D2ADR)、6.3.2.10 D2DAT

- PNPCFG index `0x2E` = D2ADR，index `0x2F` = D2DAT，构成 256 字节第二层空间。
- 第二层空间内：`offset 0x10 = I2EC_ADDR_L`、`0x11 = I2EC_ADDR_H`、`0x12 = I2EC_DATA`。
- 这是 PNPCFG 全局寄存器（非任何 LDN 内部），仅需进入 config mode 即可访问。

### 1.2 LDN 0x10 = RTC-like Timer（6.8 节，Table 6-10）

LDN 分配表（Table 6-10）：01h UART1、02h UART2、04h SWUC、05h Mouse、06h Keyboard、
0Ah CIR、0Fh SMFI、**10h RTC-like Timer (RTCT)**、11h–19h PMC1-5/SSPI/PECI。
RTCT 用 `(IOAD0+0,+1)`、`(IOAD1+0,+1)` 两对 index/data（legacy 70/71、72/73），
含 192B SRAM（bank0 128B + bank1 64B），"Access to the RTCT SRAM may be locked"。
→ 星耀14 LDN 0x10 @ 0x680/0x682 完整对应 RTCT；I2EC 与 LDN 无关。

### 1.3 启用条件（7.17 GCTRL「EC Interface Registers」，基址 0x2000，仅 EC 可访问）

| XRAM | 寄存器 | 位 | 复位默认 | 作用 |
|---:|---|---|---:|---|
| 0x200D | SPCTRL1（7.17.4.15，offset 0Dh） | 1:0 | 00b | **I2ECCTRL：00 disabled / 10 read-only / 11 read-write / 01 reserved** |
| 0x2012 | SPCTRL2（7.17.4.18，offset 12h） | 0 | 0b | **PI2ECEN：1=按 PORT_I2EC 地址译码 I2EC 周期**（仅专用端口路径） |
| 0x2014 | PI2ECH（7.17.4.21，offset 14h） | 7:0 | 03h | PORT_I2EC[15:8]（高 4 位强制 0） |
| 0x2015 | PI2ECL（7.17.4.22，offset 15h） | 7:0 | 80h | PORT_I2EC[7:0]（低 2 位强制 0）→ 复位默认基址 0x0380 |

端口定义（7.17.4.21 原文）：
- `PORT_I2EC + 1` = I2EC_XADDR_H（地址高字节）
- `PORT_I2EC + 2` = I2EC_XADDR_L（地址低字节）
- `PORT_I2EC + 3` = I2EC_XDATA（数据）
- "EC only accepts the LPC I/O cycle with PORT_I2EC address if PI2ECEN bit in SPCTRL2
  register is set."

门控关系推理（依据 7.23.3.5）：I2ECCTRL 是 I2EC 功能总开关（描述无路径限定，
"I2EC is not enabled until its controlled register in the EC side register is written"）；
PI2ECEN 只控制专用端口的地址译码。因此 depth-2 路径仅需 I2ECCTRL≠00。

### 1.4 操作流程（7.23.3.5 EC Memory Snoop，Figure 7-41/7-42）

**Figure 7-41（depth-2，经 0x4E/0x4F；以下按本机 0x4E 举配置口）**

```text
选地址高：out 0x4E,0x2E; out 0x4F,0x11   # D2ADR=0x11 → I2EC_ADDR_H
写  值  ：out 0x4E,0x2F; out 0x4F,hi
选地址低：out 0x4E,0x2E; out 0x4F,0x10   # D2ADR=0x10 → I2EC_ADDR_L
写  值  ：out 0x4E,0x2F; out 0x4F,lo
选数据  ：out 0x4E,0x2E; out 0x4F,0x12   # D2ADR=0x12 → I2EC_DATA
读/写   ：out 0x4E,0x2F; v = in 0x4F     # 读；写则为 out 0x4F,v
```

**Figure 7-42（专用端口）**：`out (PORT+1),hi; out (PORT+2),lo; R/W (PORT+3)`。

datasheet 注意事项（7.23.3.5）：
- I2EC/D2EC 不会触发 read-clear 寄存器的清除动作；
- 写非 F/F 型寄存器（write-clear、启动状态机类）结果不保证；
- I2EC 与 D2EC 可同时工作；I2EC 可配置为全目标只读。

另有第三条通道：**SMBus slave design A/B 的 pre-defined command（7.7.3，I2C 从地址
0x5B，命令 0x10=I2EC Address、0x11=I2EC Data）**——即 DBGR/SMB 调试从机，需外部
I2C 主机（见 §4.4 的 i2ite），且进入 DBGR 后不能热退出（7.23.3.2）。

---

## 2. 我们的实况复盘（对应 YMGPwcca/Mechrevo-Xingyao-14 docs/embedded-controller.md）

- 0x380 专用端口试验：读 0x200D/0x0394/0x0D13/0x0D14 全 0xFF，而 MMIO 已知
  0x0394=92，失败 → 专用端口未启用或基址非 0x380。
- 静态发现固件写 `XRAM[0x200D]`、`0x2012`、`0x2014`、`0x2015`，其中 0x2012=0x20。
  按 IT5570 A 手册 SPCTRL2 仅 bit0=PI2ECEN，写 0x20 落在保留位；不能排除 IT5571(D)
  的 PI2ECEN 位布局不同（如 bit5），也不能排除 0x2012 在 IT5571 上另有含义。
- **depth-2（0x2E/0x2F）从未测试过**——这是当前最大的空白。
- H2RAM 仅 256B 窗口（0xFEEC2300 ↔ XRAM 0x0300），0x200D 不可达，无法用于查询/
  改写 SPCTRL1；datasheet 明确 H2RAM 只映射 DLM lower 4K，方案上限如此。

0x380 失败的三种可能原因（一次 depth-2 读取即可全部判明）：
1. I2ECCTRL=00（总开关关）；
2. I2ECCTRL≠00 但 PI2ECEN=0（专用端口译码关，depth-2 仍可用）；
3. I2ECCTRL≠00、PI2ECEN=1，但 PI2ECH/L 被固件写成 0x380 以外的值
   （0x2014/0x2015 有固件写入痕迹 → 真实基址待查；注意不能与 RTCT 0x680 冲突）。

---

## 3. 别人的经验（I2EC 是否遇到过"不生效/需解锁"）

**未发现与我们完全相同的公开案例**（"固件出厂未启用 I2EC"）。最接近的素材：

### 3.1 minortex/ec_reverse（本仓库 refs/ 已有完整 clone；另一台 IT5571 机器）

- 那台机器（GXxHXxx/Slimbook 系固件 2.12）**出厂就把 I2EC 配成 read-write**：
  初始化表两处表驱动写 `SPCTRL1=0x43`（表项 `{0x20,0x0D,0x43}`，位于 flash 偏移
  0x0108A 与 0x06050，即 CODE:0x0FF7–0x1035 / 0xDF66 两张 `{addr_hi,addr_lo,value}`
  表），随后公共代码 `ORL A,#0xC8`，最终 `0x200D=0xCB`（bit1:0=11，RW）。
- 专用端口启用代码在 CODE:0xDF47–0xDF59：`SPCTRL2.PI2ECEN=1；PI2ECH=0x06；
  PI2ECL=0x80` → dedicated base 0x680，端口 0x681/0x682/0x683。
- 2026-07-19 真机验证 `i2ec_rw.py read 0x200D = 0xCB`，且与 H2RAM 交叉读一致。
- **改固件把 I2EC 降级只读的方法**：两个表项 `43 → 42`（最终 0x200D=0xCA，
  I2ECCTRL=10）。这证明：只要在 EC 初始化表里写一次 0x200D 的低两位，I2EC 就被
  永久启用/改权限——固件 patch 的最小改动就是 2 个字节。
- gist 评论区（w568w/957976b5…）minortex 原话：他从 IT5570 datasheet 7.17.4.15 找到
  "`xram[0x200d].1:0` dictates the read/write permissions"，并逆向 EC flasher
  `ifux64.efi` 得到"native method to bypass the restrictions via io ports"
  （即 tools/i2ec_rw.py 走 0x681–0x683）。他还发现部分地址（0x9C9、0xA4E 等）经
  ACPI 路径读回 0xFF 是 H2RAM 窗口权限，而非 I2EC 问题。

### 3.2 passiveEndeavour/it5570-fan（AceMagic W1 等白牌迷你机，IT5570 @ 0x4E）

**depth-2 I2EC 的现成 Linux 内核实现**（it5570_fan.c，称 "SMFI indirect"，寄存器号
与 datasheet Figure 7-41 完全一致）：

```c
#define SIO_PORT 0x4E
#define SIO_DATA 0x4F
static int sio_sram_read(u16 addr, u8 *val)
{
    outb(0x2E, SIO_PORT); outb(0x11, SIO_DATA);   /* I2EC_ADDR_H */
    outb(0x2F, SIO_PORT); outb((addr >> 8) & 0xFF, SIO_DATA);
    outb(0x2E, SIO_PORT); outb(0x10, SIO_DATA);   /* I2EC_ADDR_L */
    outb(0x2F, SIO_PORT); outb(addr & 0xFF, SIO_DATA);
    outb(0x2E, SIO_PORT); outb(0x12, SIO_DATA);   /* I2EC_DATA  */
    outb(0x2F, SIO_PORT); *val = inb(SIO_DATA);
}
/* SIO 进入序列：0x87,0x01,0x55,0xAA @0x4E；退出 0x02；ChipID @idx 0x20/0x21 */
```

该驱动用它读 0x05B9/0x086A/0x0C44 等 ACPI 256B 窗口之外的传感器——**证明在这批
IT5570 迷你机固件上 depth-2 出厂可用（未设专用端口也能用）**。这是我们方案的直接
先例。

### 3.3 其他

- **frankcrawford/it87 issue #64**（Gigabyte X870 双 ITE 芯片）：另一思路——通过
  MMIO（SMFI/ECIO）映射访问 EC 寄存器空间；还证明 0x4E 上盲目进 config mode 可能
  与 eSPI-LPC 桥（IT8883）冲突锁死需冷启动。星耀14 已能在 0x4E 正常读 ChipID，
  无此问题，但注意 depth-2 操作期间要持有 PNPCFG、结束退出。
- **c0d3z3r0/i2ite**：FTDI I2C 外部工具，走 SMBus debug slave（0x5B，0x10/0x11 命令）
  实现 I2EC/D2EC 读写（支持 `xram_dump()`），需拆机接线，作为最后手段。
- **内核 uniwill-laptop**（docs.kernel.org）：Uniwill 公模的 charge limit/风扇/键盘灯
  走标准接口（ACPI EC/WMI 层面），完全不涉及 I2EC——内核里没有现成 IT55xx I2EC
  模块；it5570-fan 是最接近的内核态实现。
- **nathanmarlor/strix-halo-fan-control**（Bosgame M5/IT5570）：用户态直接 EC 访问，
  但走的不是 I2EC。

---

## 4. 可执行答案：如何让 I2EC 生效（按优先级）

### 步骤 A（先做，只读、零风险）：depth-2 探测

前提：能对 0x4E/0x4F 进出 PNPCFG config mode（本项目已验证；进入序列参考
it5570-fan：`out 0x4E,0x87; 0x01; 0x55; 0xAA`）。

1. **先验证 depth-2 机构本身**：写 I2EC_ADDR_H/L 后读回
   （`D2ADR=0x11`→`D2DAT 写 0x5A`→`D2ADR=0x2F 选 D2DAT 再读`，注意读 D2DAT 前要
   先把 index 切到 0x2F）。0x10/0x11 能正确回环 ⇒ PNPCFG depth-2 活着。
2. **读 SPCTRL1**：按 §1.4 序列对 EC 地址 0x200D 做 depth-2 读。
3. **判读**：
   - `(v & 3) == 2` → I2EC read-only 已启用；`== 3` → read-write 已启用。
     立即用 depth-2 读 0x2012/0x2014/0x2015 记录 PI2ECEN 与真实专用基址
     （并解决 0x2012=0x20 的位疑点），随后 0x0000–0xFFFF 全空间可读。
     与已知值交叉验证：0x0394（MMIO 读到 92）。
   - 恒为 0xFF（且第 1 步回环正常、非 0xFF 的已知地址也全 0xFF）→
     **I2ECCTRL=00，固件未启用 I2EC**，转步骤 C。
   - 恒为 0xFF 且第 1 步回环也失败 → PNPCFG 被占/进入序列问题，排查后重试。
4. 只读、单客户端运行；depth-2 与其它 PNPCFG 访问互斥（同一把 0x4E/0x4F 锁）。

（若 `(v&3)==3` 且想启用专用端口：经 depth-2 向 0x2012 写 PI2ECEN、向 0x2014/0x2015
写基址即可——但 0x680 已被 RTCT 占用，须选未占用基址，如 0x380 或 0x900 段；
I2EC 写非 F/F 寄存器有风险，专用端口非必需，depth-2 已够用。）

### 步骤 B：启用专用端口（可选，仅当 I2ECCTRL≠00 且需要端口式访问）

- 从 depth-2 读到的 0x2014/0x2015 得到当前 PORT_I2EC；若 PI2ECEN=0 可经 depth-2
  写 0x2012 置位后按 Figure 7-42 用 `PORT+1/+2/+3`。
- 每次写 0x200D/0x2012 后是否需要同步等待 datasheet 未提；建议写后回读确认。

### 步骤 C：固件未启用（I2ECCTRL=00）时的唯一软件路径——改 EC 固件初始化表

参考 minortex/ec_reverse 的已验证模式（同 die、同寄存器）：

1. dump 本机 EC 固件（外部编程器或本仓库 ec-flash Follow Mode 路径）。
2. 反汇编找对 `0x200D`/`0x2012`/`0x2014`/`0x2015` 的初始化写入（本项目静态分析已
   定位到这些写入点；星耀14 固件里 0x200D 的写入值是多少待查——**很可能只差把
   低两位从 00 改成 10/11 的 1 个字节**）。
3. 若固件走同样的 `{addr_hi,addr_lo,value}` 表：把 0x200D 表项 value 的低两位改为
   `10b`（只读，推荐）或 `11b`（读写，高危）。若固件写的是常数 `ORL`，等价地把
   立即数 low-2 位改 10/11。
4. 最小 diff、双读校验、保留恢复路径后刷写；重启动按 §3.1 的分级验证
   （先确认 EC 正常，再 `0x200D&3` 判读，再与 MMIO 已知地址交叉验证）。
5. 风险提示（datasheet + minortex 实践）：
   - I2ECCTRL=11 会开放**整个 16 位 EC memory** 的写，包括片上外设寄存器，
     误写可致 EC 挂死；优先 10（只读）。
   - `0x683` 写路径在只读补丁机上无效；别与其他直接 outb 工具并行。
   - RTCT 占用 0x680 的事实说明选专用基址前必须扫描端口冲突。

### 步骤 D（最后手段）：D2EC / SMBus debug slave

外部 FTDI I2C（c0d3z3r0/i2ite，地址 0x5B，命令 0x10/0x11），或 DBGR/EPP。
注意 7.23.3.2：DBGR/SMB 一旦被侦测进入**不能热退出**，需要拆机与恢复预案。

---

## 5. 与各提问点的对应

1. **datasheet 启用条件**：见 §1.3——PNPCFG 无 I2EC 位；总开关 EC 侧
   SPCTRL1(0x200D)[1:0]，专用端口另需 SPCTRL2(0x2012).PI2ECEN + PI2ECH/L
   (0x2014/0x2015)。端口定义 `PORT+1/+2/+3`（datasheet 7.17.4.21）。
2. **别人遇到不生效**：无同款公开案例；it5570-fan（可用 depth-2）、it87#64
   （MMIO 替代路）、minortex（固件已启用+patch 降级）是三个最近参照。
3. **minortex/ec_reverse docs**：本地 clone 齐全；I2EC enable 相关 =
   `docs/xram_host_access.md` + `docs/i2ec_battery_investigation.md`（固件启用序列、
   43→42 补丁、真机 0xCB 验证、专用端口初始化代码位置）。
4. **w568w gist 评论区**：minortex 确认 0x200D 权限位出处（IT5570 手册 7.17.4.15）
   与 ifux64.efi 逆向出的 io port 方法；未讨论"出厂未启用的解锁"（他的机器已启用）。
5. **现成模块/工具**：内核态 it5570-fan（depth-2 I2EC 实战代码）；用户态
   ec_reverse tools/i2ec_rw.py（专用端口）；外部 i2ite（SMBus 0x5B）。无现成
   "IT55xx I2EC 一键解锁"工具——因为解锁本就只能靠固件。

## 6. 来源

- IT5570 A 版 V0.3.1 datasheet（本地 `refs/ec_reverse/ref/IT5570_A_V0.3.1_U.pdf`；
  [archive.org 镜像](https://ia800805.us.archive.org/18/items/it-5570-a-v0.3.1-u/IT5570_A_V0.3.1_U.pdf)）：
  §6.3.2.9/6.3.2.10（D2ADR/D2DAT、depth-2 offset 10/11/12）、Table 6-10（LDN 分配）、
  §6.8（RTCT）、§7.17.4.15/18/21/22（SPCTRL1/2、PI2ECH/L）、§7.23.3.5（ECMS、
  Figure 7-41/7-42、启用说明）、§7.7.3（SMBus pre-defined command）
- minortex/ec_reverse（本地 clone；[GitHub](https://github.com/minortex/ec_reverse)）：
  README「Experimental I2EC utility」、docs/xram_host_access.md、
  docs/i2ec_battery_investigation.md、tools/i2ec_rw.py
- w568w gist 评论区（[gist](https://gist.github.com/w568w/957976b59906e0ce5d6c13ad342e1593)）
- passiveEndeavour/it5570-fan（[GitHub](https://github.com/passiveEndeavour/it5570-fan)，
  it5570_fan.c 的 sio_sram_read/sio_enter）
- 本项目 docs/embedded-controller.md（0x380 试验、静态写入点、H2RAM 0xFEEC2300）
- frankcrawford/it87 [issue #64](https://github.com/frankcrawford/it87/issues/64)
- c0d3z3r0/i2ite（[GitHub](https://github.com/c0d3z3r0/i2ite)）
- 内核 uniwill-laptop 文档（[docs.kernel.org](https://docs.kernel.org/admin-guide/laptops/uniwill-laptop.html)）
- nathanmarlor/strix-halo-fan-control（[GitHub](https://github.com/nathanmarlor/strix-halo-fan-control)）
