# mechrevo-ctl 安装

星耀14（P916F-STX）综合控制工具：EC 直控（充电阈值/键盘灯/Fn锁）、WMI 性能模式、FN 键 OSD 守护。纯 std 零依赖 Rust，单二进制。

## 前提

- Rust 工具链（`pacman -S rust` 或 `mise use -g rust`）
- Linux 7.x（WMI 事件通道依赖 `huawei_wmi` 模块自动加载，本机同方公模内核自带）
- OSD 功能需要 KDE Plasma（powerdevil / kscreen-doctor / 通知通道）

## 构建与安装

```bash
cargo build --release
sudo install -Dm755 target/release/mechrevo-ctl /usr/local/bin/mechrevo-ctl
```

## 命令速查（`sudo mechrevo-ctl <命令>`）

| 命令 | 作用 |
|---|---|
| `info` | 综合状态（EC/电池/模式/风扇/背光/充电阈值） |
| `charge status` | 当前充电阈值（PMC2 通道） |
| `charge set <T1> <T2>` | 设置并使能（T1 开始充电 / T2 上限，如 `80 100`） |
| `charge restore [T1] [T2]` | 幂等恢复（默认 80/100；电池耗尽断电 EC 会清零，开机跑一次） |
| `kbd get` / `kbd set 0-2` | 键盘背光三档（EC 0x33） |
| `fnlock get` / `fnlock set 0\|1` | Fn 键锁定（EC 0x32 bit1） |
| `mode` | 当前性能模式 FTVL 值（EC 0x3F） |
| `perf get` / `perf set 1\|2` | WMI 性能模式（1=均衡 2=性能，带 ALIB 功耗墙联动） |
| `fan` | 双风扇原始数值（未标定，非认证 RPM） |
| `turbo` | Turbo 状态读数 |
| `wmi <mfid> <sfid> [param]` | 原始 WMAA 调用（16进制，逆向用） |
| `osd` | FN 键 OSD 守护（由 systemd 服务跑，见下方） |

## 开机自启（OSD 守护 + 充电阈值恢复）

systemd 单元在本仓库 [`../../integrations/systemd/`](../../integrations/systemd/INSTALL.md)，含 `mechrevo-osd.service` 与 `mechrevo-charge.service`。

## 验证

```bash
sudo mechrevo-ctl info          # 应列出 EC 版本/电池/充电阈值
sudo mechrevo-ctl charge status # 显示 T1/T2
```

OSD：服务起来后按 Fn+Esc（触控板开关）应弹出 OSD 并实际开关触控板。

## 安全边界与卸载

- EC 写入经 `/dev/mem`，代码内白名单仅放行键盘背光/触控板键两个偏移；充电阈值走 PMC2 端口 0x68/0x6C 协议
- 原理与寄存器地图：[`../../docs/hardware-map.md`](../../docs/hardware-map.md)
- 卸载：`sudo systemctl disable --now mechrevo-osd mechrevo-charge && sudo rm /usr/local/bin/mechrevo-ctl`（EC 里已写入的充电阈值会保留，可用 `charge set 0 100` 类操作复位）
