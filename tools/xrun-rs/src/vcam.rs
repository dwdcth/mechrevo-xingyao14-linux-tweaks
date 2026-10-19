// vcam.rs — Windows Studio Effects「背景虚化」管线原生复刻（PerceptionCore 行为按
// docs/win-effects-re.md 逆向常量复刻），推理经 npu::DpuSession 在 NPU 上跑原装 MEP 模型。
//
// 链路（RE 文档 §1.4 / §3.2）：
//   ffmpeg(v4l2, mjpg→bgr24) ─→ id109 CCForegroundSegmentation
//       输入 normed_rgbm (128×224×4 int8)：RGB=帧像素−128，第4通道=上帧 mask 反馈（首帧 0）
//       输出两档 seg：x1(448×256 显示用) + x0.5(224×128 反馈用)
//   ─→ SegmentationMaskRefiner（EMA α=0.3；帧间 RMSE>0.02 重置）
//   ─→ 掩码下采样至工作分辨率 192×108（SHALLOW 384×216）+ 羽化(半径25)
//   ─→ 背景 bokeh（BLUR 核 4.0 / SHALLOW 3.0，可分离 box×3 近似高斯）→ 合成
//   ─→ ffmpeg(bgr24→yuyv422 /dev/videoN 或 png 序列)
//
// 参数：TOML 风格扁平键（Windows 同名键做别名，见 apply_cfg_key）+ CLI 覆盖（CLI 优先于文件）。

use crate::img;
use crate::npu::DpuSession;
use crate::profile;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

// ───────────────────────── 配置 ─────────────────────────

#[derive(Clone, PartialEq, Debug)]
enum BgMode {
    Off,
    Blur,    // KS EXTENDED_BACKGROUNDSEGMENTATION BLUR=1；bokeh σ=4.0，mask 192×108
    Shallow, // SHALLOWFOCUS=4；σ=3.0，mask 384×216
}

#[derive(Clone)]
struct Config {
    bg_mode: BgMode,
    bg_strength: f32,        // 0..1：线性缩放 EMA α 与 bokeh 核（RE §4）
    bg_reset_threshold: f32, // 帧间 RMSE 重置阈值，Windows 默认 0.02
    ema_alpha: f32,          // 0.3（SegmentationMaskRefiner +0x18）
    feather_radius: usize,   // 25（BackgroundEffectsLib +0x10）
    feather_iters: usize,    // 羽化 box 迭代数（默认 1；Windows 常量池含 7，192 宽下 1 次更接近观感）
    seg_wfov: i32,           // Segmentation.WFOV（模型即 WFOV 布局，记录/透传）
    refiner_feathering: bool, // Segmentation.RefinerMode == FEATHERING
    // —— stub 开关（模型在包内但管线未接，仅接受+提示）——
    eye_contact: String,     // off/on/stare（KS EXTENDED_EYEGAZECORRECTION）
    eye_contact_version: i32, // 2 = GAN 路线（id119）
    superres: String,        // none/1x/2x/4x（id132）
    framing: String,         // none/auto/face/full-frame（ActiveDirector.RefinerMode）
    face_threshold: f32,     // FaceDetection.FaceThreshold
    num_faces: i32,
    // —— I/O ——
    model: String,
    profile_path: Option<String>,
    src: String,
    out: String, // v4l2:/dev/video9 | png:DIR | null
    width: usize,
    height: usize,
    fps: u32,
    frames: usize, // 0 = 无限
    dump_dir: Option<String>,
    port: u16, // 控制台端口（默认 8711，0=禁用）
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bg_mode: BgMode::Blur,
            bg_strength: 1.0,
            bg_reset_threshold: 0.02,
            ema_alpha: 0.3,
            feather_radius: 25,
            feather_iters: 1,
            seg_wfov: 1,
            refiner_feathering: true,
            eye_contact: "off".into(),
            eye_contact_version: 2,
            superres: "none".into(),
            framing: "none".into(),
            face_threshold: 0.5,
            num_faces: 1,
            model: "../mep-models/id109.onnx".into(),
            profile_path: None,
            src: "/dev/video0".into(),
            out: "v4l2:/dev/video9".into(),
            width: 1280,
            height: 720,
            fps: 30,
            frames: 0,
            dump_dir: None,
            port: 8711,
        }
    }
}

/// 配置键（配置文件 / CLI 同一套）：Windows 同名扁平键做别名（RE §2.1/§4）
fn apply_cfg_key(cfg: &mut Config, key: &str, val: &str) -> Result<(), String> {
    let k = key.trim().to_ascii_lowercase();
    // 值解析：支持 `"字符串"  # 行尾注释`、`bare  # 注释`、纯 bare
    let v = {
        let t = val.trim();
        if let Some(rest) = t.strip_prefix('"') {
            if let Some(end) = rest.find('"') {
                &rest[..end]
            } else {
                t
            }
        } else {
            t.split('#').next().unwrap_or(t).trim()
        }
    };
    let s = || v.to_ascii_lowercase();
    match k.as_str() {
        "bg-mode" | "backgroundeffects.mode" => {
            // 官方命名（MS Learn Studio Effects）：标准模糊 standard / 人像模糊 portrait
            cfg.bg_mode = match s().as_str() {
                "off" | "0" => BgMode::Off,
                "blur" | "1" | "standard" => BgMode::Blur,
                "shallow" | "shallowfocus" | "portrait" | "4" => BgMode::Shallow,
                other => return Err(format!("bg-mode 非法值 {other:?}（off|blur|standard|shallow|portrait）")),
            };
        }
        "bg-strength" | "backgroundeffects.strength" => {
            cfg.bg_strength = v.parse().map_err(|_| "需为浮点")?;
        }
        "bg-reset-threshold" | "backgroundeffects.resetthreshold" => {
            cfg.bg_reset_threshold = v.parse().map_err(|_| "需为浮点")?;
        }
        "seg-wfov" | "segmentation.wfov" => {
            cfg.seg_wfov = v.parse().map_err(|_| "需为整数")?;
        }
        "segmentation.refinermode" | "refiner" => {
            cfg.refiner_feathering = match s().as_str() {
                "none" => false,
                "feathering" => true,
                other => return Err(format!("refiner 非法值 {other:?}（none|feathering）")),
            };
        }
        "feather-radius" => cfg.feather_radius = v.parse().map_err(|_| "需为整数")?,
        "feather-iters" => cfg.feather_iters = v.parse().map_err(|_| "需为整数")?,
        "eye-contact" | "eyecontact.mode" => {
            // 官方命名：标准 standard / 提词器 teleprompter（KS 旧值 on=1/stare=2 做兼容别名）
            cfg.eye_contact = match s().as_str() {
                "on" | "1" => "standard".to_string(),
                "stare" | "2" => "teleprompter".to_string(),
                other => other.to_string(),
            };
        }
        "eye-contact-version" | "eyecontact.version" => {
            cfg.eye_contact_version = v.parse().map_err(|_| "需为整数")?;
        }
        "superres" | "superres.model" => {
            cfg.superres = s().trim_start_matches("superres_").to_ascii_lowercase();
        }
        "framing" | "activedirector.refinermode" => cfg.framing = s(),
        "face-threshold" | "facedetection.facethreshold" => {
            cfg.face_threshold = v.parse().map_err(|_| "需为浮点")?;
        }
        "num-faces" | "facetracker.numfacestotrack" => {
            cfg.num_faces = v.parse().map_err(|_| "需为整数")?;
        }
        "model" => cfg.model = unquote_or_raw(v).into(),
        "profile" => cfg.profile_path = Some(unquote_or_raw(v).into()),
        "src" => cfg.src = unquote_or_raw(v).into(),
        "out" | "dst" => cfg.out = unquote_or_raw(v).into(),
        "width" => cfg.width = v.parse().map_err(|_| "需为整数")?,
        "height" => cfg.height = v.parse().map_err(|_| "需为整数")?,
        "fps" => cfg.fps = v.parse().map_err(|_| "需为整数")?,
        "frames" => cfg.frames = v.parse().map_err(|_| "需为整数")?,
        "dump-dir" => cfg.dump_dir = Some(unquote_or_raw(v).into()),
        "port" => cfg.port = v.parse().map_err(|_| "需为整数")?,
        _ => return Err(format!("未知键 {key:?}")),
    }
    Ok(())
}

fn unquote_or_raw(v: &str) -> &str {
    let t = v.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

/// 迷你配置解析：扁平 `key = value` 行 + # 注释（与 profile 解析器同风格；未知键/非法值 fail-fast）
fn load_config_file(path: &str) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("读取配置 {} 失败: {}", path, e);
        std::process::exit(2);
    });
    let mut kv = Vec::new();
    for (ln, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            eprintln!("{}:{}: 缺少 '='", path, ln + 1);
            std::process::exit(2);
        };
        kv.push((k.trim().to_string(), v.trim().to_string()));
    }
    kv
}

// ───────────────────────── ffmpeg 子进程 ─────────────────────────

struct Ffmpeg {
    child: std::process::Child,
}

impl Ffmpeg {
    fn spawn(args: &[&str]) -> Ffmpeg {
        let child = Command::new("ffmpeg")
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap_or_else(|e| {
                eprintln!("启动 ffmpeg 失败: {}（args: {:?}）", e, args);
                std::process::exit(1);
            });
        Ffmpeg { child }
    }
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Ffmpeg {
    fn drop(&mut self) {
        self.kill();
    }
}

// ───────────────────────── 管线 ─────────────────────────

struct Pipeline {
    sess: DpuSession,
    in_h: usize,
    in_w: usize,
    hi_idx: usize,
    lo_idx: usize,
    hi_w: usize,
    hi_h: usize,
    // 状态
    prev_ema: Option<Vec<f32>>, // 上帧 EMA 后的显示掩码（hi 分辨率）
    /// 上帧掩码统计 (cx, cy, mass, x0, x1, y0, y1)，归一化 0..1；None=无有效前景
    mask_centroid: Option<(f32, f32, f32, f32, f32, f32, f32)>,
    fb_raw: Vec<f32>,           // 反馈通道原始掩码（lo 分辨率），首帧全 0 = Windows 的 memset 0
    frame_no: usize,
}

impl Pipeline {
    /// 上帧掩码统计（取景窗口用）
    pub fn mask_stats(&self) -> Option<(f32, f32, f32, f32, f32, f32, f32)> {
        self.mask_centroid
    }

    /// 返回 (合成帧 bgr24, [pre_ms, npu_ms, post_ms])
    fn process(
        &mut self,
        frame_bgr: &[u8],
        w: usize,
        h: usize,
        cfg: &Config,
        dump: bool,
    ) -> (Vec<u8>, [f32; 3]) {
        let (work_w, work_h) = match cfg.bg_mode {
            BgMode::Shallow => (384usize, 216usize),
            _ => (192usize, 108usize),
        };
        let t_pre = Instant::now();

        // 1) id109 输入（normed_rgbm，NHWC4 int8）：帧→in_w×in_h；RGB=px−128；
        //    第4通道=上帧原始 mask×255−128（首帧 0×255−128 = −128，即缓冲全 0 语义）
        let small = img::resize_bilinear_u8(frame_bgr, w, h, 3, self.in_w, self.in_h);
        let n = self.in_w * self.in_h;
        let mut input = vec![0u8; n * 4];
        for i in 0..n {
            // ffmpeg 给 bgr24，模型吃 RGB
            input[i * 4] = small[i * 3 + 2].wrapping_sub(128); // R
            input[i * 4 + 1] = small[i * 3 + 1].wrapping_sub(128); // G
            input[i * 4 + 2] = small[i * 3].wrapping_sub(128); // B
            let m = (self.fb_raw[i] * 255.0).round().clamp(0.0, 255.0) as u8;
            input[i * 4 + 3] = m.wrapping_sub(128); // mask 反馈
        }
        let pre_ms = t_pre.elapsed().as_secs_f32() * 1e3;

        // 2) NPU 推理
        let t_npu = Instant::now();
        let outs = self.sess.run(&input);
        let npu_ms = t_npu.elapsed().as_secs_f32() * 1e3;

        let t_post = Instant::now();
        // 3) 反量化 int8 → 软值：(v+128)/255（与输入构造同一恒等映射）。
        //    id109 seg 输出实测为 ~[0.5,0.75] 窄带软值（非饱和 0/1）→ 显示用掩码再做
        //    分位归一化（2%/98%，256 箱直方图 O(n)）拉伸成双峰；反馈通道保持原始字节。
        let deq = |b: &u8| ((*b as i32 + 128) as f32 / 255.0).clamp(0.0, 1.0);
        let raw_lo: Vec<f32> = outs[self.lo_idx].iter().map(deq).collect();
        let mut new_hi: Vec<f32> = outs[self.hi_idx].iter().map(deq).collect();
        img::percentile_normalize_256(&mut new_hi, 2.0, 98.0);

        // 4) SegmentationMaskRefiner：EMA（α 随 bg-strength 线性缩放）+ 帧间 RMSE 重置
        let alpha = (cfg.ema_alpha * cfg.bg_strength).clamp(0.01, 1.0);
        let ema = match &self.prev_ema {
            None => new_hi.clone(), // 首帧
            Some(prev) => {
                let m = prev.len().min(new_hi.len());
                let mut mse = 0f32;
                for i in 0..m {
                    let d = new_hi[i] - prev[i];
                    mse += d * d;
                }
                if (mse / m as f32).sqrt() > cfg.bg_reset_threshold {
                    new_hi.clone() // 场景切换 → 全量重置（Windows 阈值 0.02）
                } else {
                    prev.iter().zip(new_hi.iter()).map(|(&p, &x)| p + alpha * (x - p)).collect()
                }
            }
        };
        // 掩码统计（自动取景用；阈值 0.5 的质心+包围盒+质量）
        {
            let (mut sx, mut sy, mut mass) = (0f32, 0f32, 0f32);
            let (mut x0, mut x1, mut y0, mut y1) = (1f32, 0f32, 1f32, 0f32);
            for (i, &v) in ema.iter().enumerate() {
                if v > 0.5 {
                    let x = (i % self.hi_w) as f32 / self.hi_w as f32;
                    let y = (i / self.hi_w) as f32 / self.hi_h as f32;
                    sx += x;
                    sy += y;
                    mass += 1.0;
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
            self.mask_centroid = if mass / ema.len() as f32 > 0.01 {
                Some((sx / mass, sy / mass, mass / ema.len() as f32, x0, x1, y0, y1))
            } else {
                None
            };
        }
        self.prev_ema = Some(ema.clone());
        // 反馈通道写回原始输出字节（Windows +0x48 成员回写语义；
        // fb_raw*255 roundtrip = 原字节恒等：ch3 = (v+128)−128 = v）
        self.fb_raw = raw_lo;
        self.frame_no += 1;

        // 5) 合成（bg 链与 mask 链并行：std::thread::scope，bg 链占大头）
        let out_frame = if cfg.bg_mode == BgMode::Off {
            frame_bgr.to_vec()
        } else {
            std::thread::scope(|s| {
                // 背景（worker 线程）：帧 → 工作分辨率 → bokeh（σ = 核×strength，box×3
                // 近似高斯）→ 可分离双线性放回原尺寸
                let sigma = match cfg.bg_mode {
                    BgMode::Shallow => 3.0,
                    _ => 4.0,
                } * cfg.bg_strength;
                let bg_handle = s.spawn(move || {
                    // 全平面化：小图去交错 → 每通道 bokeh → 单通道可分离放大（向量化友好）
                    let small_frame = img::resize_bilinear_u8(frame_bgr, w, h, 3, work_w, work_h);
                    let nsmall = work_w * work_h;
                    let mut planes: Vec<Vec<u8>> = Vec::with_capacity(3);
                    for c in 0..3 {
                        let mut p: Vec<f32> = (0..nsmall).map(|i| small_frame[i * 3 + c] as f32).collect();
                        if sigma >= 0.2 {
                            p = img::gauss_approx_f32(&p, work_w, work_h, sigma, 3);
                        }
                        let up = img::resize_bilinear_u8_plane_sep(
                            &(p.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect::<Vec<u8>>()),
                            work_w, work_h, w, h,
                        );
                        planes.push(up);
                    }
                    planes
                });
                // 掩码（当前线程）：hi → 工作分辨率 → 放大到帧尺寸 → 羽化。
                // 羽化半径 25 按【帧分辨率】语义（1280 宽下 2%，Windows 手感；
                // 若在工作分辨率 192 宽下会占 13% → 人物边缘被过度软化，实测验证过）
                let mask_work = img::resize_bilinear_f32(&ema, self.hi_w, self.hi_h, work_w, work_h);
                let mut mask_full = img::resize_bilinear_f32_sep(&mask_work, work_w, work_h, w, h);
                if cfg.refiner_feathering {
                    for _ in 0..cfg.feather_iters {
                        mask_full = img::box_blur_f32(&mask_full, w, h, cfg.feather_radius);
                    }
                }
                let bg_planes = bg_handle.join().expect("bg thread");
                // out = frame·m + bg·(1−m)（整数数学；bg 为平面布局）
                let mut out = vec![0u8; w * h * 3];
                for i in 0..w * h {
                    let m = (mask_full[i] * 255.0).round().clamp(0.0, 255.0) as u32;
                    let im = 255 - m;
                    for c in 0..3 {
                        let f = frame_bgr[i * 3 + c] as u32;
                        let b = bg_planes[c][i] as u32;
                        out[i * 3 + c] = ((f * m + b * im + 127) / 255) as u8;
                    }
                }
                out
            })
        };

        if dump {
            if let Some(dir) = &cfg.dump_dir {
                let _ = std::fs::create_dir_all(dir);
                let fno = self.frame_no - 1;
                let _ = std::fs::write(format!("{}/f{:03}_input.int8", dir, fno), &input);
                let _ = std::fs::write(format!("{}/f{:03}_seg_hi.u8", dir, fno), &outs[self.hi_idx]);
                let _ = std::fs::write(format!("{}/f{:03}_seg_lo.u8", dir, fno), &outs[self.lo_idx]);
                let _ = std::fs::write(format!("{}/f{:03}_frame.bgr", dir, fno), frame_bgr);
                let _ = std::fs::write(format!("{}/f{:03}_out.bgr", dir, fno), &out_frame);
            }
        }
        let post_ms = t_post.elapsed().as_secs_f32() * 1e3;
        (out_frame, [pre_ms, npu_ms, post_ms])
    }
}

// ───────────────────────── 主入口 ─────────────────────────

pub fn main(argv: &[String]) {
    let mut cfg = Config::default();
    // 顺序：内置默认 → --config 文件 → CLI 键值（CLI 优先）
    let mut file_kv: Vec<(String, String)> = Vec::new();
    let mut cli_kv: Vec<(String, String)> = Vec::new();
    let mut rest: Vec<String> = vec![];
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].clone();
        if a == "--help" || a == "-h" {
            print_usage();
            std::process::exit(0);
        }
        let takes_value = [
            "--config", "--bg-mode", "--bg-strength", "--bg-reset-threshold", "--seg-wfov",
            "--refiner", "--feather-radius", "--feather-iters", "--eye-contact",
            "--eye-contact-version", "--superres", "--framing", "--face-threshold",
            "--num-faces", "--model", "--profile", "--src", "--out", "--dst",
            "--width", "--height", "--fps", "--frames", "--dump-dir", "--port",
        ]
        .contains(&a.as_str());
        if takes_value {
            let val = argv.get(i + 1).cloned().unwrap_or_else(|| {
                eprintln!("{} 需要参数值", a);
                std::process::exit(1);
            });
            if a == "--config" {
                file_kv = load_config_file(&val);
            } else {
                cli_kv.push((a.trim_start_matches('-').to_string(), val));
            }
            i += 2;
        } else {
            rest.push(a);
            i += 1;
        }
    }
    if !rest.is_empty() {
        eprintln!("vcam: 无法识别的参数 {:?}", rest);
        print_usage();
        std::process::exit(1);
    }
    for (k, v) in file_kv.into_iter().chain(cli_kv) {
        if let Err(e) = apply_cfg_key(&mut cfg, &k, &v) {
            eprintln!("{} = {}: {}", k, v, e);
            std::process::exit(2);
        }
    }

    // stub 提示
    if cfg.eye_contact != "off" {
        eprintln!("[stub] eye-contact={} 未实现（id119 GazeDirector 管线未接入，仅记录）", cfg.eye_contact);
    }
    if cfg.superres != "none" {
        eprintln!("[stub] superres={} 未实现（id132 管线未接入，仅记录）", cfg.superres);
    }
    if cfg.framing != "none" {
        eprintln!("[stub] framing={} 未实现（ActiveDirector 管线未接入，仅记录）", cfg.framing);
    }

    // NPU 会话（profile 守卫 → id109）
    let (prof, handle) = profile::open_target(cfg.profile_path.as_deref());
    let sess = DpuSession::open(&cfg.model, &prof, handle, true);
    let in_shape = sess.input_shape();
    assert!(in_shape.len() >= 4 && in_shape[3] == 4, "id109 输入需为 NHWC4，got {:?}", in_shape);
    let in_h = in_shape[1] as usize;
    let in_w = in_shape[2] as usize;
    // 输出双档：大=显示（x1，BgBlur 用），小=反馈（x0.5）
    let (hi_idx, lo_idx) = if sess.outs[0].elems >= sess.outs[1].elems { (0, 1) } else { (1, 0) };
    let hi_shape = sess.outs[hi_idx].shape.clone();
    let lo_shape = sess.outs[lo_idx].shape.clone();
    let (hi_h, hi_w) = (hi_shape[1] as usize, hi_shape[2] as usize);
    let (lo_h, lo_w) = (lo_shape[1] as usize, lo_shape[2] as usize);
    println!(
        "id109: in {}×{}×4 (normed_rgbm), seg_hi {}×{} (x1 显示), seg_lo {}×{} (x0.5 反馈)",
        in_w, in_h, hi_w, hi_h, lo_w, lo_h
    );
    eprintln!(
        "[cfg] bg-mode={:?} strength={} reset={} feather={}×{} wfov={} | {}×{}@{} {} → {}",
        cfg.bg_mode, cfg.bg_strength, cfg.bg_reset_threshold, cfg.feather_radius, cfg.feather_iters,
        cfg.seg_wfov, cfg.width, cfg.height, cfg.fps, cfg.src, cfg.out
    );

    let mut pipe = Pipeline {
        fb_raw: vec![0f32; lo_w * lo_h],
        sess,
        in_h,
        in_w,
        hi_idx,
        lo_idx,
        hi_w,
        hi_h,
        prev_ema: None,
        mask_centroid: None,
        frame_no: 0,
    };

    // ffmpeg 捕获：v4l2(mjpeg→bgr24) 或 img:静态图循环（测试用，无需摄像头空闲）
    let (w, h) = (cfg.width, cfg.height);
    let cap_args: Vec<String> = if let Some(imgpath) = cfg.src.strip_prefix("img:") {
        [
            "-hide_banner", "-loglevel", "error",
            "-loop", "1", "-framerate", &cfg.fps.to_string(), "-i", imgpath,
            "-vf", &format!("scale={}:{}", w, h),
            "-f", "rawvideo", "-pix_fmt", "bgr24", "-",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    } else {
        [
            "-hide_banner", "-loglevel", "error",
            "-f", "v4l2", "-input_format", "mjpeg",
            "-framerate", &cfg.fps.to_string(),
            "-video_size", &format!("{}x{}", w, h),
            "-i", &cfg.src,
            "-f", "rawvideo", "-pix_fmt", "bgr24", "-",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    let cap_args_ref: Vec<&str> = cap_args.iter().map(|s| s.as_str()).collect();
    let mut cap = Ffmpeg::spawn(&cap_args_ref);
    let mut cap_out = cap.child.stdout.take().expect("capture stdout");

    // ffmpeg 输出（v4l2loopback 参数组合与 Python 版 vcam_blur.py 实证一致）
    let (out_fmt, out_dst) = match cfg.out.split_once(':') {
        Some((f, d)) => (f.to_string(), d.to_string()),
        // 无冒号：裸 "null" → null；其余按 v4l2 设备路径
        None => {
            if cfg.out == "null" {
                ("null".to_string(), "-".to_string())
            } else {
                ("v4l2".to_string(), cfg.out.clone())
            }
        }
    };
    let head: Vec<String> = [
        "-hide_banner", "-loglevel", "error",
        "-f", "rawvideo", "-pix_fmt", "bgr24",
        "-s", &format!("{}x{}", w, h),
        "-r", &cfg.fps.to_string(),
        "-i", "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let tail: Vec<String> = match out_fmt.as_str() {
        "null" => vec!["-f".into(), "null".into(), "-".into()],
        "png" => vec!["-f".into(), "image2".into(), format!("{}/frame%05d.png", out_dst)],
        _ => vec![
            "-f".into(), "v4l2".into(), "-pix_fmt".into(), "yuyv422".into(), out_dst.clone(),
        ],
    };
    let out_args: Vec<&str> = head.iter().chain(tail.iter()).map(|s| s.as_str()).collect();
    let mut enc = Ffmpeg::spawn(&out_args);
    let mut enc_in = enc.child.stdin.take().expect("encoder stdin");

    // 网页控制台（Python 版同款体验）：Runtime 参数每帧读取（热调参，无需重启管线）
    let init_mode = match cfg.bg_mode {
        BgMode::Off => "passthrough",
        BgMode::Shallow => "portrait",
        BgMode::Blur => "blur",
    };
    let rt = crate::console::Runtime::new(init_mode, cfg.bg_strength, cfg.bg_reset_threshold);
    let preview_tx = crate::console::start(cfg.port, &rt);

    let frame_bytes = w * h * 3;
    let mut frame = vec![0u8; frame_bytes];
    let mut ema = [0f32; 5]; // pre, npu, post, write, total
    let mut ema_init = false;
    let mut t_prev = Instant::now();
    let mut fps_ema = 0f32;
    let mut n_done = 0usize;
    let t_start = Instant::now();
    let mut last_offer = Instant::now();
    let mut framing_state = FramingState::default();

    loop {
        if cfg.frames != 0 && n_done >= cfg.frames {
            break;
        }
        if cap_out.read_exact(&mut frame).is_err() {
            eprintln!("捕获流结束");
            break;
        }
        let t_frame = Instant::now();
        // ── 热参数：每帧从控制台 Runtime 读 ──
        let p = rt.params();
        let eff_mode = match p.mode.as_str() {
            "passthrough" | "off" => BgMode::Off,
            "portrait" | "shallow" => BgMode::Shallow,
            _ => BgMode::Blur,
        };
        let is_passthrough = eff_mode == BgMode::Off;
        let framing_on = p.framing == "auto";
        let mut eff_cfg = cfg.clone();
        eff_cfg.bg_mode = eff_mode;
        eff_cfg.bg_strength = p.strength;
        eff_cfg.bg_reset_threshold = p.reset_threshold;
        // ── 自动取景（原生近似：id109 分割质心/包围盒 + EMA 平滑窗口；
        //    Windows 用 id101/113 人脸框，对齐需反编译检测头解码）──
        let frame_view: Vec<u8> = if framing_on {
            let win = framing_window(&mut framing_state, pipe.mask_stats(), w, h);
            crop_resize(&frame, w, h, win, w, h)
        } else {
            frame.clone()
        };
        // 处理（passthrough 直通 / 特效管线），公共出帧 out_frame 供写出与预览
        let mut break_out = false;
        let out_frame: Vec<u8> = if is_passthrough {
            n_done += 1;
            if enc_in.write_all(&frame_view).is_err() {
                eprintln!("输出管道断裂（检查 {}）", cfg.out);
                break_out = true;
            }
            // passthrough 也更新耗时统计（分量记 0，仅计整帧）
            let now = Instant::now();
            let total_ms = t_frame.elapsed().as_secs_f32() * 1e3;
            let inst = 1.0 / now.duration_since(t_prev).as_secs_f32().max(1e-6);
            t_prev = now;
            if !ema_init {
                ema = [0.0, 0.0, 0.0, 0.0, total_ms];
                fps_ema = inst;
                ema_init = true;
            } else {
                for j in 0..4 {
                    ema[j] *= 0.9;
                }
                ema[4] = ema[4] * 0.9 + total_ms * 0.1;
                fps_ema = fps_ema * 0.9 + inst * 0.1;
            }
            frame_view
        } else {
            let dump = cfg.dump_dir.is_some() && n_done < 3;
            let (out_frame, [pre, npu, post]) = pipe.process(&frame_view, w, h, &eff_cfg, dump);
            if enc_in.write_all(&out_frame).is_err() {
                eprintln!("输出管道断裂（检查 {}）", cfg.out);
                break_out = true;
            }
            n_done += 1;
            let now = Instant::now();
            let total_ms = t_frame.elapsed().as_secs_f32() * 1e3;
            let write_ms = (total_ms - pre - npu - post).max(0.0);
            let inst = 1.0 / now.duration_since(t_prev).as_secs_f32().max(1e-6);
            t_prev = now;
            if !ema_init {
                ema = [pre, npu, post, write_ms, total_ms];
                fps_ema = inst;
                ema_init = true;
            } else {
                let cur = [pre, npu, post, write_ms, total_ms];
                for j in 0..5 {
                    ema[j] = ema[j] * 0.9 + cur[j] * 0.1;
                }
                fps_ema = fps_ema * 0.9 + inst * 0.1;
            }
            if n_done % 60 == 0 {
                let rss = read_vmsrss().map(|k| format!("{:.1} MB", k as f64 / 1024.0)).unwrap_or_else(|| "n/a".into());
                println!(
                    "{} 帧 | pre {:.1} | npu {:.1} | post {:.1} | write {:.1} | 计 {:.1} ms | {:.1} fps | rss {}",
                    n_done, ema[0], ema[1], ema[2], ema[3], ema[4], fps_ema, rss
                );
            }
            out_frame
        };
        if break_out {
            break;
        }
        // 预览投放（公共路径：直通也投，修"点直通预览冻结"；≤10fps 覆盖式）
        if let Some(tx) = &preview_tx {
            if last_offer.elapsed().as_secs_f32() >= 0.1 {
                let small = img::resize_bilinear_u8(&out_frame, w, h, 3, 640, 360);
                let _ = tx.try_send(small);
                last_offer = Instant::now();
            }
        }
        rt.set_stats(fps_ema, ema[4], n_done as u64);
    }

    let el = t_start.elapsed().as_secs_f32();
    println!(
        "完成 {} 帧 / {:.1}s = {:.1} fps（EMA 分解 ms：pre {:.2} | npu {:.2} | post {:.2} | write {:.2}）",
        n_done, el, n_done as f32 / el.max(1e-6), ema[0], ema[1], ema[2], ema[3]
    );
    // 优雅收尾：先关编码器 stdin（flush 尾帧，如 image2/png），再等它退出；捕获端直接 kill
    drop(enc_in);
    let _ = enc.child.wait();
    drop(cap_out);
    cap.kill();
}

fn read_vmsrss() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.trim().split_whitespace().next()?.parse().ok();
        }
    }
    None
}

fn print_usage() {
    eprintln!("用法: xrun-rs vcam [选项]（Windows 背景虚化管线：id109 NPU 分割 + EMA 重整 + 羽化 + bokeh 合成）");
    eprintln!(
        "  --config <f>                TOML 风格配置（扁平键；Windows 同名键做别名，如 Segmentation.WFOV）\n\
         \x20 --bg-mode off|blur|shallow  BLUR=σ4.0/mask192×108；SHALLOW=σ3.0/mask384×216\n\
         \x20 --bg-strength 0..1         线性缩放 EMA α(0.3) 与 bokeh 核\n\
         \x20 --bg-reset-threshold f     帧间 RMSE 重置阈值（Windows 0.02）\n\
         \x20 --seg-wfov 0|1             Segmentation.WFOV\n\
         \x20 --refiner none|feathering  Segmentation.RefinerMode（羽化开关）\n\
         \x20 --feather-radius N         羽化半径（Windows 25）\n\
         \x20 --feather-iters N          羽化迭代（默认 1）\n\
         \x20 --eye-contact off|on|stare [stub]   --superres none|1x|2x|4x [stub]\n\
         \x20 --framing none|auto|face|full-frame [stub]  --face-threshold f [stub]  --num-faces N [stub]\n\
         \x20 --src /dev/video0  --out v4l2:/dev/video9|png:DIR|null\n\
         \x20 --width 1280 --height 720 --fps 30 --frames N（0=无限）\n\
         \x20 --model <id109.onnx> --profile <abi.toml> --dump-dir DIR（前3帧中间量落盘供分析）"
    );
}

// ───────────────────────── 自动取景（原生近似实现） ─────────────────────────
// 窗口 = 分割质心 + 包围盒驱动的缩放，EMA 平滑（OneEuro-lite）；无前景时缓慢回全景。
// 与 Windows 的差距：Windows ActiveDirector 用 id101 人脸框 + FaceTracker 平滑；
// 对齐需反编译 id101 检测头 (1,28,28,70)+(1,4,28) 的解码。

#[derive(Default)]
struct FramingState {
    cx: f32, // 平滑后的窗口中心（归一化）
    cy: f32,
    zoom: f32,
    init: bool,
}

/// 由上帧掩码统计计算平滑取景窗口 (x, y, w, h)（像素，原帧坐标系）
fn framing_window(
    st: &mut FramingState,
    stats: Option<(f32, f32, f32, f32, f32, f32, f32)>,
    w: usize,
    h: usize,
) -> (usize, usize, usize, usize) {
    const ALPHA: f32 = 0.12; // 窗口平滑系数（防抖）
    let (tcx, tcy, tzoom) = match stats {
        Some((cx, cy, mass, x0, x1, y0, y1)) if mass > 0.03 => {
            // 目标窗口：包住人物包围盒并留 30%/45% 余量，限制缩放 1.0~1.6
            let bw = (x1 - x0 + 0.02).clamp(0.30, 1.0);
            let bh = (y1 - y0 + 0.02).clamp(0.30, 1.0);
            let z = (1.0 / (bw * 1.30).min(bh * 1.45)).clamp(1.0, 1.6);
            (cx.clamp(0.5 / z, 1.0 - 0.5 / z), cy.clamp(0.5 / z, 1.0 - 0.5 / z), z)
        }
        _ => (0.5, 0.5, 1.0), // 无前景 → 全景
    };
    if !st.init {
        st.cx = 0.5;
        st.cy = 0.5;
        st.zoom = 1.0;
        st.init = true;
    }
    st.cx += ALPHA * (tcx - st.cx);
    st.cy += ALPHA * (tcy - st.cy);
    st.zoom += ALPHA * (tzoom - st.zoom);
    let vw = (w as f32 / st.zoom).round() as usize;
    let vh = (h as f32 / st.zoom).round() as usize;
    let x = ((st.cx * w as f32 - vw as f32 / 2.0).round().clamp(0.0, (w - vw) as f32)) as usize;
    let y = ((st.cy * h as f32 - vh as f32 / 2.0).round().clamp(0.0, (h - vh) as f32)) as usize;
    (x, y, vw, vh)
}

/// 从原帧裁 (x,y,cw,ch) 子矩形并平面化可分离缩放到 (dw,dh)
fn crop_resize(frame: &[u8], w: usize, h: usize, win: (usize, usize, usize, usize), dw: usize, dh: usize) -> Vec<u8> {
    let (x0, y0, cw, ch) = win;
    if cw == w && ch == h {
        return frame.to_vec();
    }
    // 逐通道：裁剪平面（行切片拷贝）→ 可分离缩放
    let mut planes = Vec::with_capacity(3);
    for c in 0..3 {
        let mut plane = vec![0u8; cw * ch];
        for y in 0..ch {
            let src_row = ((y0 + y) * w + x0) * 3 + c;
            for x in 0..cw {
                plane[y * cw + x] = frame[src_row + x * 3];
            }
        }
        planes.push(img::resize_bilinear_u8_plane_sep(&plane, cw, ch, dw, dh));
    }
    let mut out = vec![0u8; dw * dh * 3];
    for i in 0..dw * dh {
        for c in 0..3 {
            out[i * 3 + c] = planes[c][i];
        }
    }
    out
}

// ───────────────────────── 隐藏微基准：xrun-rs vcam-bench ─────────────────────────
// 测 img.rs 各原语在管线真实尺寸下的耗时（优化依据）。
pub fn bench() {
    let (w, h) = (1280usize, 720usize);
    let (ww, wh) = (192usize, 108usize);
    let frame = vec![100u8; w * h * 3];
    let mask_hi = vec![0.6f32; 448 * 256];
    let plane = vec![100f32; ww * wh];
    let n = 30;
    let mut t = Instant::now();
    let tick = |name: &str, t: &mut Instant| {
        println!("  {:28} {:.2} ms", name, t.elapsed().as_secs_f32() * 1e3 / n as f32);
        *t = Instant::now();
    };
    for _ in 0..n { let _ = crate::img::resize_bilinear_u8(&frame, w, h, 3, ww, wh); }
    tick("resize 720p→192x108 (u8x3)", &mut t);
    for _ in 0..n {
        let b = crate::img::gauss_approx_f32(&plane, ww, wh, 4.0, 3);
        let _ = b;
    }
    tick("bokeh blur 192x108 σ4 boxx3", &mut t);
    let bg_small = vec![100u8; ww * wh * 3];
    for _ in 0..n { let _ = crate::img::resize_bilinear_u8_sep(&bg_small, ww, wh, 3, w, h); }
    tick("bg upscale 192x108→720p sep", &mut t);
    for _ in 0..n { let _ = crate::img::resize_bilinear_f32(&mask_hi, 448, 256, ww, wh); }
    tick("mask hi→work (f32)", &mut t);
    let mut mw = vec![0.5f32; ww * wh];
    for _ in 0..n { mw = crate::img::box_blur_f32(&mw, ww, wh, 25); }
    tick("feather box r25 (f32)", &mut t);
    for _ in 0..n { let _ = crate::img::resize_bilinear_f32_sep(&mw, ww, wh, w, h); }
    tick("mask upscale →720p sep", &mut t);
    // 合成
    let m_full = vec![200f32; w * h];
    let bg = vec![100u8; w * h * 3];
    let mut out = vec![0u8; w * h * 3];
    for _ in 0..n {
        for i in 0..w * h {
            let m = (m_full[i] * 255.0).round().clamp(0.0, 255.0) as u32;
            let im = 255 - m;
            for c in 0..3 {
                let f = frame[i * 3 + c] as u32;
                let b = bg[i * 3 + c] as u32;
                out[i * 3 + c] = ((f * m + b * im + 127) / 255) as u8;
            }
        }
    }
    tick("composite (int)", &mut t);
    let mut v = vec![0f32; 448 * 256];
    for _ in 0..n { crate::img::percentile_normalize_256(&mut v, 2.0, 98.0); }
    tick("percentile norm hi", &mut t);
}
