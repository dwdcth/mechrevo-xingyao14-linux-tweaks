// abi.rs — libstdc++/VART C++ ABI 最小复刻（自 main.rs 原样搬出，语义见各注释）
// 全部为已验证（2026-10-17，id101/109/113/119/132 与 C++ xrun2 逐位一致）的约定。

use std::alloc::alloc;
use std::ffi::c_void;
use std::mem::{size_of, transmute};
use std::process::exit;

/// fork 的 unique_ptr<T, CustomDeleter>：16 字节 {对象指针, 删除器指针}，非平凡 → sret 约定
#[repr(C)]
pub struct Ret16 {
    pub obj: *mut c_void,
    pub deleter: *mut c_void,
}

/// libstdc++ std::string（x86_64）: {char* p; size_t len; union{char buf[16]; size_t cap}}
#[repr(C)]
pub struct CxxString {
    p: *mut u8,
    len: usize,
    buf: [u8; 16],
}

impl CxxString {
    /// 一律堆分配（不用 SSO）：SSO 指针自引用 struct 内部，Rust 按值返回会移动导致悬空；
    /// 堆变体对任意长度都是合法表示，const 读方只碰 p/len。
    /// 堆变体的 capacity 字段只写不读（const 引用场景读方只碰 p/len）。
    /// 故意不释放：析构会调 operator delete(p, cap+1)，与 Rust 分配器不匹配 → 泄漏 N+1 字节无妨。
    pub fn new(s: &[u8]) -> Self {
        let n = s.len();
        unsafe {
            let layout = std::alloc::Layout::from_size_align(n + 1, 8).unwrap();
            let p = alloc(layout);
            std::ptr::copy_nonoverlapping(s.as_ptr(), p, n);
            *p.add(n) = 0;
            let mut c = CxxString { p, len: n, buf: [0; 16] };
            c.buf[..size_of::<usize>()].copy_from_slice(&(n as u64).to_ne_bytes());
            c
        }
    }
}

pub fn read_cxx_string(c: &CxxString) -> Vec<u8> {
    let sso = c.p as *const u8 == c.buf.as_ptr() as *const u8;
    unsafe {
        if sso {
            c.buf[..c.len].to_vec()
        } else {
            std::slice::from_raw_parts(c.p, c.len).to_vec()
        }
    }
}

/// libstdc++ std::vector<T>：{begin, end, cap} 三指针；返回值非平凡 → sret
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CxxVec {
    pub b: *mut c_void,
    pub e: *mut c_void,
    pub c: *mut c_void,
}

impl CxxVec {
    pub fn null() -> Self {
        CxxVec { b: std::ptr::null_mut(), e: std::ptr::null_mut(), c: std::ptr::null_mut() }
    }
    pub fn elem_count(&self, elem: usize) -> usize {
        (self.e as usize - self.b as usize) / elem
    }
    pub fn ptr_at(&self, i: usize) -> *mut c_void {
        unsafe { *(self.b as *mut *mut c_void).add(i) }
    }
    pub fn i32_at(&self, i: usize) -> i32 {
        unsafe { *(self.b as *mut i32).add(i) }
    }
    /// 从元素指针数组构造单/多元素 vector（用于 execute_async 传参）
    pub fn from_slice(items: &[*mut c_void]) -> Self {
        CxxVec {
            b: items.as_ptr() as *mut c_void,
            e: unsafe { items.as_ptr().add(items.len()) } as *mut c_void,
            c: unsafe { items.as_ptr().add(items.len()) } as *mut c_void,
        }
    }
}

/// libstdc++ 新版 std::any（反汇编 any_cast<mapSS>@0x5884c0 实证）：
///   offset 0 = manager 函数指针；offset 8 = storage（external 管理器下为堆上 T*）
#[repr(C)]
pub struct CxxAny {
    pub manager: *mut c_void,
    pub storage: *mut c_void,
}

/// pair<const std::string, std::string>：两个 32B string 肩并肩
#[repr(C)]
pub struct PairSS {
    pub key: CxxString,
    pub val: CxxString,
}

/// 平凡 POD 对：SysV 下 rax:rdx / rax 返回，Rust repr(C) 结构体同分类
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DataPair {
    pub addr: u64,
    pub size: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct JobInfo {
    pub jobid: u32,
    pub status: i32,
}

pub unsafe fn dlsym_or_die(h: *mut c_void, name: &str) -> *mut c_void {
    let c = std::ffi::CString::new(name).unwrap();
    let p = libc::dlsym(h, c.as_ptr());
    if p.is_null() {
        eprintln!("dlsym failed: {}", name);
        exit(1);
    }
    p
}

// ───────────── 虚调用（vtable 槽位派发；槽位值来自 ABI profile） ─────────────

pub unsafe fn vslot(obj: *mut c_void, slot: usize) -> *mut c_void {
    let vt = *(obj as *mut *mut *mut c_void);
    *vt.add(slot)
}

/// sret 型虚调用：rdi=返回缓冲, rsi=this
pub unsafe fn vcall_sret_vec(obj: *mut c_void, slot: usize, ret: *mut CxxVec) {
    let f: unsafe extern "C" fn(*mut CxxVec, *mut c_void) = transmute(vslot(obj, slot));
    f(ret, obj);
}

/// data() 槽位调用的独立封装（空 index 向量 = 整块）
pub unsafe fn data_fn_generic(buf: *mut c_void, tb_data_slot: usize) -> DataPair {
    let empty_idx = CxxVec::null();
    let f: unsafe extern "C" fn(*mut c_void, *const CxxVec) -> DataPair =
        transmute(vslot(buf, tb_data_slot));
    f(buf, &empty_idx)
}
