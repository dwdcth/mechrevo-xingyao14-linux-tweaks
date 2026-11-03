# F10/F11/F12 与 Fn 互换：hwdb 改键与三个终端坑（fnswap）

需求：单按 F10/F11/F12 直接出 **Insert / PrintScreen / Delete**，按住 Fn 才是 F 键。
结论：udev hwdb 内核级 scancode→keycode 改写，零守护进程、重启持久、`--restore` 一键还原。
交互向导见 [tools/fnswap](../../tools/fnswap/INSTALL.md)，本篇留证据链与排雷记录。

## scancode 实测（AT Translated Set 2 keyboard，/dev/input/event1）

| 按法 | scancode | 出厂 keycode |
|---|---|---|
| 单按 F10 | 0x44 | f10 |
| Fn+F10 | 0xd2 | insert |
| 单按 F11 | 0x57 | f11 |
| Fn+F11 | 0xb7 | sysrq（即 PrintScreen） |
| 单按 F12 | 0x58 | f12 |
| Fn+F12 | 0xd3 | delete |

- MSC_SCAN 是 atkbd 的 **set-1 翻译码，扩展键高位置 1**：d2=52h|80h（Insert）、
  d3=53h|80h（Delete）；同机方向键 cb/cd/d0 与左 Meta db 可互证（4bh|80h 等）
- 第一次抓取 Fn+F11 得到与单按相同的 0x57，一度以为该键没有独立 Fn 功能——
  实为当时没按住 Fn，重抓得到 b7/sysrq。**同码先怀疑按法，再怀疑 Fn 锁（Fn+Esc）**

## 最终 hwdb（/etc/udev/hwdb.d/99-fn-swap.hwdb）

```ini
# 还原: python3 tools/fnswap/fnswap.py --restore
# 方向: 单独按=Insert/PrSc/Del, Fn+=F键
# 原始映射(留档): KEYBOARD_KEY_44=f10 KEYBOARD_KEY_D2=insert KEYBOARD_KEY_57=f11 KEYBOARD_KEY_B7=sysrq KEYBOARD_KEY_58=f12 KEYBOARD_KEY_D3=delete
evdev:name:AT Translated Set 2 keyboard:dmi:bvn*:bvr*:bd*:svn*:pn*:pvr*
 KEYBOARD_KEY_44=insert
 KEYBOARD_KEY_D2=f10
 KEYBOARD_KEY_57=sysrq
 KEYBOARD_KEY_B7=f11
 KEYBOARD_KEY_58=delete
 KEYBOARD_KEY_D3=f12
```

写完 `systemd-hwdb update` + `udevadm trigger <键盘 syspath>` 即时生效，无需重启。
头部「原始映射」留档由脚本在写入前自动嵌入——hwdb 本就是出厂 atkbd 键位表之上的
覆盖，**删掉规则文件即回到出厂表**，留档只为让 `--restore` 动手前能回显改了什么。

## 排雷三则（fnswap 已内置对策）

1. **终端回显竞态**：抓键等待时终端处于 ECHO 模式，F 键以 `^[[21~` 形式回显；更糟的是
   按住键产生的自动重复转义序列会**晚于脚本恢复回显**才落进 tty 缓冲，把确认输入污染成
   `^[[3~y`（表现为确认被误判为"取消"）。对策：等待期切 cbreak 关回显，收尾宽限 0.3s
   让迟到序列落完再 TCSAFLUSH 丢弃；确认改单键裸读，吞掉一切非 y/n 字节。
2. **evtest 与 stdin 是同一次击键的两条流**：确认时按的 `y`/回车同样被 evtest 收进
   事件队列，紧接着的验证步骤把它们当成被测按键（表现为"还没按 F10/F11 就一路输出到
   F12"，且验证结果是 `y`、`enter`）。对策：每条提示前 drain 事件队列，只认提示
   **之后**的新按键。
3. **EVIOCGKEYCODE ioctl 号手算坑**：漏算 size 位段的 0x80004508 会被内核解析成
   EVIOCGUNIQ，返回 ENOENT——极易误判成设备问题。正确值
   `_IOR('E',0x04,int[2])` = `(2<<30)|(8<<16)|(0x45<<8)|4` = 0x80084504。

## 验证与还原

- **免按键验证**：EVIOCGKEYCODE 直接查内核当前键位表，实测 6/6 ✓（安装向导收尾
  的按键验证之前先做了这道内核态检查，可区分"规则没生效"与"验证吃了假事件"）
- **还原**：`sudo python3 tools/fnswap/fnswap.py --restore`——先回显留档再确认删除；
  或手动删文件 + `systemd-hwdb update` + `udevadm trigger -s input`
