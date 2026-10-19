// console.rs — vcam 网页控制台（对齐 Python 版 vcam_blur.py 的体验，零依赖）
//
// 架构（与 Python 版同构）：
//   主循环 ──offer(小图, ~10fps 覆盖式)──→ mpsc(容量1, 满则丢弃)
//        → 【编码线程】ffmpeg 子进程（bgr24 rawvideo → mjpeg 640×360）
//          从 stdout 切出完整 JPEG（SOI/EOI 扫描）→ PreviewHub.publish
//   PreviewHub：Mutex<(代数号, 最新 JPEG)> + Condvar；/preview.mjpg 客户端
//   各自等"代数 > 自己已发的"，广播同一份（全管线只有一个 ffmpeg，不随客户端增加）
//   HTTP：std::net::TcpListener + 每连接线程（127.0.0.1:8711，--port 可改）

use crate::json::{self, Json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

// ───────────────────────── 运行时共享参数/统计 ─────────────────────────

#[derive(Clone)]
pub struct Params {
    /// passthrough | blur(标准模糊) | portrait(人像模糊)
    pub mode: String,
    /// 0..1（bg-strength）
    pub strength: f32,
    /// bg-reset-threshold
    pub reset_threshold: f32,
    /// 眼神接触 stub：off | standard | teleprompter（记录；id119 管线未接入）
    pub eye_contact: String,
    /// 自动取景：none | auto（原生近似：分割质心窗口）
    pub framing: String,
}

#[derive(Clone)]
pub struct Stats {
    pub fps: f32,
    pub frame_ms: f32,
    pub frames: u64,
}

pub struct Runtime {
    params: Mutex<Params>,
    stats: Mutex<Stats>,
    hub: PreviewHub,
}

impl Runtime {
    pub fn new(mode: &str, strength: f32, reset_threshold: f32) -> Arc<Runtime> {
        Arc::new(Runtime {
            params: Mutex::new(Params {
                mode: mode.to_string(),
                strength,
                reset_threshold,
                eye_contact: "off".to_string(),
                framing: "none".to_string(),
            }),
            stats: Mutex::new(Stats { fps: 0.0, frame_ms: 0.0, frames: 0 }),
            hub: PreviewHub::new(),
        })
    }

    pub fn params(&self) -> Params {
        self.params.lock().unwrap().clone()
    }

    pub fn set_stats(&self, fps: f32, frame_ms: f32, frames: u64) {
        let mut s = self.stats.lock().unwrap();
        s.fps = fps;
        s.frame_ms = frame_ms;
        s.frames = frames;
    }

    /// 应用一组调参（JSON 对象）；返回 (最新参数, 被拒绝字段列表)
    pub fn set_from_json(&self, body: &str) -> (Result<Params, String>, Vec<String>) {
        let mut rejected: Vec<String> = Vec::new();
        let parsed = match json::parse(body) {
            Ok(Json::Obj(kvs)) => kvs,
            Ok(_) => {
                return (Err("body 必须是 JSON 对象".into()), rejected);
            }
            Err(e) => {
                return (Err(format!("JSON 解析失败: {}", e)), rejected);
            }
        };
        let mut p = self.params.lock().unwrap();
        let mut mode_set = false;
        let mut strength_set = false;
        let mut reset_set = false;
        for (k, v) in parsed {
            let mut key = k.as_str();
            // 别名归一：bg-mode/bg_mode → mode；bg-strength/blur → strength；bg-reset-threshold → reset
            let norm = key.trim_start_matches("bg_").trim_start_matches('-').to_ascii_lowercase();
            let norm = norm.strip_prefix("bg-").unwrap_or(&norm);
            key = match norm {
                "mode" => "mode",
                "strength" | "blur" => "strength",
                "reset-threshold" | "reset_threshold" => "reset-threshold",
                "eye-contact" | "eyecontact" => "eye-contact",
                "framing" => "framing",
                _ => {
                    rejected.push(k);
                    continue;
                }
            };
            match key {
                "eye-contact" | "eyecontact" => {
                    let m = v.as_str().unwrap_or("").to_ascii_lowercase();
                    match m.as_str() {
                        "off" | "standard" | "teleprompter" | "on" | "stare" => {
                            let norm = match m.as_str() {
                                "on" => "standard".to_string(),
                                "stare" => "teleprompter".to_string(),
                                other => other.to_string(),
                            };
                            if p.eye_contact != norm {
                                println!("[控制台] eye-contact: {} → {}（stub：id119 管线未接入，仅记录）", p.eye_contact, norm);
                            }
                            p.eye_contact = norm;
                        }
                        _ => rejected.push(k),
                    }
                }
                "framing" => {
                    let m = v.as_str().unwrap_or("").to_ascii_lowercase();
                    match m.as_str() {
                        "none" | "auto" | "face" | "full-frame" => {
                            let norm = match m.as_str() {
                                "face" | "full-frame" => "auto".to_string(),
                                other => other.to_string(),
                            };
                            if p.framing != norm {
                                println!("[控制台] framing: {} → {}（原生近似：分割质心窗口）", p.framing, norm);
                            }
                            p.framing = norm;
                        }
                        _ => rejected.push(k),
                    }
                }
                "mode" => {
                    let m = v.as_str().unwrap_or("").to_ascii_lowercase();
                    match m.as_str() {
                        "passthrough" | "off" | "blur" | "standard" | "portrait" | "shallow" => {
                            let norm_mode = match m.as_str() {
                                "off" => "passthrough".to_string(),
                                "standard" => "blur".to_string(),
                                "shallow" => "portrait".to_string(),
                                other => other.to_string(),
                            };
                            if !mode_set || p.mode != norm_mode {
                                println!("[控制台] mode: {} → {}", p.mode, norm_mode);
                            }
                            p.mode = norm_mode;
                            mode_set = true;
                        }
                        _ => rejected.push(k),
                    }
                }
                "strength" => match v.as_f64() {
                    Some(x) if (0.0..=1.0).contains(&x) && x.is_finite() => {
                        p.strength = x as f32;
                        strength_set = true;
                    }
                    _ => rejected.push(k),
                },
                "reset-threshold" => match v.as_f64() {
                    Some(x) if (0.0..=1.0).contains(&x) && x.is_finite() && x > 0.0 => {
                        p.reset_threshold = x as f32;
                        reset_set = true;
                    }
                    _ => rejected.push(k),
                },
                _ => unreachable!(),
            }
        }
        let _ = (mode_set, strength_set, reset_set);
        (Ok(p.clone()), rejected)
    }

    pub fn state_json(&self) -> String {
        let p = self.params.lock().unwrap();
        let s = self.stats.lock().unwrap();
        format!(
            "{{\"mode\":\"{}\",\"strength\":{:.3},\"reset_threshold\":{:.3},\"eye_contact\":\"{}\",\"framing\":\"{}\",\"fps\":{:.1},\"frame_ms\":{:.1},\"frames\":{}}}",
            json::escape(&p.mode),
            p.strength,
            p.reset_threshold,
            json::escape(&p.eye_contact),
            json::escape(&p.framing),
            s.fps,
            s.frame_ms,
            s.frames
        )
    }
}

// ───────────────────────── 预览枢纽（最新帧覆盖 + 广播） ─────────────────────────

pub struct PreviewHub {
    state: Mutex<(u64, Arc<Vec<u8>>)>, // (代数号, 最新 JPEG)
    cv: Condvar,
}

impl PreviewHub {
    fn new() -> Self {
        PreviewHub { state: Mutex::new((0, Arc::new(Vec::new()))), cv: Condvar::new() }
    }
    fn publish(&self, jpeg: Vec<u8>) {
        let mut st = self.state.lock().unwrap();
        st.0 += 1;
        st.1 = Arc::new(jpeg);
        self.cv.notify_all();
    }
    /// 等待代数 > after 的最新帧；超时返回 None（连接保活）
    fn wait_after(&self, after: u64, timeout: Duration) -> Option<(u64, Arc<Vec<u8>>)> {
        let mut st = self.state.lock().unwrap();
        let deadline = std::time::Instant::now() + timeout;
        while st.0 <= after || st.1.is_empty() {
            let now = std::time::Instant::now();
            if now >= deadline {
                return None;
            }
            let (guard, _res) = self.cv.wait_timeout(st, deadline - now).unwrap();
            st = guard;
        }
        Some((st.0, st.1.clone()))
    }
}

// ───────────────────────── 启动（编码线程 + HTTP 服务） ─────────────────────────

/// 启动控制台。返回主循环用的投放通道（小图 bgr24 640×360；满则丢弃=最新帧语义）。
/// port=0 时只起预览编码（无 HTTP）。失败则打印并返回 None（视频管线继续）。
pub fn start(port: u16, rt: &Arc<Runtime>) -> Option<SyncSender<Vec<u8>>> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
    // 编码线程：全管线唯一一个 ffmpeg 做 MJPEG
    let rt2 = rt.clone();
    std::thread::Builder::new()
        .name("preview-encode".into())
        .spawn(move || encoder_loop(rx, rt2))
        .ok()?;
    if port == 0 {
        println!("控制台: 禁用（--port 0）",);
        return Some(tx);
    }
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            println!("控制台启动失败 (端口 {} 被占用?): {}; 视频管线继续", port, e);
            return Some(tx);
        }
    };
    let rt3 = rt.clone();
    std::thread::Builder::new()
        .name("http-console".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let rt4 = rt3.clone();
                let _ = std::thread::Builder::new()
                    .name("http-conn".into())
                    .spawn(move || handle_conn(stream, rt4));
            }
        })
        .ok()?;
    println!(
        "控制台: http://127.0.0.1:{}/ （预览 /preview.mjpg | 状态 /api/state | 调参 POST /api/set）",
        port
    );
    Some(tx)
}

fn encoder_loop(rx: std::sync::mpsc::Receiver<Vec<u8>>, rt: Arc<Runtime>) {
    let mut child = match Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error",
            "-f", "rawvideo", "-pix_fmt", "bgr24", "-s", "640x360", "-r", "10",
            "-i", "-",
            // 管道输出必须逐包刷（否则 ffmpeg 全缓冲到 EOF，预览流无帧）
            "-flush_packets", "1",
            "-f", "mjpeg", "-q:v", "4", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("预览编码 ffmpeg 启动失败: {}", e);
            return;
        }
    };
    let mut stdin = child.stdin.take().expect("enc stdin");
    let stdout = child.stdout.take().expect("enc stdout");
    // 读线程：stdout（阻塞）→ 切完整 JPEG → 广播。
    // 必须与写分离：单线程"写一帧→读"会在无数据时阻塞，下一帧永远喂不进（死锁）。
    let rt2 = rt.clone();
    let reader = std::thread::Builder::new()
        .name("preview-read".into())
        .spawn(move || {
            let mut stdout = stdout;
            let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
            let mut chunk = [0u8; 16384];
            loop {
                match stdout.read(&mut chunk) {
                    Ok(0) => break, // EOF（编码器退出）
                    Ok(n) => {
                        buf.extend_from_slice(&chunk[..n]);
                        while let Some((start, end)) = find_jpeg(&buf) {
                            rt2.hub.publish(buf[start..end].to_vec());
                            buf.drain(..end);
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })
        .expect("spawn preview-read");
    // 写线程主体：主循环小图 → 编码器 stdin
    loop {
        let frame = match rx.recv() {
            Ok(f) => f,
            Err(_) => break, // 主循环退出
        };
        if stdin.write_all(&frame).is_err() {
            break;
        }
    }
    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    let _ = reader.join();
}

/// 找第一段完整 JPEG（SOI=FFD8FF…, EOI=FFD9）。熵编码数据中 FF 均被 00 填充，
/// 流内出现裸 FFD9 即帧尾。
fn find_jpeg(buf: &[u8]) -> Option<(usize, usize)> {
    let start = (0..buf.len().saturating_sub(2)).find(|&i| buf[i] == 0xFF && buf[i + 1] == 0xD8 && buf[i + 2] == 0xFF)?;
    let end = (start + 2..buf.len().saturating_sub(1)).find(|&i| buf[i] == 0xFF && buf[i + 1] == 0xD9)?;
    Some((start, end + 2))
}

// ───────────────────────── HTTP ─────────────────────────

fn handle_conn(mut stream: TcpStream, rt: Arc<Runtime>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(15)));
    // 请求头（POST body 之后按 Content-Length 读）
    let mut head = Vec::new();
    let mut b1 = [0u8; 1];
    loop {
        match stream.read(&mut b1) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                head.push(b1[0]);
                if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
                    break;
                }
                if head.len() > 16384 {
                    return;
                }
            }
        }
    }
    let head_str = String::from_utf8_lossy(&head);
    let mut lines = head_str.split("\r\n").flat_map(|l| l.split('\n'));
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    let path = path.split('?').next().unwrap_or(path);
    let content_length: usize = head_str
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("content-length") {
                v.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    match (method, path) {
        ("GET", "/") => respond(&mut stream, 200, "text/html; charset=utf-8", PAGE_HTML.as_bytes()),
        ("GET", "/api/state") => respond(&mut stream, 200, "application/json", rt.state_json().as_bytes()),
        ("POST", "/api/set") => {
            if content_length == 0 || content_length > 8192 {
                respond(&mut stream, 400, "application/json", "{\"error\":\"Content-Length 非法\"}".as_bytes());
                // keep-alive 需吸干 body：这里直接按长度读掉
                return;
            }
            let mut body = vec![0u8; content_length];
            if stream.read_exact(&mut body).is_err() {
                return;
            }
            let (res, rejected) = rt.set_from_json(&String::from_utf8_lossy(&body));
            match res {
                Ok(_) if rejected.is_empty() => {
                    respond(&mut stream, 200, "application/json", format!("{{\"ok\":true,\"state\":{}}}", rt.state_json()).as_bytes())
                }
                Ok(_) => {
                    let msg = format!("非法或不支持的字段值: {}", rejected.join(", "));
                    respond(&mut stream, 400, "application/json", format!("{{\"error\":\"{}\",\"state\":{}}}", json::escape(&msg), rt.state_json()).as_bytes())
                }
                Err(e) => {
                    respond(&mut stream, 400, "application/json", format!("{{\"error\":\"{}\"}}", json::escape(&e)).as_bytes())
                }
            }
        }
        ("GET", "/preview.mjpg") => stream_mjpeg(&mut stream, &rt),
        (_, "/preview.mjpg") | (_, "/api/state") | (_, "/api/set") | (_, "/") => {
            respond(&mut stream, 405, "text/plain", b"method not allowed")
        }
        _ => respond(&mut stream, 404, "text/plain", b"404 not found"),
    }
}

fn respond(stream: &mut TcpStream, code: u16, ctype: &str, body: &[u8]) {
    let reason = match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        code, reason, ctype, body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn stream_mjpeg(stream: &mut TcpStream, rt: &Arc<Runtime>) {
    let _ = stream.set_read_timeout(None);
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let head = "HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary=frame\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let mut last = 0u64;
    loop {
        let Some((seq, jpeg)) = rt.hub.wait_after(last, Duration::from_secs(2)) else {
            continue; // 超时无新帧：保持连接
        };
        last = seq;
        let part = format!(
            "--frame\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
            jpeg.len()
        );
        if stream.write_all(part.as_bytes()).is_err() || stream.write_all(&jpeg).is_err() || stream.write_all(b"\r\n").is_err() || stream.flush().is_err() {
            return; // 客户端断开
        }
    }
}

// ───────────────────────── 单页控制台 ─────────────────────────

const PAGE_HTML: &str = r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>AI 虚拟摄像头 · xrun-rs</title>
<style>
:root{--bg:#0e1116;--panel:#161a22;--panel2:#1d222d;--line:#2a3040;--fg:#e7eaf2;--muted:#9199ac;--accent:#5b9cff;--ok:#41d39c;--warn:#ff7a6b}
*{box-sizing:border-box;-webkit-tap-highlight-color:transparent}
html,body{margin:0}
body{background:var(--bg);color:var(--fg);font:15px/1.65 system-ui,-apple-system,"Noto Sans CJK SC","PingFang SC","Microsoft YaHei",sans-serif}
.wrap{max-width:780px;margin:0 auto;padding:14px 14px 44px}
header{display:flex;align-items:center;gap:10px;margin:6px 0 14px}
h1{font-size:17px;margin:0;font-weight:600;flex:none}
.badge{font-size:12px;color:var(--muted);border:1px solid var(--line);border-radius:99px;padding:1px 10px;margin-left:auto}
.dot{width:9px;height:9px;border-radius:50%;background:var(--ok);box-shadow:0 0 10px var(--ok)}
.dot.off{background:var(--warn);box-shadow:0 0 10px var(--warn)}
.card{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:14px;margin-bottom:14px}
#prev{display:block;width:100%;aspect-ratio:16/9;background:#000;border-radius:10px;object-fit:contain}
.row{display:flex;align-items:center;gap:10px}
.row label{font-weight:600;flex:none}
.val{font-variant-numeric:tabular-nums;color:var(--accent);font-weight:700;min-width:52px;text-align:right;flex:none}
input[type=range]{flex:1;accent-color:var(--accent);height:34px;min-width:0}
.modes{display:flex;gap:10px;margin-top:12px}
button{cursor:pointer;border:1px solid var(--line);background:var(--panel2);color:var(--fg);border-radius:10px;padding:11px 10px;font-size:15px;transition:background .15s,transform .1s;flex:1}
button:active{transform:scale(.98)}
button.on{background:var(--accent);border-color:var(--accent);color:#fff;font-weight:700}
#toast{position:fixed;left:50%;bottom:18px;transform:translateX(-50%);background:var(--warn);color:#fff;border-radius:99px;padding:6px 16px;font-size:13px;opacity:0;pointer-events:none;transition:opacity .25s;max-width:90vw}
#toast.show{opacity:1}
.stats{display:grid;grid-template-columns:repeat(3,1fr);gap:10px;margin-top:2px}
.stat{background:var(--panel2);border-radius:10px;padding:10px;text-align:center}
.stat b{display:block;font-size:19px;font-variant-numeric:tabular-nums}
.stat span{font-size:12px;color:var(--muted)}
</style>
</head>
<body>
<div class="wrap">
  <header>
    <div class="dot off" id="dot"></div>
    <h1>AI 虚拟摄像头</h1>
    <span class="badge" id="mode">—</span>
  </header>
  <div class="card"><img id="prev" src="/preview.mjpg" alt="实时预览"></div>
  <div class="card">
    <div class="row">
      <label for="st">虚化强度</label>
      <input type="range" id="st" min="0" max="100" step="1" value="100">
      <span class="val" id="stVal">100%</span>
    </div>
    <div class="modes">
      <button id="m-blur">标准模糊</button>
      <button id="m-portrait">人像模糊</button>
      <button id="m-passthrough">直通</button>
    </div>
  </div>
  <div class="card">
    <div class="row"><label>眼神接触</label><span class="val" id="ecNote" style="font-size:12px;color:var(--muted)">（复刻中·暂不生效）</span></div>
    <div class="modes">
      <button id="ec-off">关</button>
      <button id="ec-standard">标准</button>
      <button id="ec-teleprompter">提词器</button>
    </div>
  </div>
  <div class="card" style="color:var(--muted);font-size:13px">
    对齐 Windows Studio Effects 设置项：背景效果（标准/人像模糊）、眼神接触（标准/提词器）。
    自动取景、创意滤镜、人像光效在 MEP 本 SKU 逆向管线中未复刻，暂不提供。
  </div>
  <div class="stats">
    <div class="stat"><b id="fps">—</b><span>帧率 fps</span></div>
    <div class="stat"><b id="ms">—</b><span>单帧耗时 ms</span></div>
    <div class="stat"><b id="frames">—</b><span>累计帧数</span></div>
  </div>
</div>
<div id="toast"></div>
<script>
const $=i=>document.getElementById(i);
const MODES=['blur','portrait','passthrough'];
const ECS=['off','standard','teleprompter'];
let dragging=false;
function toast(m){const t=$('toast');t.textContent=m;t.classList.add('show');clearTimeout(t._h);t._h=setTimeout(()=>t.classList.remove('show'),2200)}
async function api(body){
  const r=await fetch('/api/set',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)});
  const j=await r.json().catch(()=>({}));
  if(!r.ok) throw new Error(j.error||('HTTP '+r.status));
  return j.state;
}
$('st').addEventListener('input',e=>{dragging=true;$('stVal').textContent=e.target.value+'%'});
$('st').addEventListener('change',e=>{dragging=false;
  api({strength:+e.target.value/100}).then(sync).catch(e=>toast(e.message))});
MODES.forEach(m=>{$('m-'+m).addEventListener('click',()=>{
  api({mode:m}).then(sync).catch(e=>toast(e.message))})});
ECS.forEach(e=>{$('ec-'+e).addEventListener('click',()=>{
  api({'eye-contact':e}).then(sync).catch(e=>toast(e.message))})});
function sync(s){
  $('mode').textContent=({blur:'标准模糊',portrait:'人像模糊',passthrough:'直通'})[s.mode]||s.mode;
  MODES.forEach(m=>$('m-'+m).classList.toggle('on',s.mode===m));
  ECS.forEach(e=>$('ec-'+e).classList.toggle('on',s.eye_contact===e));
  $('fr-none').classList.toggle('on',s.framing==='none');$('fr-auto').classList.toggle('on',s.framing==='auto');
  if(!dragging){$('st').value=Math.round(s.strength*100);$('stVal').textContent=Math.round(s.strength*100)+'%'}
  $('fps').textContent=s.fps.toFixed(1);
  $('ms').textContent=Math.round(s.frame_ms);
  $('frames').textContent=s.frames;
}
async function poll(){try{sync(await (await fetch('/api/state')).json());$('dot').classList.remove('off')}
  catch{$('dot').classList.add('off')}}
poll();setInterval(poll,1000);
</script>
</body>
</html>
"#;
