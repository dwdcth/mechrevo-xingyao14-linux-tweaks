#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""MechRevo 星耀14 — F10/F11/F12 与 Fn 组合互换(udev hwdb,无需任何后台守护)

默认方向(改完后):
    单独按 F10/F11/F12 → Insert / PrintScreen / Delete
    按住 Fn 再按       → F10 / F11 / F12

反向(--reverse):
    单独按 F10/F11/F12 → F10 / F11 / F12
    按住 Fn 再按       → Insert / PrintScreen / Delete

用法(建议直接 sudo,省得中途再输密码):
    sudo python3 tools/fnswap.py [--reverse]

说明:
- 抓键期间会临时关闭终端回显(cbreak),不会出现 ^[[21~ 之类的乱码,
  也不会让残留转义序列污染后续确认输入。
- 个别 Fn 组合若没有独立 scancode(如本机实测 Fn+F11 与单独按同为 0x57,
  即 Fn+F11 本来就没有独立功能),脚本会自动跳过该键,其余键照常互换。
- 不 sudo 也能跑:抓键阶段只需要 input 组权限,写 hwdb 时会自动用 sudo tee。

还原:
    sudo python3 tools/fnswap.py --restore   # 一键:先回显原始映射留档,再删规则回出厂键位
    # 或手动:
    #   sudo rm /etc/udev/hwdb.d/99-fn-swap.hwdb
    #   sudo systemd-hwdb update && sudo udevadm trigger -s input
"""

import argparse
import os
import queue
import re
import subprocess
import sys
import termios
import threading
import time

try:
    import tty
except ImportError:  # 非 Unix 环境兜底,正常用不到
    tty = None

KBD_NAME = "AT Translated Set 2 keyboard"
HWDB_PATH = "/etc/udev/hwdb.d/99-fn-swap.hwdb"
FKEYS = ("f10", "f11", "f12")
IDLE_SKIP = 8  # 秒:Fn 组合等这么久还没等到不同按键,就跳过该键

STEPS = [
    ("10", "单独按一下 F10", "按住 Fn 不放,再按一下 F10"),
    ("11", "单独按一下 F11", "按住 Fn 不放,再按一下 F11"),
    ("12", "单独按一下 F12", "按住 Fn 不放,再按一下 F12"),
]

SCAN_RE = re.compile(r"MSC_SCAN\), value ([0-9a-fA-F]+)")
KEY_RE = re.compile(r"EV_KEY\), code \d+ \(KEY_([A-Za-z0-9_]+)\), value (\d)")


def find_device():
    """在 /proc/bus/input/devices 里按名字找到内置键盘的 event 节点。"""
    name = None
    try:
        f = open("/proc/bus/input/devices", encoding="utf-8", errors="replace")
    except OSError:
        return None
    with f:
        for line in f:
            if line.startswith("N: Name="):
                name = line.split("=", 1)[1].strip().strip('"')
            elif line.startswith("H: Handlers=") and name == KBD_NAME:
                for tok in line.split():
                    if tok.startswith("event"):
                        return "/dev/input/" + tok
    return None


class QuietWait:
    """等物理按键期间关掉终端回显,并清掉输入缓冲——
    否则 F 键会以 ^[[21~ 形式回显、残留字节还会混进后面的 input()。"""

    def __enter__(self):
        self.fd = sys.stdin.fileno() if sys.stdin.isatty() else None
        if self.fd is not None:
            self.saved = termios.tcgetattr(self.fd)
            tty.setcbreak(self.fd)  # 关回显;ISIG 保留,Ctrl+C 仍有效
            termios.tcflush(self.fd, termios.TCIFLUSH)
        return self

    def __exit__(self, *exc):
        if self.fd is not None:
            # 宽限 0.3s:按住期间终端侧自动重复的转义序列可能还在路上,
            # 让它们趁回显仍关闭时落进缓冲,再随恢复一起丢弃
            time.sleep(0.3)
            termios.tcsetattr(self.fd, termios.TCSAFLUSH, self.saved)


def confirm(prompt):
    """单键裸读确认:关回显、丢弃迟到的转义序列,只认 y/n/回车。"""
    print(f"{prompt} [y/N] ", end="", flush=True)
    if not sys.stdin.isatty():
        return sys.stdin.readline().strip().lower() == "y"
    fd = sys.stdin.fileno()
    saved = termios.tcgetattr(fd)
    try:
        tty.setcbreak(fd)
        time.sleep(0.25)                       # 等上一按键迟到的转义序列落完
        termios.tcflush(fd, termios.TCIFLUSH)  # 一次性清掉
        while True:
            ch = sys.stdin.read(1)
            if ch in ("y", "Y"):
                print("y")
                return True
            if ch in ("n", "N", "\r", "\n", ""):
                print("n")
                return False
            # 其余字节(转义序列、误触的键)全部吞掉
    finally:
        termios.tcsetattr(fd, termios.TCSAFLUSH, saved)


def trigger(sysdev, use_sudo=False):
    """udevadm trigger 键盘设备让 hwdb 即时套用,失败则退化为整个 input 子系统。"""
    pre = ["sudo"] if use_sudo else []
    r = subprocess.run(pre + ["udevadm", "trigger", sysdev])
    if r.returncode != 0:
        subprocess.run(pre + ["udevadm", "trigger", "-s", "input"], check=True)


def restore(sysdev):
    """一键还原:hwdb 只覆盖了几个 scancode,删掉规则文件、重编译、重新 trigger,
    内核就回到出厂 atkbd 键位表。动手前先回显文件里的「原始映射」留档。"""
    if not os.path.exists(HWDB_PATH):
        sys.exit(f"{HWDB_PATH} 不存在——本来就没改过,无需还原。")
    with open(HWDB_PATH) as f:
        old = f.read()
    print("将删除以下规则(头部「原始映射」注释留档了改动前的键位):\n")
    print(old)
    if not confirm("确认还原为出厂键位?"):
        sys.exit("已取消,未做任何修改。")
    if os.geteuid() == 0:
        os.remove(HWDB_PATH)
        subprocess.run(["systemd-hwdb", "update"], check=True)
        trigger(sysdev)
    else:
        subprocess.run(["sudo", "rm", HWDB_PATH], check=True)
        subprocess.run(["sudo", "systemd-hwdb", "update"], check=True)
        trigger(sysdev, use_sudo=True)
    print("\n已恢复出厂键位表。想再互换回来,重跑一次本脚本即可。")


class KeyReader:
    """后台跑 evtest,把「MSC_SCAN + EV_KEY 按下」配成 (scancode, keyname) 入队。"""

    def __init__(self, dev):
        self.p = subprocess.Popen(
            ["evtest", dev],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            text=True, bufsize=1,
        )
        self.q = queue.Queue()
        threading.Thread(target=self._pump, daemon=True).start()

    def _pump(self):
        scan = None
        for line in self.p.stdout:
            m = SCAN_RE.search(line)
            if m:
                scan = m.group(1).lower()
                continue
            m = KEY_RE.search(line)
            if m and int(m.group(2)) == 1 and scan:  # 只取按下,忽略松开/重复
                self.q.put((scan, m.group(1).lower()))
            scan = None

    def wait_press(self, timeout=None):
        """等到下一个有效按键(Fn 自身产生的事件不算数);超时返回 None。"""
        while True:
            try:
                scan, key = self.q.get(timeout=timeout)
            except queue.Empty:
                return None
            if key == "fn" or key.startswith("fn_"):
                continue
            return scan, key

    def drain(self):
        """丢掉队列里积压的事件——确认时按的 y/回车同样会进 evtest 流,
        不清掉会被下一个验证步骤当成新按键。"""
        while True:
            try:
                self.q.get_nowait()
            except queue.Empty:
                return

    def stop(self):
        self.p.terminate()


def ask_bare(reader, prompt):
    reader.drain()
    print(f"\n▶ {prompt}   (Ctrl+C 退出)", flush=True)
    with QuietWait():
        scan, key = reader.wait_press()
    print(f"  抓到: scancode={scan}  当前 keycode={key}")
    return scan, key


def ask_fn_combo(reader, n, prompt, bare_scan):
    """等 Fn 组合;按出来还是单独按那个 scancode 就继续等,超时则跳过该键。"""
    reader.drain()
    print(f"\n▶ {prompt}   (Ctrl+C 退出)", flush=True)
    same = 0
    with QuietWait():
        while True:
            got = reader.wait_press(timeout=IDLE_SKIP)
            if got is None:
                print(f"  {IDLE_SKIP} 秒内没等到与「单独按」不同的按键,跳过 F{n} 的互换。")
                print(f"  (这台机器的 Fn+F{n} 应该本来就没有独立功能,hwdb 无从区分)")
                return None
            scan, key = got
            if scan == bare_scan:
                same += 1
                if same >= 5:
                    print(f"  连续 {same} 次都还是 F{n} 本身,跳过 F{n} 的互换。")
                    return None
                print(f"  还是 F{n} 本身(scancode {scan})——确认 Fn 按住了,再按一次")
                continue
            print(f"  抓到: scancode={scan}  当前 keycode={key}")
            return scan, key


def main():
    ap = argparse.ArgumentParser(description="星耀14 F10/F11/F12 与 Fn 组合互换(hwdb)")
    ap.add_argument("--reverse", action="store_true",
                    help="反向:单独按=F10/F11/F12,Fn+=Insert/PrSc/Del")
    ap.add_argument("--device", help="强制指定设备节点,如 /dev/input/event1")
    ap.add_argument("--restore", action="store_true",
                    help="一键还原:删除互换规则,恢复出厂键位(先回显留档再动手)")
    args = ap.parse_args()

    dev = args.device or find_device()
    if not dev:
        sys.exit(f"没找到名为「{KBD_NAME}」的设备,用 --device /dev/input/eventX 指定")
    sysdev = "/sys/class/input/" + os.path.basename(dev)
    print(f"键盘设备: {dev} ({KBD_NAME})")

    if args.restore:
        restore(sysdev)
        return

    reader = KeyReader(dev)
    try:
        results = {}
        for n, bare_p, fn_p in STEPS:
            bs, bk = ask_bare(reader, bare_p)
            fn = ask_fn_combo(reader, n, fn_p, bs)
            results[n] = {"bare": (bs, bk), "fn": fn}

        # ---- 校验并生成规则 ----
        rules, done, skipped = [], [], []
        print("\n抓到的映射:")
        for n, _, _ in STEPS:
            bs, bk = results[n]["bare"]
            fg = results[n]["fn"]
            if fg is None:
                skipped.append(n)
                print(f"  F{n}: 单独按 {bs} → {bk} | Fn 组合无独立 scancode,跳过")
                continue
            fs, fk = fg
            print(f"  F{n}: 单独按 {bs} → {bk} | Fn {fs} → {fk}")
            if bk == fk:
                skipped.append(n)
                print(f"        两种按法 keycode 相同({bk}),跳过")
                continue
            fkey = "f" + n
            if args.reverse:
                if bk in FKEYS:
                    skipped.append(n)
                    print(f"        单独按现在就是 {bk},已是反向目标,跳过")
                    continue
                rules += [(bs, fkey), (fs, bk)]   # 单独按→F键,Fn→原功能
            else:
                rules += [(bs, fk), (fs, fkey)]   # 单独按→原Fn功能,Fn→F键
            done.append(n)

        if not rules:
            sys.exit("没有任何键可以互换,未做修改。")
        if skipped:
            names = "、".join("F" + n for n in skipped)
            print(f"跳过: {names}(维持原样)")

        comment = "单独按=F键, Fn+=Insert/PrSc/Del" if args.reverse \
            else "单独按=Insert/PrSc/Del, Fn+=F键"
        if skipped:
            comment += "; 跳过 " + "、".join("F" + n for n in skipped) + "(无独立 Fn scancode)"
        # 改动前的原始键位留档(嵌进规则文件头,--restore 时回显)
        orig = []
        for n in done:
            bs, bk = results[n]["bare"]
            fs, fk = results[n]["fn"]
            orig += [(bs, bk), (fs, fk)]
        hwdb = "\n".join(
            [
                "# MechRevo 星耀14 — F10/F11/F12 与 Fn 组合互换(tools/fnswap.py 生成)",
                "# 还原: python3 tools/fnswap/fnswap.py --restore",
                "#       (或删除本文件后 systemd-hwdb update && udevadm trigger -s input)",
                "# 方向: " + comment,
                "# 原始映射(留档): " + " ".join(
                    f"KEYBOARD_KEY_{s.upper()}={k}" for s, k in orig),
                f"evdev:name:{KBD_NAME}:dmi:bvn*:bvr*:bd*:svn*:pn*:pvr*",
            ]
            + [f" KEYBOARD_KEY_{scan.upper()}={key}" for scan, key in rules]
        ) + "\n"

        print("\n将写入 " + HWDB_PATH + ":")
        print(hwdb)
        if not confirm("确认应用?"):
            sys.exit("已取消,未做任何修改")

        # ---- 应用 ----
        if os.geteuid() == 0:
            with open(HWDB_PATH, "w") as f:
                f.write(hwdb)
            subprocess.run(["systemd-hwdb", "update"], check=True)
            trigger(sysdev)
        else:
            subprocess.run(["sudo", "tee", HWDB_PATH], input=hwdb, text=True,
                           check=True, stdout=subprocess.DEVNULL)
            subprocess.run(["sudo", "systemd-hwdb", "update"], check=True)
            trigger(sysdev, use_sudo=True)

        # ---- 立刻验证(只验证互换成功的键) ----
        print("\n规则已写入并触发,现在验证(规则应已即时生效):")
        bad = 0
        for n in done:
            want = ("f" + n) if args.reverse else results[n]["fn"][1]
            reader.drain()
            print(f"\n▶ 再单独按一下 F{n} 验证", flush=True)
            with QuietWait():
                _, got = reader.wait_press()
            mark = "✓" if got == want else "✗"
            bad += got != want
            print(f"  {mark} F{n} → {got}(期望 {want})")
        if bad:
            print(f"{bad} 项未即时生效——重启一次即可(hwdb 在设备重新枚举时套用)。")
        else:
            print("\n全部生效 ✓ 无需任何后台进程,重启后依然有效。")
    except KeyboardInterrupt:
        print("\n已取消。")
    finally:
        reader.stop()


if __name__ == "__main__":
    main()
