# fnswap — F10/F11/F12 与 Fn 组合互换（hwdb，零守护）

改完后**单按 F10/F11/F12 = Insert / PrintScreen / Delete，按住 Fn 再按才是 F 键**。
实现是 udev hwdb 的内核级 scancode→keycode 映射：无任何后台进程/daemon，
重启后持续有效。交互向导 `fnswap.py` 一步步引导抓 scancode、生成规则、写入并当场验证。

## 前提

- 验证环境：**P916F-STX，BIOS 1.15，CachyOS（内核 7.3），KDE Plasma 6.8 Wayland**
- `evtest`：`sudo pacman -S evtest`
- 权限：抓键只需 `input` 组成员；写 hwdb 需要 root——脚本不带 sudo 跑也行，
  写入那步会自动 `sudo tee`（要一次密码），或者干脆整体 `sudo python3` 跑
- Fn 锁（Fn+Esc）保持**关**：锁着时两种按法 scancode 相同，脚本会提示后放弃
- 本机实测 scancode（`AT Translated Set 2 keyboard`，atkbd set-1 翻译码，扩展键高位置 1；
  **仅对这版 EC 有效**，别机照抄无意义）：

| 按法 | scancode | 默认 keycode |
|---|---|---|
| 单按 F10 | 0x44 | f10 |
| Fn+F10 | 0xd2 | insert |
| 单按 F11 | 0x57 | f11 |
| Fn+F11 | 0xb7 | sysrq（=PrintScreen） |
| 单按 F12 | 0x58 | f12 |
| Fn+F12 | 0xd3 | delete |

## 安装

```bash
sudo python3 tools/fnswap/fnswap.py
```

按提示依次按 6 组键（每键单独/Fn 各一次）→ 确认 `y` → 自动写入
`/etc/udev/hwdb.d/99-fn-swap.hwdb` 并执行 `systemd-hwdb update` + `udevadm trigger` →
再单按 F10/F11/F12 当场验证。

生成的规则（本机实测值）：

```ini
evdev:name:AT Translated Set 2 keyboard:dmi:bvn*:bvr*:bd*:svn*:pn*:pvr*
 KEYBOARD_KEY_44=insert
 KEYBOARD_KEY_D2=f10
 KEYBOARD_KEY_57=sysrq
 KEYBOARD_KEY_B7=f11
 KEYBOARD_KEY_58=delete
 KEYBOARD_KEY_D3=f12
```

反向（单按=F 键，Fn+=Insert/PrSc/Del）：`sudo python3 tools/fnswap/fnswap.py --reverse`

## 验证

- 脚本收尾的即时验证（按下即比对 keycode）
- 免按键的内核态查表（EVIOCGKEYCODE，直接读当前映射）：

```bash
python3 - <<'EOF'
import fcntl, array, os
IOC = (2<<30)|(8<<16)|(0x45<<8)|4        # EVIOCGKEYCODE = _IOR('E',0x04,int[2])
want = {0x44:110, 0xd2:68, 0x57:99, 0xb7:87, 0x58:111, 0xd3:88}
fd = os.open('/dev/input/event1', os.O_RDONLY)
for s, w in want.items():
    b = array.array('i', [s, 0]); fcntl.ioctl(fd, IOC, b, True)
    print(f"{s:#04x}: {b[1]}  {'OK' if b[1]==w else '不符,期望 '+str(w)}")
EOF
```

- 手感：任意编辑器里单按 F12 删字符、F10 插入、F11 为截图键

## 卸载 / 还原

一键（先回显 hwdb 文件里留档的「原始映射」，确认后删除规则、恢复出厂键位表）：

```bash
sudo python3 tools/fnswap/fnswap.py --restore
```

手动等价：

```bash
sudo rm /etc/udev/hwdb.d/99-fn-swap.hwdb
sudo systemd-hwdb update && sudo udevadm trigger -s input
```

排雷记录（终端回显竞态、evtest 双流积压、ioctl 号手算坑）见
[docs/notes/fn-key-swap.md](../../docs/notes/fn-key-swap.md)。
