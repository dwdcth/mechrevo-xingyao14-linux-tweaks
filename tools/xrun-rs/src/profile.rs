// profile.rs — ABI profile（原子包：库指纹 + 符号名 + vtable 槽位）+ 指纹守卫
// （自 main.rs 原样搬出；设计说明见 profile/default.toml 头部注释）

use std::env;
use std::ffi::c_void;
use std::fs;

use std::process::exit;

/// 内置默认 profile（构建时嵌入二进制；实测来源注释见该文件头）
pub const DEFAULT_PROFILE: &str = include_str!("../profile/default.toml");

/// 魔法环境值（引擎侧行为，非 profile；run.sh 已设置）
pub const ENV_FIRMWARE: &str = "XLNX_VART_FIRMWARE";
/// 指纹校验跳过键；值必须精确 "TRUE"（大写、长度 4）。
/// 依据：合并库 0x522d73 处 cmpq $0x4,len + cmpl $0x45555254（"TRUE" 逐字节），
/// "1"/"true" 均不识别 → graph_runner.cpp:424 FATAL
pub const ENV_SKIP_FP_CHK: &str = "XLNX_VART_SKIP_FP_CHK";
pub const ENV_SKIP_FP_CHK_TRUE: &str = "TRUE";

#[derive(Clone)]
pub struct Slots {
    pub runner_execute_async: usize,
    pub runner_wait: usize,
    pub runner_get_input_tensors: usize,
    pub runner_get_output_tensors: usize,
    pub runner_get_inputs: usize,
    pub runner_get_outputs: usize,
    pub tb_data: usize,
    pub tb_sync_for_read: usize,
    pub tb_sync_for_write: usize,
}

#[derive(Clone)]
pub struct Symbols {
    pub graph_deserialize: String,
    pub graph_root: String,
    pub sg_has_attr: String,
    pub sg_get_attr: String,
    pub sg_children: String,
    pub sg_name: String,
    pub t_shape: String,
    pub t_elems: String,
    pub t_dtype: String,
    pub t_bits: String,
    pub attrs_create: String,
    pub attrs_set_attr: String,
    pub runner_create_attrs: String,
    pub map_init_ctor: String,
    pub any_manager_mapss: String,
    pub any_cast_str: String,
}

#[derive(Clone)]
pub struct Profile {
    pub target_lib: String,
    pub build_id: String,
    pub slots: Slots,
    pub symbols: Symbols,
}

// —— 严格 TOML 子集解析（只允许：[表] / key = "字符串" / key = 整数 / 整行 # 注释 / 空行）——
// 未知表/未知键/缺键/语法残缺一律报错：profile 是安全机制，宁可 fail-fast 也不静默回退默认值。
pub fn parse_profile(text: &str, src: &str) -> Result<Profile, String> {
    let mut section = String::new();
    let mut fp_lib: Option<String> = None;
    let mut fp_id: Option<String> = None;
    let mut slots_map: std::collections::BTreeMap<String, usize> = Default::default();
    let mut sym_map: std::collections::BTreeMap<String, String> = Default::default();
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |msg: &str| Err(format!("{}:{}: {}", src, ln + 1, msg));
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            if !["fingerprint", "slots", "symbols"].contains(&section.as_str()) {
                return err(&format!("未知表 [{section}]（只允许 fingerprint/slots/symbols）"));
            }
            continue;
        }
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| format!("{}:{}: 缺少 '='（本解析器只支持整行 # 注释）", src, ln + 1))?;
        let key = k.trim().to_string();
        let val = v.trim();
        match section.as_str() {
            "fingerprint" => {
                let s = unquote(val).ok_or_else(|| format!("{}:{}: {} 需为字符串", src, ln + 1, key))?;
                match key.as_str() {
                    "target_lib" => fp_lib = Some(s),
                    "build_id" => fp_id = Some(s),
                    _ => return err(&format!("fingerprint 未知键 {key}")),
                }
            }
            "slots" => {
                let n: usize = val
                    .parse()
                    .map_err(|_| format!("{}:{}: {} 需为非负整数", src, ln + 1, key))?;
                if ![
                    "runner_execute_async", "runner_wait", "runner_get_input_tensors",
                    "runner_get_output_tensors", "runner_get_inputs", "runner_get_outputs",
                    "tb_data", "tb_sync_for_read", "tb_sync_for_write",
                ]
                .contains(&key.as_str())
                {
                    return err(&format!("slots 未知键 {key}（拼错键名会静默用默认槽位 → 危险，严格拒绝）"));
                }
                slots_map.insert(key, n);
            }
            "symbols" => {
                let s = unquote(val).ok_or_else(|| format!("{}:{}: {} 需为字符串", src, ln + 1, key))?;
                if ![
                    "graph_deserialize", "graph_root", "sg_has_attr", "sg_get_attr",
                    "sg_children", "sg_name", "t_shape", "t_elems", "t_dtype", "t_bits",
                    "attrs_create", "attrs_set_attr", "runner_create_attrs",
                    "map_init_ctor", "any_manager_mapss", "any_cast_str",
                ]
                .contains(&key.as_str())
                {
                    return err(&format!("symbols 未知键 {key}"));
                }
                sym_map.insert(key, s);
            }
            _ => return err("键出现在任何 [表] 之前"),
        }
    }
    // 完整性：缺任何一角 → profile 不完整，拒绝
    let target_lib = fp_lib
        .ok_or_else(|| "profile 不完整：缺 fingerprint.target_lib（用 tools/make_profile.py 重新生成）".to_string())?;
    let build_id = fp_id
        .ok_or_else(|| "profile 不完整：缺 fingerprint.build_id（用 tools/make_profile.py 重新生成）".to_string())?;
    if build_id.len() != 40 || !build_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("fingerprint.build_id 需为 40 位十六进制（sha1），当前: {build_id}"));
    }
    let need_slot = |m: &std::collections::BTreeMap<String, usize>, k: &str| -> Result<usize, String> {
        m.get(k).copied().ok_or_else(|| format!("profile 不完整：缺 slots.{k}（用 tools/make_profile.py 重新生成）"))
    };
    let slots = Slots {
        runner_execute_async: need_slot(&slots_map, "runner_execute_async")?,
        runner_wait: need_slot(&slots_map, "runner_wait")?,
        runner_get_input_tensors: need_slot(&slots_map, "runner_get_input_tensors")?,
        runner_get_output_tensors: need_slot(&slots_map, "runner_get_output_tensors")?,
        runner_get_inputs: need_slot(&slots_map, "runner_get_inputs")?,
        runner_get_outputs: need_slot(&slots_map, "runner_get_outputs")?,
        tb_data: need_slot(&slots_map, "tb_data")?,
        tb_sync_for_read: need_slot(&slots_map, "tb_sync_for_read")?,
        tb_sync_for_write: need_slot(&slots_map, "tb_sync_for_write")?,
    };
    let need_sym = |m: &std::collections::BTreeMap<String, String>, k: &str| -> Result<String, String> {
        m.get(k).cloned().ok_or_else(|| format!("profile 不完整：缺 symbols.{k}（用 tools/make_profile.py 重新生成）"))
    };
    let symbols = Symbols {
        graph_deserialize: need_sym(&sym_map, "graph_deserialize")?,
        graph_root: need_sym(&sym_map, "graph_root")?,
        sg_has_attr: need_sym(&sym_map, "sg_has_attr")?,
        sg_get_attr: need_sym(&sym_map, "sg_get_attr")?,
        sg_children: need_sym(&sym_map, "sg_children")?,
        sg_name: need_sym(&sym_map, "sg_name")?,
        t_shape: need_sym(&sym_map, "t_shape")?,
        t_elems: need_sym(&sym_map, "t_elems")?,
        t_dtype: need_sym(&sym_map, "t_dtype")?,
        t_bits: need_sym(&sym_map, "t_bits")?,
        attrs_create: need_sym(&sym_map, "attrs_create")?,
        attrs_set_attr: need_sym(&sym_map, "attrs_set_attr")?,
        runner_create_attrs: need_sym(&sym_map, "runner_create_attrs")?,
        map_init_ctor: need_sym(&sym_map, "map_init_ctor")?,
        any_manager_mapss: need_sym(&sym_map, "any_manager_mapss")?,
        any_cast_str: need_sym(&sym_map, "any_cast_str")?,
    };
    Ok(Profile { target_lib, build_id, slots, symbols })
}

fn unquote(v: &str) -> Option<String> {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Some(v[1..v.len() - 1].to_string())
    } else {
        None
    }
}

/// 载入 profile（默认内置或 --profile 外置）+ 守卫 + dlopen 目标库。
/// 返回 (profile, dlopen handle)。任何失败都 exit（fail-fast）。
pub fn open_target(profile_path: Option<&str>) -> (Profile, *mut c_void) {
    // ── 0. 载入 ABI profile ──
    let (text, src) = match profile_path {
        Some(p) => (
            fs::read_to_string(p).unwrap_or_else(|e| {
                eprintln!("读取 profile {} 失败: {}", p, e);
                exit(2);
            }),
            p.to_string(),
        ),
        None => (DEFAULT_PROFILE.to_string(), "<内置 profile/default.toml>".to_string()),
    };
    let prof = match parse_profile(&text, &src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[profile-guard] profile 解析失败: {}\n  用 tools/make_profile.py 重新生成", e);
            exit(2);
        }
    };
    println!("profile: {} (target={}, slots/symbols 已校验)", src, prof.target_lib);

    // ── 1. 按 profile 装载目标运行库（RAI 1.8：xir+vart+graph-engine 合并库）──
    let handle = {
        let c = std::ffi::CString::new(prof.target_lib.as_str()).unwrap();
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL) };
        if h.is_null() {
            eprintln!(
                "dlopen failed: {} (需在 LD_LIBRARY_PATH，如 RAI 1.8 voe/lib 或本目录 libs/)\n  last error: {}",
                prof.target_lib,
                unsafe {
                    std::ffi::CStr::from_ptr(libc::dlerror())
                        .to_string_lossy()
                        .into_owned()
                }
            );
            exit(1);
        }
        println!("loaded {}", prof.target_lib);
        h
    };

    // ── 1b. 指纹守卫：加载的库必须与 profile 同一构建，否则拒绝（fail-fast）──
    verify_profile_fingerprint(&prof);

    // ── 1c. 魔法环境值预检 ──
    match env::var(ENV_SKIP_FP_CHK) {
        Ok(v) if v == ENV_SKIP_FP_CHK_TRUE => {}
        other => eprintln!(
            "[env-hint] {}={:?}（需精确 {}，\"1\" 无效）——指纹校验不过时引擎会直接 FATAL",
            ENV_SKIP_FP_CHK, other, ENV_SKIP_FP_CHK_TRUE
        ),
    }
    if env::var(ENV_FIRMWARE).is_err() {
        eprintln!("[env-hint] {} 未设置——graph-engine 将找不到固件 xclbin", ENV_FIRMWARE);
    }

    (prof, handle)
}

// ───────────── build-id 读取（dl_iterate_phdr + PT_NOTE 解析） ─────────────

unsafe extern "C" {
    fn dl_iterate_phdr(
        cb: unsafe extern "C" fn(*mut libc::dl_phdr_info, libc::size_t, *mut c_void) -> libc::c_int,
        data: *mut c_void,
    ) -> libc::c_int;
}

struct BuildIdHit {
    name: String,
    id: Vec<u8>, // 原始 20B；空 = 该对象没有 GNU build-id note
}

unsafe extern "C" fn phdr_callback(
    info: *mut libc::dl_phdr_info,
    _size: libc::size_t,
    data: *mut c_void,
) -> libc::c_int {
    const PT_NOTE: u32 = 4;
    const NT_GNU_BUILD_ID: u32 = 3;
    let info = &*info;
    let name = std::ffi::CStr::from_ptr(info.dlpi_name).to_string_lossy().into_owned();
    // 目标名来自 profile（GUARD_TARGET，单线程 CLI 在校验前设置）
    let target: &[u8] = unsafe { GUARD_TARGET };
    if target.is_empty() || !name.as_bytes().windows(target.len()).any(|w| w == target) {
        return 0; // 继续遍历
    }
    let mut hit = BuildIdHit { name, id: Vec::new() };
    // ET_DYN：段内容位于 dlpi_addr + p_vaddr；PT_NOTE 里逐条解析 Elf64_Note
    for i in 0..info.dlpi_phnum as usize {
        let ph = &*info.dlpi_phdr.add(i);
        if ph.p_type != PT_NOTE {
            continue;
        }
        let mut cur = (info.dlpi_addr + ph.p_vaddr) as *const u8;
        let end = cur.add(ph.p_memsz as usize);
        while cur.add(12) <= end {
            let namesz = (cur as *const u32).read_unaligned() as usize;
            let descsz = (cur as *const u32).add(1).read_unaligned() as usize;
            let ntype = (cur as *const u32).add(2).read_unaligned();
            let name_field = cur.add(12);
            let desc = name_field.add(namesz.div_ceil(4) * 4);
            if desc.add(descsz) <= end
                && ntype == NT_GNU_BUILD_ID
                && namesz >= 4
                && std::slice::from_raw_parts(name_field, 4) == b"GNU\0"
            {
                hit.id = std::slice::from_raw_parts(desc, descsz).to_vec();
                break;
            }
            cur = desc.add(descsz.div_ceil(4) * 4);
        }
        if !hit.id.is_empty() {
            break;
        }
    }
    *(data as *mut Option<BuildIdHit>) = Some(hit);
    return 1; // 找到目标，停止遍历
}

/// 校验已装载库的 build-id 与 profile 指纹一致（原子包检查）。
/// 不匹配/读不到 → 拒绝运行（exit 2，非警告）。
pub fn verify_profile_fingerprint(prof: &Profile) {
    let mut hit: Option<BuildIdHit> = None;
    unsafe { dl_iterate_phdr(phdr_callback_named(prof.target_lib.as_str()), &mut hit as *mut _ as *mut c_void) };
    let Some(hit) = hit else {
        eprintln!(
            "[profile-guard] dl_iterate_phdr 未找到与 profile 指纹 target_lib='{}' 匹配的已加载对象 — 拒绝运行",
            prof.target_lib
        );
        exit(2);
    };
    let actual: String = hit.id.iter().map(|b| format!("{:02x}", b)).collect();
    if actual != prof.build_id {
        eprintln!(
            "[profile-guard] BUILD-ID MISMATCH — 拒绝运行\n\
             \x20 已加载: {} ({})\n\
             \x20 profile: {}\n\
             \x20 profile 是原子包（指纹+符号+槽位必须同源）：加载的库与 profile 不属于同一构建，\n\
             \x20 槽位可能漂移（症状：派发到错误函数/段错误/静默输出 0），继续跑不安全。\n\
             \x20 请实测生成新 profile: python3 tools/make_profile.py <libvitis-ai-runtime.so> 新.toml\n\
             \x20（或按 README 用 tools/vt_slots.py 手工重测），再以 --profile 新.toml 运行。",
            hit.name,
            if actual.is_empty() { "<无 GNU build-id note>".to_string() } else { actual },
            prof.build_id
        );
        exit(2);
    }
    println!(
        "[profile-guard] {} build-id OK ({})",
        hit.name.rsplit('/').next().unwrap_or(&hit.name),
        prof.build_id
    );
}

// dl_iterate_phdr 回调需要目标名（从 profile 来）：存进全局 leak 的 &'static [u8]
// （单线程 CLI，校验前设置一次，无竞争）
static mut GUARD_TARGET: &[u8] = b"";

fn phdr_callback_named(target: &str) -> unsafe extern "C" fn(*mut libc::dl_phdr_info, libc::size_t, *mut c_void) -> libc::c_int {
    let leaked: &'static [u8] = Box::leak(target.as_bytes().to_vec().into_boxed_slice());
    unsafe { GUARD_TARGET = leaked; }
    phdr_callback
}
