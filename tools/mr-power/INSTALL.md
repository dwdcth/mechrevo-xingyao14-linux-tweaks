# mr-power 安装

Ryzen SMU 功耗墙（PPT）快捷脚本：`quiet`=12W 静音档、`<N>`=钳到 N 瓦（5-65）、`stock`=按当前 Fn+X 档位用 WMI 重写恢复、`info`=查当前限额。中文输出。

## 前提

- **ryzenadj**：`mise use -g github:FlyGoat/RyzenAdj`（上游发 manylinux 免编译二进制）或 AUR `ryzenadj`
- **mechrevo-ctl** 已装（`stock` 子命令走它的 WMI 通道）：见 [../mechrevo-ctl/INSTALL.md](../mechrevo-ctl/INSTALL.md)

## 安装

```bash
sudo install -Dm755 mr-power /usr/local/bin/mr-power
```

脚本会经 `SUDO_USER`（兜底 uid 1000）定位该用户 mise 目录下的 ryzenadj 真实路径——root 的 mise 不信任用户 shim，不能直接调。

## 用法与验证

```bash
mr-power info     # 当前功耗墙：持续 XW · 长时 XW · 短时爆发 XW
mr-power quiet    # 12W
mr-power 20       # 20W
mr-power stock    # 恢复机器默认
```

## 注意

- **切 KDE 电源模式或按 Fn+X 换档不会自动解除 ryzenadj 钳制**（PMF 不重写 PPT），恢复用 `mr-power stock`、Fn+X 按两下或重启
- 空闲功耗与功耗墙无关（实测空闲 ~4W 不随钳制变化）
