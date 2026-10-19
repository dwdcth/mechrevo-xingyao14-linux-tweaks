//! mechrevo-ctl — 机械革命星耀 14 (P916F-STX / ITE IT5571) 控制工具
//!
//! 三条通路（零第三方依赖，纯 std）：
//!   EC MMIO   /dev/mem   @ 0xFEEC2300 (XRAM 0x0300-0x03FF 窗口)
//!   PMC2      /dev/port  @ 0x68/0x6C   (充电阈值，协议来自 YMGPwcca/Mechrevo-Xingyao-14)
//!   WMAA      huawei-wmi debugfs       (固件官方 WMI 通道)
//!
//! 安全：写入白名单 + DMI 板型校验；charge 只用已验证命令族。

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::process::exit;

const EC_BASE: u64 = 0xFEEC_2300;
// 已验证偏移（窗口内）
const OFF_ECMV: u64 = 0x00;
const OFF_ECSV: u64 = 0x01;
const OFF_ACIN: u64 = 0x30;
const OFF_LIDS: u64 = 0x31;
const OFF_TPKF: u64 = 0x32; // bit0 Turbo, bit1 Fn锁
const OFF_KBBL: u64 = 0x33; // 键盘背光
const OFF_FNS0: u64 = 0x3B;
const OFF_FNS1: u64 = 0x3D;
const OFF_FTVL: u64 = 0x3F;
const WRITABLE: [u64; 2] = [OFF_KBBL, OFF_TPKF];

const EXPECTED: (u8, u8) = (0x01, 0x0F); // ECMV/ECSV

const USAGE: &str = "\
mechrevo-ctl — 星耀14 控制 (EC/充电/WMI)

用法: sudo mechrevo-ctl <命令>
  info                     综合状态（EC/电池/模式/风扇/背光/充电阈值）
  fan                      双风扇数值（单位未标定，非认证 RPM）
  kbd get|set <0-2>      键盘背光三档 0关/1暗/2亮 (EC 0x33)
  fnlock get|set 0|1       Fn 键锁定 (EC 0x32 bit1)
  turbo                    Turbo 状态读数 (EC 0x32 bit0)
  mode                     性能模式值 FTVL (EC 0x3F)
  charge                   充电阈值状态 (PMC2)
  charge restore [T1] [T2] 幂等恢复充电上限（默认 80/100）
  charge set <T1> <T2>     设置并使能
  wmi <mfid> <sfid> [param]  原始 WMAA 调用（16进制）
  perf get                 WMI 性能模式 GPFM
  perf set 1|2             WMI 性能模式 SPFM（带 ALIB 功耗墙联动）
  osd                      FN键 OSD 守护（触控板/键盘灯/麦克风/飞行模式/性能模式/刷新率）
";

// ---------- EC MMIO ----------

struct Ec {
    f: File,
}

impl Ec {
    fn open() -> Result<Self, String> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/mem")
            .map_err(|e| format!("打开 /dev/mem 失败: {e}（需要 sudo）"))?;
        Ok(Ec { f })
    }
    fn rd(&mut self, off: u64) -> Result<u8, String> {
        let mut b = [0u8; 1];
        self.f
            .read_exact_at(&mut b, EC_BASE + off)
            .map_err(|e| format!("EC 读 0x{off:02X} 失败: {e}"))?;
        Ok(b[0])
    }
    fn wr(&mut self, off: u64, val: u8) -> Result<(), String> {
        if !WRITABLE.contains(&off) {
            return Err(format!("偏移 0x{off:02X} 不在写入白名单"));
        }
        self.f
            .write_all_at(&[val], EC_BASE + off)
            .map_err(|e| format!("EC 写 0x{off:02X} 失败: {e}"))
    }
    fn rd16(&mut self, off: u64) -> Result<u16, String> {
        Ok(u16::from_le_bytes([self.rd(off)?, self.rd(off + 1)?]))
    }
}

// DMI 板型校验（EC 与 osd 共用；换机 fail-closed，防同格式日志误触发）
fn board_check() -> Result<(), String> {
    let board = std::fs::read_to_string("/sys/devices/virtual/dmi/id/board_name")
        .unwrap_or_default();
    if !(board.contains("XINGYAO") || board.contains("P916F")) {
        return Err(format!("板型不匹配: {board}"));
    }
    Ok(())
}

fn check_safety(ec: &mut Ec) -> Result<(), String> {
    board_check()?;
    let (cmv, csv) = (ec.rd(OFF_ECMV)?, ec.rd(OFF_ECSV)?);
    if (cmv, csv) != EXPECTED {
        return Err(format!(
            "EC 版本 0x{cmv:02X}.0x{csv:02X} != 预期 0x{:02X}.0x{:02X}",
            EXPECTED.0, EXPECTED.1
        ));
    }
    Ok(())
}

// ---------- PMC2 充电 ----------

struct Pmc2 {
    f: File,
}

impl Pmc2 {
    fn open() -> Result<Self, String> {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/port")
            .map_err(|e| format!("打开 /dev/port 失败: {e}（需要 sudo）"))?;
        Ok(Pmc2 { f })
    }
    fn pin(&mut self, port: u64) -> Result<u8, String> {
        let mut b = [0u8; 1];
        self.f
            .read_exact_at(&mut b, port)
            .map_err(|e| format!("端口 0x{port:02X} 读失败: {e}"))?;
        Ok(b[0])
    }
    fn pout(&mut self, port: u64, v: u8) -> Result<(), String> {
        self.f
            .write_all_at(&[v], port)
            .map_err(|e| format!("端口 0x{port:02X} 写失败: {e}"))
    }
    fn wait_ibf(&mut self) -> bool {
        for _ in 0..2_000_000 {
            if self.pin(0x6C).map(|s| s & 0x02 == 0).unwrap_or(false) {
                return true;
            }
        }
        false
    }
    fn wait_obf(&mut self) -> bool {
        for _ in 0..2_000_000 {
            if self.pin(0x6C).map(|s| s & 0x01 != 0).unwrap_or(false) {
                return true;
            }
        }
        false
    }
    /// PMC2 命令。每条命令前先排空残留应答（设置类命令也有回包！）
    fn cmd(&mut self, cmd: u8, data: u8, want_read: bool) -> Result<Option<u8>, String> {
        while self.pin(0x6C).unwrap_or(0) & 0x01 != 0 {
            let _ = self.pin(0x68);
        }
        if !self.wait_ibf() {
            return Err("IBF 超时".into());
        }
        self.pout(0x6C, cmd)?;
        if !self.wait_ibf() {
            return Err("IBF 超时(2)".into());
        }
        self.pout(0x68, data)?;
        if want_read {
            if !self.wait_obf() {
                return Err("OBF 超时".into());
            }
            return Ok(Some(self.pin(0x68)?));
        }
        Ok(None)
    }
    fn read_state(&mut self) -> Result<(u8, u8, u8), String> {
        let s = self.cmd(0xF1, 0x12, true)?.ok_or("state 无应答")?;
        let t1 = self.cmd(0xF1, 0x13, true)?.ok_or("T1 无应答")?;
        let t2 = self.cmd(0xF1, 0x14, true)?.ok_or("T2 无应答")?;
        Ok((s, t1, t2))
    }
    fn restore(&mut self, t1v: u8, t2v: u8) -> Result<(), String> {
        let (s, t1, t2) = self.read_state()?;
        if s == 1 && t1 == t1v && t2 == t2v {
            println!("已是目标状态 T1={t1v}/T2={t2v}，无需写入");
            return Ok(());
        }
        if t1 != t1v {
            println!("T1 -> {t1v} (回包 {:?})", self.cmd(0xF2, t1v, true)?);
        }
        if t2 != t2v {
            println!("T2 -> {t2v} (回包 {:?})", self.cmd(0xF3, t2v, true)?);
        }
        let (s, t1, t2) = self.read_state()?;
        if t1 != t1v || t2 != t2v {
            return Err(format!("阈值验证失败: {t1}/{t2}"));
        }
        if s != 1 {
            self.cmd(0xF1, 0x11, false)?;
        }
        let (s, t1, t2) = self.read_state()?;
        if s != 1 {
            return Err(format!("使能验证失败: state={s}"));
        }
        println!("已恢复: state=1 T1={t1}% T2={t2}%");
        Ok(())
    }
}

// ---------- WMAA (huawei-wmi debugfs) ----------

const HWMI_ARG: &str = "/sys/kernel/debug/huawei-wmi/arg";
const HWMI_CALL: &str = "/sys/kernel/debug/huawei-wmi/call";

fn wmi_call(mfid: u8, sfid: u8, param: u8) -> Result<[u8; 8], String> {
    let val: u64 = mfid as u64 | ((sfid as u64) << 8) | ((param as u64) << 16);
    {
        let mut f = OpenOptions::new()
            .write(true)
            .open(HWMI_ARG)
            .map_err(|e| format!("打开 arg 失败: {e}（需要 sudo / huawei_wmi 模块）"))?;
        let s = format!("0x{val:x}\n");
        f.write_all(s.as_bytes()).map_err(|e| e.to_string())?;
    }
    let mut f = File::open(HWMI_CALL).map_err(|e| format!("打开 call 失败: {e}"))?;
    let mut txt = String::new();
    f.read_to_string(&mut txt).map_err(|e| e.to_string())?;
    // 解析第二个 {0x..,0x..} 中的前 8 字节
    let mut out = [0u8; 8];
    let mut braces = 0usize;
    let mut got = 0usize;
    let b: Vec<char> = txt.chars().collect();
    let mut i = 0usize;
    while i + 3 < b.len() {
        if b[i] == '{' {
            braces += 1;
        }
        if braces == 2 && b[i] == '0' && b[i + 1] == 'x' && got < 8 {
            let hs: String = b[i + 2..i + 4].iter().collect();
            if let Ok(v) = u8::from_str_radix(&hs, 16) {
                out[got] = v;
                got += 1;
                i += 3;
            }
        }
        i += 1;
    }
    if got == 0 {
        return Err("call 无输出".into());
    }
    Ok(out)
}

// ---------- main ----------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] == "help" || args[1] == "-h" {
        print!("{USAGE}");
        return;
    }
    let cmd = args[1].as_str();
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
    let a3 = args.get(3).map(|s| s.as_str()).unwrap_or("");
    let a4 = args.get(4).map(|s| s.as_str()).unwrap_or("");

    let r = dispatch(cmd, sub, a3, a4);
    if let Err(e) = r {
        eprintln!("错误: {e}");
        exit(1);
    }
}


fn dispatch(cmd: &str, sub: &str, a3: &str, _a4: &str) -> Result<(), String> {
    // --- 纯 WMI 命令（不需要 /dev/mem） ---
    match cmd {
        "wmi" => {
            let mfid = u8::from_str_radix(sub, 16).map_err(|_| "mfid 需16进制")?;
            let sfid = u8::from_str_radix(a3, 16).map_err(|_| "sfid 需16进制")?;
            let param: u8 = if _a4.is_empty() {
                0
            } else {
                u8::from_str_radix(_a4, 16).map_err(|_| "param 需16进制")?
            };
            let d = wmi_call(mfid, sfid, param)?;
            println!("STAT=0x{:02X} 数据: {:02X?}", d[0], &d[1..]);
            return Ok(());
        }
        "perf" => {
            if sub.is_empty() || sub == "get" {
                let d = wmi_call(0x03, 0x09, 0)?;
                println!("WMI 性能模式 GPFM: {} (STAT=0x{:02X})", d[1], d[0]);
            } else if sub == "set" {
                let v: u8 = if a3 == "2" { 2 } else { 1 };
                let d = wmi_call(0x03, 0x0A, v)?;
                println!("SPFM 模式{v} 已发送 (STAT=0x{:02X})", d[0]);
            }
            return Ok(());
        }
        // --- FN键 OSD 守护（不需要 root，需 udev uaccess 规则） ---
        "osd" => return osd_daemon(),
        _ => {}
    }

    // --- 充电（/dev/port） ---
    if cmd == "charge" {
        let mut p = Pmc2::open()?;
        return match sub {
            "" | "status" => {
                let (s, t1, t2) = p.read_state()?;
                println!("state={s} T1={t1}% T2={t2}% ({})", if s == 1 { "已启用" } else { "未启用" });
                Ok(())
            }
            "restore" => {
                let t1: u8 = if a3.is_empty() { 80 } else { a3.parse().map_err(|_| "T1 数字")? };
                let t2: u8 = if _a4.is_empty() { 100 } else { _a4.parse().map_err(|_| "T2 数字")? };
                p.restore(t1, t2)
            }
            "set" => {
                let t1: u8 = a3.parse().map_err(|_| "T1 数字")?;
                let t2: u8 = _a4.parse().map_err(|_| "T2 数字")?;
                if t1 > 100 || t2 > 100 || t1 > t2 {
                    return Err("需 0..100 且 T1<=T2（T1>T2 是未验证路径）".into());
                }
                p.restore(t1, t2)
            }
            _ => Err("charge 子命令: status|restore|set".into()),
        };
    }

    // --- EC MMIO 命令 ---
    let mut ec = Ec::open()?;
    match cmd {
        "info" => {
            check_safety(&mut ec)?;
            let (ac, lid) = (ec.rd(OFF_ACIN)?, ec.rd(OFF_LIDS)?);
            println!("EC 固件: 0x{:02X}.0x{:02X}", ec.rd(OFF_ECMV)?, ec.rd(OFF_ECSV)?);
            println!("电源: {} | 盖: {}", if ac & 0x40 != 0 { "AC在线" } else { "电池" }, if lid & 0x02 != 0 { "开" } else { "合" });
            let tp = ec.rd(OFF_TPKF)?;
            println!("Turbo: {} | Fn锁: {}", if tp & 1 != 0 { "开" } else { "关" }, if tp & 2 != 0 { "开" } else { "关" });
            println!("性能模式 FTVL: 0x{:02X}", ec.rd(OFF_FTVL)?);
            println!("风扇: {} / {}", ec.rd16(OFF_FNS0)?, ec.rd16(OFF_FNS1)?);
            println!("键盘背光: {}/255", ec.rd(OFF_KBBL)?);
            if let Ok((s, t1, t2)) = Pmc2::open().and_then(|mut p| p.read_state()) {
                println!("充电上限: {} (T1={t1}% T2={t2}%)", if s == 1 { "启用" } else { "未启用" });
            }
            Ok(())
        }
        "fan" => {
            println!("{}\n{}", ec.rd16(OFF_FNS0)?, ec.rd16(OFF_FNS1)?);
            Ok(())
        }
        "kbd" => {
            match sub {
                "" | "get" => {
                    println!("{}", ec.rd(OFF_KBBL)?);
                }
                "set" => {
                    check_safety(&mut ec)?;
                    let v: u8 = a3.parse::<u8>().map_err(|_| "0-2")?.min(2);
                    ec.wr(OFF_KBBL, v)?;
                    println!("键盘背光 -> {v}");
                }
                _ => return Err("kbd get|set <0-2>".into()),
            }
            Ok(())
        }
        "fnlock" => {
            match sub {
                "" | "get" => {
                    println!("{}", (ec.rd(OFF_TPKF)? >> 1) & 1);
                }
                "set" => {
                    check_safety(&mut ec)?;
                    let v: u8 = if a3 == "1" { 1 } else { 0 };
                    let cur = ec.rd(OFF_TPKF)?;
                    ec.wr(OFF_TPKF, (cur & 0xFD) | (v << 1))?;
                    println!("Fn锁 -> {}", if v == 1 { "开" } else { "关" });
                }
                _ => return Err("fnlock get|set 0|1".into()),
            }
            Ok(())
        }
        "turbo" => {
            println!("{}", ec.rd(OFF_TPKF)? & 1);
            Ok(())
        }
        "mode" => {
            println!("{}", ec.rd(OFF_FTVL)?);
            Ok(())
        }
        _ => {
            print!("{USAGE}");
            Ok(())
        }
    }
}

// ---------- FN键 OSD 守护 ----------
//
// EC 功能键由 WMI 事件 ABBC0F5C 上报，但 huawei_wmi 的 sparse_keymap 对
// 未映射码只打内核日志（"Unknown key pressed"）、不上报 evdev——
// 事件设备上读不到任何东西，唯一通道是 /dev/kmsg。
// 事件码实测（2026-10-17/18）：
//   0x30/0x31  FN+F3 触控板 关/开（码即状态）
//   0x20/21/22 FN+F8 键盘灯 关/暗/亮（码即档位）
//   0x50/0x51  FN+Esc Fn锁 关/开
//   0xa1       FN+F7 麦克风
//   0xa0       FN+F9 飞行模式
//   0x41/0x42  FN+X 性能模式（EC 直通 SMU 功耗墙：均衡27W/性能38W实测；
//              另联动 powerdevil setProfile 解除 amd_pmf 钳制，挂件显示同步）
//   0xa2       FN+R 刷新率（EC 不动手：kscreen-doctor 60↔120 切换 + OSD）
//
// OSD 走 plasmashell 原生 org.kde.osdService，图标/动画与系统一致。
// 需 root 运行（dmesg_restrict=1），经用户总线地址调用会话 DBus。

// by-path 符号链接 -> eventN -> /sys 里对应的 inputNN（序号随启动变化，勿硬编码）
fn osd_input_idx() -> Result<String, String> {
    let ev = std::fs::read_link("/dev/input/by-path/platform-huawei-wmi-event")
        .map_err(|e| format!("by-path 链接: {e}"))?;
    let ev = ev.file_name().and_then(|s| s.to_str()).ok_or("event 名")?.to_string();
    let l = std::fs::read_link(format!("/sys/class/input/{ev}"))
        .map_err(|e| format!("/sys/class/input/{ev}: {e}"))?;
    let s = l.to_string_lossy();
    let idx = s.rsplit("/input/").next().unwrap_or("").split('/').next().unwrap_or("");
    if idx.starts_with("input") {
        Ok(idx.to_string())
    } else {
        Err(format!("input 序号解析失败: {s}"))
    }
}

// 事件码 -> 动作/OSD 载荷
//   0x20/21/22 FN+F8 键盘灯（EC 已执行，仅 OSD；连按合并）
//   0x30/0x31  FN+F3 触控板（EC 已执行，仅 OSD；连按合并）
//   0x50/0x51  FN+Esc Fn锁 关/开（EC 已执行，仅 OSD；连按合并）
//   0xa1       FN+F7 麦克风（EC 不动手：PipeWire 切静音 + OSD，逐次执行）
//   0xa0       FN+F9 飞行模式（EC 不动手：rfkill 全局开关 + OSD，逐次执行）
//   0x41/0x42  FN+X 性能模式（EC 已执行，仅 OSD；连按合并）
//   0xa2       FN+R 刷新率（EC 不动手：60↔120Hz 切换 + OSD，逐次执行）
enum Cat {
    Kbd(i32),
    Tp(bool),
    Fn(bool),
    Mic,
    Air,
    Mode(bool),
    Refresh,
}

fn osd_parse(line: &str, tag: &str) -> Option<Cat> {
    let p = line.find(tag)?;
    let hex = line.get(p + tag.len()..p + tag.len() + 4)?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    match v {
        0x30 => Some(Cat::Tp(false)),
        0x31 => Some(Cat::Tp(true)),
        0x20 => Some(Cat::Kbd(0)),
        0x21 => Some(Cat::Kbd(50)),
        0x22 => Some(Cat::Kbd(100)),
        0x50 => Some(Cat::Fn(false)),
        0x51 => Some(Cat::Fn(true)),
        0x41 => Some(Cat::Mode(false)), // FTVL 1 = 均衡
        0x42 => Some(Cat::Mode(true)),  // FTVL 2 = 性能
        0xa1 => Some(Cat::Mic),
        0xa0 => Some(Cat::Air),
        0xa2 => Some(Cat::Refresh),
        _ => None,
    }
}

// /run/user/<uid>（单用户机取第一个）
fn user_rundir() -> Option<String> {
    std::fs::read_dir("/run/user").ok()?.flatten().map(|e| e.path().display().to_string()).find(|p| std::path::Path::new(&format!("{p}/bus")).exists())
}

// 用户会话总线通用异步调用（不阻塞事件循环；顺便收割已退出的子进程防僵尸）
fn dbus_call(dest: &str, path: &str, iface: &str, method: &str, sig: &str, args: &[&str], kids: &mut Vec<std::process::Child>) {
    let Some(rd) = user_rundir() else { return };
    let addr = format!("unix:path={rd}/bus");
    kids.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
    match std::process::Command::new("busctl")
        .arg(format!("--address={addr}"))
        .args(["call", dest, path, iface, method, sig])
        .args(args)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => kids.push(c),
        Err(e) => eprintln!("dbus {method} 启动失败: {e}"),
    };
}

// Fn+X 联动 KDE 电源模式：powerdevil setProfile 走官方通道——
// 挂件显示同步、amd_pmf 钳制解除，且无需 root
fn profile_set(prof: &str, kids: &mut Vec<std::process::Child>) {
    dbus_call(
        "org.kde.Solid.PowerManagement",
        "/org/kde/Solid/PowerManagement/Actions/PowerProfile",
        "org.kde.Solid.PowerManagement.Actions.PowerProfile",
        "setProfile",
        "s",
        &[prof],
        kids,
    );
}

fn osd_call_args(method: &str, sig: &str, args: &[&str], kids: &mut Vec<std::process::Child>) {
    dbus_call("org.kde.plasmashell", "/org/kde/osdService", "org.kde.osdService", method, sig, args, kids);
}

fn osd_call(method: &str, sig: &str, val: &str, kids: &mut Vec<std::process::Child>) {
    osd_call_args(method, sig, &[val], kids);
}

// FN+F7 麦克风静音：切默认源，读回状态配 OSD（需 XDG_RUNTIME_DIR 指向用户运行时）
fn mic_toggle(kids: &mut Vec<std::process::Child>) {
    let Some(rd) = user_rundir() else { return };
    let mut wp = |args: &[&str]| {
        std::process::Command::new("/usr/bin/wpctl")
            .args(args)
            .env("XDG_RUNTIME_DIR", &rd)
            .output()
    };
    let _ = wp(&["set-mute", "@DEFAULT_AUDIO_SOURCE@", "toggle"]);
    let muted = wp(&["get-volume", "@DEFAULT_AUDIO_SOURCE@"])
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("[MUTED]"))
        .unwrap_or(false);
    if muted {
        osd_call_args("showText", "ss", &["audio-input-microphone-muted", "麦克风 已静音"], kids);
    } else {
        osd_call_args("showText", "ss", &["audio-input-microphone", "麦克风 已开启"], kids);
    }
}

// FN+F9 飞行模式：有任一无线电开启 → 全部 block，否则全部 unblock（rfkill 需 CAP_NET_ADMIN）
fn air_toggle(kids: &mut Vec<std::process::Child>) {
    let mut any_on = false;
    if let Ok(rd) = std::fs::read_dir("/sys/class/rfkill") {
        for e in rd.flatten() {
            let soft = std::fs::read_to_string(e.path().join("soft")).unwrap_or_default();
            let hard = std::fs::read_to_string(e.path().join("hard")).unwrap_or_default();
            if soft.trim() == "0" && hard.trim() == "0" {
                any_on = true;
            }
        }
    }
    let act = if any_on { "block" } else { "unblock" };
    let _ = std::process::Command::new("/usr/bin/rfkill").arg(act).arg("all").status();
    osd_call("wifiEnabledChanged", "b", if any_on { "false" } else { "true" }, kids);
}

// FN+R 刷新率：EC 只发事件不执行（Windows 由 OEM 控制中心切）。
// 这里接管：读当前模式（kscreen-doctor -o，* 标记），60↔120 互切，分辨率保持。
fn refresh_toggle(kids: &mut Vec<std::process::Child>) {
    let Some(rd) = user_rundir() else { return };
    fn env<'a>(rd: &str, c: &'a mut std::process::Command) -> &'a mut std::process::Command {
        // KScreen 后端跟随 Qt 平台：xcb 在服务环境必死，offscreen 拿不到后端，
        // 必须用 wayland 平台直连 KWin（socket 名随开机变化，动态探测）
        let wl = std::fs::read_dir(rd)
            .ok()
            .and_then(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .find(|n| n.starts_with("wayland-") && !n.ends_with(".lock"))
            })
            .unwrap_or_else(|| "wayland-0".into());
        c.env("XDG_RUNTIME_DIR", rd)
            .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={rd}/bus"))
            .env("QT_QPA_PLATFORM", "wayland")
            .env("WAYLAND_DISPLAY", wl)
    }
    let Ok(o) = env(&rd, &mut std::process::Command::new("/usr/bin/kscreen-doctor")).arg("-o").output()
    else {
        return;
    };
    let txt = String::from_utf8_lossy(&o.stdout).into_owned();
    // 去 ANSI 色码（kscreen-doctor 强制上色）
    let mut clean = String::with_capacity(txt.len());
    let mut esc = false;
    for c in txt.chars() {
        if esc {
            if c == 'm' {
                esc = false;
            }
        } else if c == '\x1b' {
            esc = true;
        } else {
            clean.push(c);
        }
    }
    let mut out_name = String::new();
    let mut cur = String::new(); // "2880x1800@120.00"
    for line in clean.lines() {
        let t = line.trim();
        if out_name.is_empty() && t.contains("Output:") {
            if let Some(n) = t.split_whitespace().nth(2) {
                out_name = n.to_string();
            }
        }
        for tok in t.split_whitespace() {
            // '*'=当前 '*'+'!'可能叠加（60Hz 档同时是 preferred，形如 "@60.00*!"）
            if tok.contains('@') && tok.contains('*') {
                cur = tok.trim_end_matches(|c| c == '*' || c == '!').to_string();
                if let Some(cp) = cur.find(':') {
                    cur = cur[cp + 1..].to_string();
                }
            }
        }
    }
    if out_name.is_empty() || cur.is_empty() {
        eprintln!("mechrevo-osd: 刷新率解析失败: {clean}");
        return;
    }
    let Some(at) = cur.find('@') else { return };
    let (wh, rate_s) = cur.split_at(at);
    let rate: f64 = rate_s[1..].parse().unwrap_or(120.0);
    let target = if rate >= 90.0 { 60 } else { 120 };
    eprintln!("mechrevo-osd: 刷新率 {rate} -> {target} ({out_name} {wh})");
    // .output() 捕获 stdout：status() 会把整个配置打印进 journal
    let applied = env(&rd, &mut std::process::Command::new("/usr/bin/kscreen-doctor"))
        .arg(format!("output.{out_name}.mode.{wh}@{target}"))
        .output();
    if let Err(e) = &applied {
        eprintln!("mechrevo-osd: kscreen-doctor 执行失败: {e}");
    }
    // 等显示重新配置完成（模式切换黑屏）再弹 OSD，否则 OSD 会在黑屏期被吞掉
    std::thread::sleep(std::time::Duration::from_millis(900));    osd_call_args(
        "showText",
        "ss",
        &["preferences-desktop-display", &format!("刷新率 {target} Hz")],
        kids,
    );
}

fn osd_daemon() -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    board_check()?; // 与 EC 子命令对齐（参考 Nocehi/xingyao-oem-mode-osd fail-closed）
    let idx = osd_input_idx()?;
    let tag = format!("{idx}: Unknown key pressed, code: 0x");
    println!("mechrevo-osd: 尾部 /dev/kmsg，过滤 {tag}…（150ms 合并连按）");
    // O_NONBLOCK 轮询（x86-64 = 0x800），单 fd 单游标，无重复投递
    let mut g = OpenOptions::new()
        .read(true)
        .custom_flags(0x800)
        .open("/dev/kmsg")
        .map_err(|e| format!("/dev/kmsg: {e}（需 CAP_SYSLOG 或 root）"))?;
    g.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?; // 跳过历史
    const POLL: std::time::Duration = std::time::Duration::from_millis(50);
    const QUIET: u32 = 3; // 3×50ms 静默 = 150ms 无新事件才发射
    let mut buf = [0u8; 2048];
    let mut kids: Vec<std::process::Child> = Vec::new();
    let (mut kbd, mut tp, mut fnl, mut mode, mut quiet) = (None, None, None, None, 0u32);
    loop {
        let got = match g.read(&mut buf) {
            Ok(n) if n > 0 => {
                let line = String::from_utf8_lossy(&buf[..n]).into_owned();
                osd_parse(&line, &tag)
            }
            _ => None, // EAGAIN：暂无新记录
        };
        if let Some(c) = got {
            match c {
                Cat::Kbd(v) => kbd = Some(v),
                Cat::Tp(v) => tp = Some(v),
                Cat::Fn(v) => fnl = Some(v),
                Cat::Mode(v) => mode = Some(v),
                Cat::Mic => mic_toggle(&mut kids), // 动作键：逐次执行，不走合并
                Cat::Air => air_toggle(&mut kids),
                Cat::Refresh => refresh_toggle(&mut kids),
            }
            quiet = 0;
        } else if kbd.is_some() || tp.is_some() || fnl.is_some() || mode.is_some() {
            quiet += 1;
            if quiet >= QUIET {
                // 连按合并后只发最终态：防止 plasmashell OSD 被高频调用刷残卡屏
                if let Some(v) = kbd.take() {
                    osd_call("keyboardBrightnessChanged", "i", &v.to_string(), &mut kids);
                }
                if let Some(v) = tp.take() {
                    osd_call("touchpadEnabledChanged", "b", if v { "true" } else { "false" }, &mut kids);
                }
                if let Some(v) = fnl.take() {
                    let txt = if v { "Fn锁 已开启" } else { "Fn锁 已关闭" };
                    osd_call_args("showText", "ss", &["input-keyboard", txt], &mut kids);
                }
                if let Some(v) = mode.take() {
                    // 联动 KDE 电源模式：性能→performance、均衡→balanced（power-saver 由用户手动选）
                    profile_set(if v { "performance" } else { "balanced" }, &mut kids);
                    let txt = if v { "性能模式：性能" } else { "性能模式：均衡" };
                    osd_call_args("showText", "ss", &["preferences-system-performance", txt], &mut kids);
                }
                quiet = 0;
            }
        }
        std::thread::sleep(POLL);
    }
}

// read_at/write_at 需要 Seek 语义——为 File 实现小助手
trait ReadExactAt {
    fn read_exact_at(&mut self, buf: &mut [u8], off: u64) -> std::io::Result<()>;
}
trait WriteAllAt {
    fn write_all_at(&mut self, buf: &[u8], off: u64) -> std::io::Result<()>;
}
impl ReadExactAt for File {
    fn read_exact_at(&mut self, buf: &mut [u8], off: u64) -> std::io::Result<()> {
        self.seek(SeekFrom::Start(off))?;
        self.read_exact(buf)
    }
}
impl WriteAllAt for File {
    fn write_all_at(&mut self, buf: &[u8], off: u64) -> std::io::Result<()> {
        self.seek(SeekFrom::Start(off))?;
        self.write_all(buf)
    }
}
