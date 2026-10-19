// xrun-rs — VART 直执行 voe/XIR xmodel 的 DPU 子图（Rust 版 xrun2，RAI 1.8 栈）
//
// 模块划分：
//   abi.rs     — libstdc++/VART C++ ABI 最小复刻（sret/Ret16/string/vector/any/vcall）
//   profile.rs — ABI profile 原子包（指纹+符号+槽位）+ build-id 守卫
//   npu.rs     — DpuSession（装载/执行序列，单发与流式共用）
//   img.rs     — 图像原语（双线性缩放 / 可分离 box blur / 合成）
//   vcam.rs    — Windows 效果管线原生复刻（背景虚化：id109 + SegmentationMaskRefiner + 羽化 + bokeh）
//
// 方法论与关键结论见 README.md 与 ../mep-run/venv14-NOTES.md。
//
// 用法:
//   xrun-rs [--profile <f>] <model> <in.bin> <out.bin>     # 单发推理（同 xrun2 语义）
//   xrun-rs vcam [--config f] [--bg-mode blur] ...          # 流式背景虚化管线（ffmpeg 管道）
// 运行环境（run.sh 已包办）:
//   LD_LIBRARY_PATH 需含 /opt/xilinx/xrt/lib 与 1.8 voe/lib（或本目录 libs/ 的符号链接）
//   XLNX_VART_FIRMWARE=<AMD_AIE2P_4x4_Overlay_CFG0.xclbin>
//   XLNX_VART_SKIP_FP_CHK=TRUE   ← 必须精确大写 TRUE；"1" 无效
//   sudo prlimit --memlock=8589934592 --（NPU 锁页内存）

mod abi;
mod console;
mod img;
mod json;
mod npu;
mod profile;
mod vcam;

use std::env;
use std::fs;
use std::process::exit;

fn main() {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.first().map(|s| s.as_str()) == Some("vcam") {
        vcam::main(&argv[1..]);
        return;
    }
    if argv.first().map(|s| s.as_str()) == Some("vcam-bench") {
        vcam::bench();
        return;
    }

    // ── 单发模式：xrun-rs [--profile <f>] <model> <in.bin> <out.bin> ──
    let mut argv = argv;
    let mut profile_path: Option<String> = None;
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--profile" => {
                if i + 1 >= argv.len() {
                    eprintln!("--profile 需要一个文件参数");
                    exit(1);
                }
                profile_path = Some(argv[i + 1].clone());
                argv.drain(i..=i + 1);
            }
            "--help" | "-h" => {
                eprintln!("usage: xrun-rs [--profile <abi-profile.toml>] <model> <in.bin> <out.bin>\n       xrun-rs vcam [vcam-options]");
                exit(0);
            }
            _ => i += 1,
        }
    }
    if argv.len() < 3 {
        eprintln!("usage: xrun-rs [--profile <file>] <model> <in.bin> <out.bin>");
        exit(1);
    }
    let model = argv[0].clone();
    let in_path = argv[1].clone();
    let out_path = argv[2].clone();

    let (prof, handle) = profile::open_target(profile_path.as_deref());
    let mut sess = npu::DpuSession::open(&model, &prof, handle, true);

    let input = fs::read(&in_path).unwrap_or_else(|e| {
        eprintln!("cannot open {}: {}", in_path, e);
        exit(1);
    });
    let outputs = sess.run(&input);
    let n_out = outputs.len();
    for (oi, bytes) in outputs.iter().enumerate() {
        let path = if n_out > 1 { format!("{}_{}", out_path, oi) } else { out_path.clone() };
        fs::write(&path, bytes).unwrap_or_else(|e| {
            eprintln!("write {}: {}", path, e);
            exit(1);
        });
        println!("wrote {} bytes to {}", bytes.len(), path);
    }
}
