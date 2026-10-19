// npu.rs — DpuSession：一次装载、多次执行（xrun-rs 单发模式与 vcam 流式管线共用）
// 执行序列与已验证的 main.rs（2026-10-17，5 模型与 C++ xrun2 逐位一致）完全相同：
//   deserialize → root/children → device=="DPU" → attrs["lib"] 后门 → create_runner_with_attrs
//   → get_input/output_tensors（槽 5/6）→ get_inputs/get_outputs（槽 7/8）
//   → data+sync_for_write → execute_async（槽 2）+ wait（槽 3）→ sync_for_read + data 回读

use crate::abi::*;
use crate::profile::{Profile, Slots};
use std::alloc::alloc;
use std::ffi::c_void;
use std::mem::{size_of, transmute, MaybeUninit};
use std::process::exit;

#[allow(dead_code)]
pub struct OutMeta {
    pub elems: i64,
    pub bits: usize,
    pub shape: Vec<i32>,
    pub bytes: usize,
}

pub struct DpuSession {
    runner: *mut c_void,
    in_buf: *mut c_void,
    in_bytes: usize,
    in_shape: Vec<i32>,
    out_bufs: Vec<*mut c_void>,
    /// 输出张量元信息（与 out_bufs 同序）
    pub outs: Vec<OutMeta>,
    slots: Slots,
    /// execute_async 传参用的 vector 壳（指向栈上数组的稳定表示，每次调用重建）
    _keep: Vec<*mut c_void>,
    /// 每帧打印执行细节（单发模式 true，流式 false）
    verbose: bool,
}

impl DpuSession {
    /// `handle`：profile::open_target 返回的 dlopen 句柄（进程内先过指纹守卫）。
    /// 所有 C++ 对象有意泄漏（进程生命期），无 Drop。
    pub fn open(model: &str, prof: &Profile, handle: *mut c_void, verbose: bool) -> DpuSession {
        unsafe {
            let graph_deserialize: unsafe extern "C" fn(*mut Ret16, *const CxxString) =
                transmute(dlsym_or_die(handle, &prof.symbols.graph_deserialize));
            let graph_root: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
                transmute(dlsym_or_die(handle, &prof.symbols.graph_root));
            let sg_children: unsafe extern "C" fn(*mut CxxVec, *mut c_void) =
                transmute(dlsym_or_die(handle, &prof.symbols.sg_children));
            let sg_has_attr: unsafe extern "C" fn(*mut c_void, *const CxxString) -> bool =
                transmute(dlsym_or_die(handle, &prof.symbols.sg_has_attr));
            // get_attr 返回 std::any（16B 非平凡）→ sret：rdi=ret, rsi=subgraph, rdx=&name
            let sg_get_attr: unsafe extern "C" fn(*mut CxxAny, *mut c_void, *const CxxString) =
                transmute(dlsym_or_die(handle, &prof.symbols.sg_get_attr));
            // any_cast<std::string>(const any&) → std::string（sret：rdi=ret, rsi=&any）
            let any_cast_str: unsafe extern "C" fn(*mut CxxString, *const CxxAny) =
                transmute(dlsym_or_die(handle, &prof.symbols.any_cast_str));
            let sg_name: unsafe extern "C" fn(*mut CxxString, *mut c_void) =
                transmute(dlsym_or_die(handle, &prof.symbols.sg_name));
            let t_shape: unsafe extern "C" fn(*mut CxxVec, *mut c_void) =
                transmute(dlsym_or_die(handle, &prof.symbols.t_shape));
            let t_elems: unsafe extern "C" fn(*mut c_void) -> i64 =
                transmute(dlsym_or_die(handle, &prof.symbols.t_elems));
            let t_dtype: unsafe extern "C" fn(*mut c_void) -> i32 =
                transmute(dlsym_or_die(handle, &prof.symbols.t_dtype));
            let t_bits: unsafe extern "C" fn(*mut c_void) -> i32 =
                transmute(dlsym_or_die(handle, &prof.symbols.t_bits));
            let attrs_create: unsafe extern "C" fn(*mut Ret16) =
                transmute(dlsym_or_die(handle, &prof.symbols.attrs_create));
            let runner_create_attrs: unsafe extern "C" fn(*mut Ret16, *mut c_void, *mut c_void) =
                transmute(dlsym_or_die(handle, &prof.symbols.runner_create_attrs));

            // ── 反序列化 xmodel ──
            let mut gret = MaybeUninit::<Ret16>::zeroed();
            let path_str = CxxString::new(model.as_bytes());
            graph_deserialize(gret.as_mut_ptr(), &path_str);
            let graph = gret.assume_init().obj;
            if verbose {
                println!("graph={:p}", graph);
            }
            if graph.is_null() {
                eprintln!("Graph::deserialize returned null");
                exit(1);
            }

            let root = graph_root(graph);
            let mut kids = CxxVec::null();
            sg_children(&mut kids, root);
            let n_kids = kids.elem_count(size_of::<usize>());
            if verbose {
                println!("root children={}", n_kids);
            }

            // attrs["lib"] = {"DPU": "libgraph-engine.so"} —— 官方后门：
            // 模型 runner map={"run":"librt-engine.so"} 不在 1.8 valid-libs 名单，
            // 必须改指内置 graph-engine。any(map) 全用合并库导出的真实机器构造。
            let mut aret = MaybeUninit::<Ret16>::zeroed();
            attrs_create(aret.as_mut_ptr());
            let attrs = aret.assume_init().obj;
            {
                let map_init_ctor: unsafe extern "C" fn(
                    *mut c_void,      // this
                    *const PairSS,    // initializer_list::_M_array（按值 16B 拆两寄存器）
                    usize,            // initializer_list::_M_len
                    *const c_void,    // const less<string>&（空类，哑指针即可）
                    *const c_void,    // const allocator&（空类，哑指针即可）
                ) = transmute(dlsym_or_die(handle, &prof.symbols.map_init_ctor));
                let map_manager: *mut c_void = dlsym_or_die(handle, &prof.symbols.any_manager_mapss);
                let attrs_set_attr: unsafe extern "C" fn(
                    *mut c_void,
                    *const CxxString,
                    *const CxxAny,
                ) = transmute(dlsym_or_die(handle, &prof.symbols.attrs_set_attr));

                let pairs: [PairSS; 1] = [PairSS {
                    key: CxxString::new(b"DPU"),
                    val: CxxString::new(b"libgraph-engine.so"),
                }];
                // map 对象 48B；泄漏（attrs 生命期=进程）
                let map_buf: *mut c_void =
                    alloc(std::alloc::Layout::from_size_align(48, 8).unwrap()) as *mut c_void;
                let dummy: u8 = 0;
                map_init_ctor(
                    map_buf,
                    pairs.as_ptr(),
                    1,
                    &dummy as *const u8 as *const c_void,
                    &dummy as *const u8 as *const c_void,
                );
                let lib_any = CxxAny { manager: map_manager, storage: map_buf };
                let lib_key = CxxString::new(b"lib");
                attrs_set_attr(attrs, &lib_key, &lib_any);
            }

            // ── 找 device=="DPU" 的子图（必须先读 device 再调 create_runner_with_attrs：
            //    缺 "runner" attr 的子图会跌进旧 DpuRunner 路径 glog-FATAL）──
            let mut runner: *mut c_void = std::ptr::null_mut();
            let dev_key = CxxString::new(b"device");
            for i in 0..n_kids {
                let kid = kids.ptr_at(i);
                let mut name = MaybeUninit::<CxxString>::zeroed();
                sg_name(name.as_mut_ptr(), kid);
                let name = name.assume_init();
                let nm = String::from_utf8_lossy(&read_cxx_string(&name)).into_owned();
                let mut dev = String::new();
                if sg_has_attr(kid, &dev_key) {
                    let mut any = MaybeUninit::<CxxAny>::zeroed();
                    sg_get_attr(any.as_mut_ptr(), kid, &dev_key);
                    let any = any.assume_init();
                    let mut s = MaybeUninit::<CxxString>::zeroed();
                    any_cast_str(s.as_mut_ptr(), &any);
                    let s = s.assume_init();
                    dev = String::from_utf8_lossy(&read_cxx_string(&s)).into_owned();
                }
                if verbose {
                    println!("  subgraph[{}] {} [device={}]", i, nm, dev);
                }
                if dev == "DPU" {
                    let mut rret = MaybeUninit::<Ret16>::zeroed();
                    runner_create_attrs(rret.as_mut_ptr(), kid, attrs);
                    runner = rret.assume_init().obj;
                    if runner.is_null() {
                        eprintln!("create_runner_with_attrs returned null for DPU subgraph");
                        exit(1);
                    }
                    break;
                }
            }
            if runner.is_null() {
                eprintln!("no DPU subgraph");
                exit(1);
            }

            // ── 张量元信息（虚槽位 5/6）──
            let mut in_tensors = CxxVec::null();
            vcall_sret_vec(runner, prof.slots.runner_get_input_tensors, &mut in_tensors);
            let mut out_tensors = CxxVec::null();
            vcall_sret_vec(runner, prof.slots.runner_get_output_tensors, &mut out_tensors);
            let n_in = in_tensors.elem_count(size_of::<usize>());
            let n_out = out_tensors.elem_count(size_of::<usize>());

            let mut in_bits = 8usize;
            let mut in_elems = 0i64;
            let mut in_shape = Vec::new();
            for i in 0..n_in {
                let t = in_tensors.ptr_at(i);
                let mut sh = CxxVec::null();
                t_shape(&mut sh, t);
                let dims: Vec<i32> = (0..sh.elem_count(4)).map(|d| sh.i32_at(d)).collect();
                if verbose {
                    println!(
                        "input tensor: elems={} dtype={} bits={} shape={:?}",
                        t_elems(t),
                        t_dtype(t),
                        t_bits(t),
                        dims
                    );
                }
                if i == 0 {
                    in_bits = { let b = t_bits(t); if b > 0 { b as usize } else { 8 } };
                    in_elems = t_elems(t);
                    in_shape = dims;
                }
            }
            let mut outs = Vec::new();
            for i in 0..n_out {
                let t = out_tensors.ptr_at(i);
                let mut sh = CxxVec::null();
                t_shape(&mut sh, t);
                let dims: Vec<i32> = (0..sh.elem_count(4)).map(|d| sh.i32_at(d)).collect();
                let bits = { let b = t_bits(t); if b > 0 { b as usize } else { 8 } };
                let elems = t_elems(t);
                if verbose {
                    println!(
                        "output tensor: elems={} dtype={} bits={} shape={:?}",
                        elems,
                        t_dtype(t),
                        t_bits(t),
                        dims
                    );
                }
                outs.push(OutMeta { elems, bits, shape: dims, bytes: elems as usize * (bits / 8) });
            }

            // ── 引擎自有缓冲区（虚槽位 7/8；外部缓冲会导致输出全 0）──
            let mut in_bufs = CxxVec::null();
            vcall_sret_vec(runner, prof.slots.runner_get_inputs, &mut in_bufs);
            let mut out_bufs_v = CxxVec::null();
            vcall_sret_vec(runner, prof.slots.runner_get_outputs, &mut out_bufs_v);
            let n_in_bufs = in_bufs.elem_count(size_of::<usize>());
            let n_out_bufs = out_bufs_v.elem_count(size_of::<usize>());
            if verbose {
                println!("engine buffers: {} in, {} out", n_in_bufs, n_out_bufs);
            }
            let out_bufs: Vec<*mut c_void> = (0..n_out_bufs).map(|i| out_bufs_v.ptr_at(i)).collect();

            DpuSession {
                runner,
                in_buf: in_bufs.ptr_at(0),
                in_bytes: in_elems as usize * (in_bits / 8),
                in_shape,
                out_bufs,
                outs,
                slots: prof.slots.clone(),
                _keep: Vec::new(),
                verbose,
            }
        }
    }

    #[allow(dead_code)]
    pub fn input_bytes(&self) -> usize {
        self.in_bytes
    }

    /// 第 0 输入张量的 shape（NHWC）
    pub fn input_shape(&self) -> Vec<i32> {
        self.in_shape.clone()
    }

    /// 单次推理：input 长度不符时零填充并告警（与 xrun2 单发语义一致）。
    pub fn run(&mut self, input: &[u8]) -> Vec<Vec<u8>> {
        let need = self.in_bytes;
        let mut buf = input.to_vec();
        if buf.len() != need {
            eprintln!(
                "input size mismatch: given={} expected={} — zero-padded",
                buf.len(),
                need
            );
            buf.resize(need, 0);
        }
        unsafe {
            // 输入上载：data()（空 index 向量）取引擎缓冲指针，memcpy 后 sync_for_write
            let dp = data_fn_generic(self.in_buf, self.slots.tb_data);
            std::ptr::copy_nonoverlapping(buf.as_ptr(), dp.addr as *mut u8, need);
            let sync_w: unsafe extern "C" fn(*mut c_void, u64, usize) =
                transmute(vslot(self.in_buf, self.slots.tb_sync_for_write));
            sync_w(self.in_buf, 0, need);

            // execute_async（槽 2）+ wait（槽 3）
            let in_items = [self.in_buf];
            let out_items = [self.out_bufs[0]];
            let in_v = CxxVec::from_slice(&in_items);
            let out_v = CxxVec::from_slice(&out_items);
            let exec_fn: unsafe extern "C" fn(
                *mut c_void,
                *const CxxVec,
                *const CxxVec,
            ) -> JobInfo = transmute(vslot(self.runner, self.slots.runner_execute_async));
            let job = exec_fn(self.runner, &in_v, &out_v);
            let wait_fn: unsafe extern "C" fn(*mut c_void, i32, i32) -> i32 =
                transmute(vslot(self.runner, self.slots.runner_wait));
            let r = wait_fn(self.runner, job.jobid as i32, -1);
            if self.verbose {
                println!("execute_async -> jobid={} status={}", job.jobid, job.status);
                println!("wait -> {}", r);
            }
            if r != 0 {
                eprintln!("DPU wait returned {}", r);
                exit(1);
            }

            // 回读输出：sync_for_read 后 data() 取指针
            let mut result = Vec::with_capacity(self.outs.len());
            for (oi, meta) in self.outs.iter().enumerate() {
                let bufp = self.out_bufs[oi];
                let sync_r: unsafe extern "C" fn(*mut c_void, u64, usize) =
                    transmute(vslot(bufp, self.slots.tb_sync_for_read));
                sync_r(bufp, 0, meta.bytes);
                let dp = data_fn_generic(bufp, self.slots.tb_data);
                let bytes = std::slice::from_raw_parts(dp.addr as *const u8, meta.bytes);
                result.push(bytes.to_vec());
            }
            result
        }
    }
}
