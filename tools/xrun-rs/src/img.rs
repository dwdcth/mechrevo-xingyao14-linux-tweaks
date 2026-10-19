// img.rs — 零依赖图像原语：双线性缩放（预计算权重）、可分离 box blur、
// 高斯近似（3 次 box pass）、分位归一化。全为流式管线设计（热路径无逐像素重算）。

/// 双线性缩放 x 方向权重（预计算一次；y 方向每行两个源行）
#[inline]
fn x_weights(sw: usize, dw: usize) -> Vec<(usize, usize, f32)> {
    let ratio = sw as f32 / dw as f32;
    (0..dw)
        .map(|x| {
            let fx = (x as f32 + 0.5) * ratio - 0.5;
            let x0 = fx.floor().max(0.0) as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let wx = (fx - x0 as f32).clamp(0.0, 1.0);
            (x0, x1, wx)
        })
        .collect()
}

fn y_pair(sh: usize, dh: usize, y: usize) -> (usize, usize, f32) {
    let ratio = sh as f32 / dh as f32;
    let fy = (y as f32 + 0.5) * ratio - 0.5;
    let y0 = fy.floor().max(0.0) as usize;
    let y1 = (y0 + 1).min(sh - 1);
    let wy = (fy - y0 as f32).clamp(0.0, 1.0);
    (y0, y1, wy)
}

/// 双线性缩放：u8 交错多通道 → u8 交错（通道数 ch 保持；权重预计算版）
pub fn resize_bilinear_u8(src: &[u8], sw: usize, sh: usize, ch: usize, dw: usize, dh: usize) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let xw = x_weights(sw, dw);
    let mut dst = vec![0u8; dw * dh * ch];
    for y in 0..dh {
        let (y0, y1, wy) = y_pair(sh, dh, y);
        let row0 = y0 * sw;
        let row1 = y1 * sw;
        let drow = y * dw;
        for (x, &(x0, x1, wx)) in xw.iter().enumerate() {
            for c in 0..ch {
                let p00 = src[(row0 + x0) * ch + c] as f32;
                let p01 = src[(row0 + x1) * ch + c] as f32;
                let p10 = src[(row1 + x0) * ch + c] as f32;
                let p11 = src[(row1 + x1) * ch + c] as f32;
                let v = p00 * (1.0 - wx) * (1.0 - wy)
                    + p01 * wx * (1.0 - wy)
                    + p10 * (1.0 - wx) * wy
                    + p11 * wx * wy;
                dst[(drow + x) * ch + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    dst
}

/// 双线性缩放：f32 平面（单通道，权重预计算版）
pub fn resize_bilinear_f32(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let xw = x_weights(sw, dw);
    let mut dst = vec![0f32; dw * dh];
    for y in 0..dh {
        let (y0, y1, wy) = y_pair(sh, dh, y);
        let row0 = y0 * sw;
        let row1 = y1 * sw;
        let drow = y * dw;
        for (x, &(x0, x1, wx)) in xw.iter().enumerate() {
            let p00 = src[row0 + x0];
            let p01 = src[row0 + x1];
            let p10 = src[row1 + x0];
            let p11 = src[row1 + x1];
            dst[drow + x] = p00 * (1.0 - wx) * (1.0 - wy)
                + p01 * wx * (1.0 - wy)
                + p10 * (1.0 - wx) * wy
                + p11 * wx * wy;
        }
    }
    dst
}

/// 可分离 box blur（f32 平面，滑动窗口 O(n)）。radius=0 原样拷贝。
pub fn box_blur_f32(src: &[f32], w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 || w == 0 || h == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0f32; w * h];
    let mut out = vec![0f32; w * h];
    let win = 2 * radius + 1;
    // 水平
    for y in 0..h {
        let row = y * w;
        let mut sum = 0f32;
        for x in 0..radius.min(w) {
            sum += src[row + x];
        }
        for x in 0..w {
            if x + radius < w {
                sum += src[row + x + radius];
            }
            tmp[row + x] = sum / win as f32;
            if x >= radius {
                sum -= src[row + x - radius];
            }
        }
    }
    // 垂直
    for x in 0..w {
        let mut sum = 0f32;
        for y in 0..radius.min(h) {
            sum += tmp[y * w + x];
        }
        for y in 0..h {
            if y + radius < h {
                sum += tmp[(y + radius) * w + x];
            }
            out[y * w + x] = sum / win as f32;
            if y >= radius {
                sum -= tmp[(y - radius) * w + x];
            }
        }
    }
    out
}

/// 高斯近似：n 次 box pass（经典 boxblur 逼近）。
/// 每次_pass 的 box 半径由总 σ 反解：box_width = sqrt(12σ²/n + 1)（取奇数）。
pub fn gauss_approx_f32(src: &[f32], w: usize, h: usize, sigma: f32, passes: usize) -> Vec<f32> {
    if sigma <= 0.0 || passes == 0 {
        return src.to_vec();
    }
    let n = passes as f32;
    let bw = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut width = bw.floor() as usize;
    if width % 2 == 0 {
        width += 1;
    }
    let radius = width.saturating_sub(1) / 2;
    let mut cur = src.to_vec();
    for _ in 0..passes {
        cur = box_blur_f32(&cur, w, h, radius);
    }
    cur
}

/// 就地分位归一化（256 箱直方图，O(n)）：把窄带软掩码拉伸到 [0,1]。
/// id109 seg 输出实测集中在 ~[0.5,0.75] 的软值带（有空间结构但非饱和 0/1），
/// 按帧 2%/98% 分位拉伸后呈清晰双峰（背景 74% / 前景 24%，见 README 验证记录）。
pub fn percentile_normalize_256(m: &mut [f32], plo: f32, phi: f32) {
    let n = m.len();
    if n == 0 {
        return;
    }
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in m.iter() {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if hi - lo < 1e-9 {
        m.fill(0.0);
        return;
    }
    let mut hist = [0u32; 256];
    for &v in m.iter() {
        let b = (((v - lo) / (hi - lo)) * 255.0) as usize;
        hist[b.min(255)] += 1;
    }
    let target_lo = (plo / 100.0 * n as f32).ceil() as u32;
    let target_hi = (phi / 100.0 * n as f32).ceil() as u32;
    let (mut clo, mut chi) = (lo, hi);
    let mut acc = 0u32;
    for b in 0..256 {
        acc += hist[b];
        if acc >= target_lo {
            clo = lo + (b as f32 / 255.0) * (hi - lo);
            break;
        }
    }
    acc = 0;
    for b in 0..256 {
        acc += hist[b];
        if acc >= target_hi {
            chi = lo + (b as f32 / 255.0) * (hi - lo);
            break;
        }
    }
    let span = (chi - clo).max(1e-6);
    for v in m.iter_mut() {
        *v = ((*v - clo) / span).clamp(0.0, 1.0);
    }
}

/// 可分离双线性缩放（u8 交错）：先水平后垂直，中间 f32。
/// 两遍各 2-tap、内层连续内存 → 向量化友好，比 2D 4-tap 快 ~2-3 倍（大放大比场景）。
pub fn resize_bilinear_u8_sep(src: &[u8], sw: usize, sh: usize, ch: usize, dw: usize, dh: usize) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let xw = x_weights(sw, dw);
    // 水平：sh × dw
    let mut mid = vec![0f32; sh * dw * ch];
    for y in 0..sh {
        let srow = y * sw;
        let mrow = y * dw;
        for (x, &(x0, x1, wx)) in xw.iter().enumerate() {
            for c in 0..ch {
                let a = src[(srow + x0) * ch + c] as f32;
                let b = src[(srow + x1) * ch + c] as f32;
                mid[(mrow + x) * ch + c] = a * (1.0 - wx) + b * wx;
            }
        }
    }
    // 垂直：dh × dw
    let mut dst = vec![0u8; dw * dh * ch];
    for y in 0..dh {
        let (y0, y1, wy) = y_pair(sh, dh, y);
        let r0 = y0 * dw;
        let r1 = y1 * dw;
        let drow = y * dw;
        for x in 0..dw {
            for c in 0..ch {
                let a = mid[(r0 + x) * ch + c];
                let b = mid[(r1 + x) * ch + c];
                dst[(drow + x) * ch + c] = (a * (1.0 - wy) + b * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    dst
}

/// 可分离双线性缩放（f32 平面）
pub fn resize_bilinear_f32_sep(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let xw = x_weights(sw, dw);
    let mut mid = vec![0f32; sh * dw];
    for y in 0..sh {
        let srow = y * sw;
        let mrow = y * dw;
        for (x, &(x0, x1, wx)) in xw.iter().enumerate() {
            mid[mrow + x] = src[srow + x0] * (1.0 - wx) + src[srow + x1] * wx;
        }
    }
    let mut dst = vec![0f32; dw * dh];
    for y in 0..dh {
        let (y0, y1, wy) = y_pair(sh, dh, y);
        let r0 = y0 * dw;
        let r1 = y1 * dw;
        let drow = y * dw;
        for x in 0..dw {
            dst[drow + x] = mid[r0 + x] * (1.0 - wy) + mid[r1 + x] * wy;
        }
    }
    dst
}

/// 可分离双线性缩放：u8 单通道平面（连续内存 → 向量化，交错 3 通道版的 ~15 倍）
pub fn resize_bilinear_u8_plane_sep(src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<u8> {
    if sw == dw && sh == dh {
        return src.to_vec();
    }
    let xw = x_weights(sw, dw);
    let mut mid = vec![0f32; sh * dw];
    for y in 0..sh {
        let srow = y * sw;
        let mrow = y * dw;
        for (x, &(x0, x1, wx)) in xw.iter().enumerate() {
            mid[mrow + x] = src[srow + x0] as f32 * (1.0 - wx) + src[srow + x1] as f32 * wx;
        }
    }
    let mut dst = vec![0u8; dw * dh];
    for y in 0..dh {
        let (y0, y1, wy) = y_pair(sh, dh, y);
        let r0 = y0 * dw;
        let r1 = y1 * dw;
        let drow = y * dw;
        let (a, b) = (&mid[r0..r0 + dw], &mid[r1..r1 + dw]);
        let d = &mut dst[drow..drow + dw];
        for x in 0..dw {
            d[x] = (a[x] * (1.0 - wy) + b[x] * wy).round().clamp(0.0, 255.0) as u8;
        }
    }
    dst
}
