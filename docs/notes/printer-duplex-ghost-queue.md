# CUPS 打印机设不了双面：默认打印机缺失 → 对话框掉进"幽灵队列"

> 环境：CachyOS / CUPS 2.4.20 / HPLIP hpcups 3.26.6，HP Smart Tank 660-670（网络，
> `hp:/net/Smart_Tank_660-670_series?ip=192.168.31.130`）。排查日期：2026-10-26。

## 症状

打印对话框里找不到/设不了**双面打印**选项。打印机本身正常，单面打印没问题。

## 排查过程（每步都是证据）

1. **PPD 其实有双面**——队列 `Smart_Tank_660-670` 的选项一查就有：

   ```
   $ lpoptions -p Smart_Tank_660-670 -l | grep -i duplex
   Duplex/Double-Sided Printing: DuplexNoTumble DuplexTumble *None
   ```

   硬件也支持自动双面（Smart Tank 660/670 系列规格如此）。所以不是"打印机不支持"。

2. **不带 `-p` 查默认队列时报错**，暴露真正的问题：

   ```
   $ lpoptions -l
   lpoptions：无法为 HP_Smart_Tank_660_670_series_9828A3 获取 PPD 文件：没有那个文件或目录
   ```

   这个名字不是本机任何真实队列。

3. **CUPS 服务端确认**：`/etc/cups/printers.conf` 里只有 `Smart_Tank_660-670` 一个
   队列，且**没有 `DefaultPrinter` 行**；`lpstat -d` 也报"无系统默认目标"。用户级
   `~/.cups/lpoptions`、`/etc/cups/lpoptions`、`LPDEST`/`PRINTER` 环境变量全都不存在。

4. **幽灵名字的来源**：`9828A3` 是打印机 DNS-SD 广播名（`HP Smart Tank 660-670
   series [9828A3]`）的尾缀。没有任何默认目标时，客户端问 CUPS 要默认打印机，
   cupsd 就按 mDNS 广播**现场合成一个免驱动临时队列**，命名规则是"型号下划线化 +
   广播尾缀"→ `HP_Smart_Tank_660_670_series_9828A3`。这个临时队列的 PPD 拉取失败，
   打印对话框预选它时拿不到任何作业选项 → 双面选项消失。

5. **附带残留**：Bottles（PDFelement）的 Wine 注册表里还有这个旧名字的 Windows
   打印机驱动条目（`system.reg` 中 `...\Drivers\Version-3\HP_Smart_Tank_660_670_series_9828A3`），
   应用内的打印机下拉列表会更乱。

## 修复

设默认打印机，一行了事：

```bash
sudo lpadmin -d Smart_Tank_660-670
```

验证：`lpstat -d` 应报 `系统默认目标：Smart_Tank_660-670`，`lpoptions -l`（不带
`-p`）能正常列出 Duplex 选项。

## 速查

| 命令/取值 | 说明 |
|---|---|
| `lpstat -d` | 查默认目标；报"无系统默认目标"就是本坑 |
| `lpoptions -p 队列 -l` | 列某队列全部作业选项（Duplex、PageSize、ColorModel…） |
| `lpoptions -l` | 按默认队列列选项；报"无法获取 PPD"说明默认指向了临时/幽灵队列 |
| `DuplexNoTumble` | 双面·长边翻页（普通竖版文档） |
| `DuplexTumble` | 双面·短边翻页（横版文档） |
| `sudo lpadmin -d 队列名` | 设系统默认打印机（本篇修复） |
| `sudo lpadmin -p 队列 -o Duplex=DuplexNoTumble` | 把双面设为队列默认（没开，照片纸不该双面） |
| `lp -d 队列 -o Duplex=DuplexNoTumble 文件` | 单次任务直接指定双面 |

## 备注

- 打印对话框目的地列表里若出现 `HP_Smart_Tank_660_670_series_9828A3` 这类
  "型号_尾缀"命名的条目，是 DNS-SD 合成的临时队列，别选；选 `Smart_Tank_660-670`。
- Bottles/Wine 应用每次启动会重新枚举 CUPS 队列，选当前名字的打印机即可；
  注册表旧条目不影响使用，强迫症可用 wine regedit 清理。
- 排查口诀：**"选项不见了，先怀疑对话框连的不是你以为的那台队列"**——
  `lpoptions -l`（不带 `-p`）复现对话框视角，是最快的一刀。
