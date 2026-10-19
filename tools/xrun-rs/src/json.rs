// json.rs — 迷你 JSON（严格子集：与 profile TOML 解析器同风格，非法即拒，绝不猜测）
// 支持：null/true/false/数字(含负号/小数/指数)/字符串(基础转义+UTF-8 原样)/数组/对象。
// 用途：控制台 /api/set 入参解析、/api/state 出参序列化——足够且无外部依赖。

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Result<Json, String> {
    let b = text.as_bytes();
    let mut p = 0usize;
    let v = parse_val(b, &mut p)?;
    skip_ws(b, &mut p);
    if p != b.len() {
        return Err(format!("JSON 尾部多余内容（offset {}）", p));
    }
    Ok(v)
}

fn skip_ws(b: &[u8], p: &mut usize) {
    while *p < b.len() && matches!(b[*p], b' ' | b'\t' | b'\r' | b'\n') {
        *p += 1;
    }
}

fn parse_val(b: &[u8], p: &mut usize) -> Result<Json, String> {
    skip_ws(b, p);
    match b.get(*p) {
        None => Err("JSON 意外结束".into()),
        Some(b'{') => {
            *p += 1;
            let mut kvs = Vec::new();
            skip_ws(b, p);
            if b.get(*p) == Some(&b'}') {
                *p += 1;
                return Ok(Json::Obj(kvs));
            }
            loop {
                skip_ws(b, p);
                let k = match parse_val(b, p)? {
                    Json::Str(s) => s,
                    _ => return Err("对象键需为字符串".into()),
                };
                skip_ws(b, p);
                if b.get(*p) != Some(&b':') {
                    return Err("对象缺 ':'".into());
                }
                *p += 1;
                let v = parse_val(b, p)?;
                kvs.push((k, v));
                skip_ws(b, p);
                match b.get(*p) {
                    Some(b',') => *p += 1,
                    Some(b'}') => {
                        *p += 1;
                        return Ok(Json::Obj(kvs));
                    }
                    _ => return Err("对象缺 '}' 或 ','".into()),
                }
            }
        }
        Some(b'[') => {
            *p += 1;
            let mut arr = Vec::new();
            skip_ws(b, p);
            if b.get(*p) == Some(&b']') {
                *p += 1;
                return Ok(Json::Arr(arr));
            }
            loop {
                arr.push(parse_val(b, p)?);
                skip_ws(b, p);
                match b.get(*p) {
                    Some(b',') => *p += 1,
                    Some(b']') => {
                        *p += 1;
                        return Ok(Json::Arr(arr));
                    }
                    _ => return Err("数组缺 ']' 或 ','".into()),
                }
            }
        }
        Some(b'"') => parse_str(b, p).map(Json::Str),
        Some(b't') => lit(b, p, "true", Json::Bool(true)),
        Some(b'f') => lit(b, p, "false", Json::Bool(false)),
        Some(b'n') => lit(b, p, "null", Json::Null),
        Some(c) if *c == b'-' || c.is_ascii_digit() => parse_num(b, p),
        Some(c) => Err(format!("非法字符 {:?}（offset {}）", *c as char, p)),
    }
}

fn lit(b: &[u8], p: &mut usize, word: &str, v: Json) -> Result<Json, String> {
    if b[*p..].starts_with(word.as_bytes()) {
        *p += word.len();
        Ok(v)
    } else {
        Err(format!("非法字面量（期望 {}，offset {}）", word, p))
    }
}

fn parse_num(b: &[u8], p: &mut usize) -> Result<Json, String> {
    let start = *p;
    if b.get(*p) == Some(&b'-') {
        *p += 1;
    }
    while *p < b.len() && (b[*p].is_ascii_digit() || matches!(b[*p], b'.' | b'e' | b'E' | b'+' | b'-')) {
        *p += 1;
    }
    let s = std::str::from_utf8(&b[start..*p]).map_err(|_| "数字编码错误".to_string())?;
    s.parse::<f64>()
        .map(Json::Num)
        .map_err(|_| format!("非法数字 {:?}", s))
}

fn parse_str(b: &[u8], p: &mut usize) -> Result<String, String> {
    debug_assert_eq!(b[*p], b'"');
    *p += 1;
    let mut out = String::new();
    loop {
        match b.get(*p) {
            None => return Err("字符串未闭合".into()),
            Some(b'"') => {
                *p += 1;
                return Ok(out);
            }
            Some(b'\\') => {
                *p += 1;
                match b.get(*p) {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'/') => out.push('/'),
                    Some(b'n') => out.push('\n'),
                    Some(b't') => out.push('\t'),
                    Some(b'r') => out.push('\r'),
                    Some(b'b') => out.push('\u{8}'),
                    Some(b'f') => out.push('\u{c}'),
                    Some(b'u') => {
                        let hex = std::str::from_utf8(b.get(*p + 1..*p + 5).ok_or("\\u 转义不完整")?)
                            .map_err(|_| "\\u 转义非法")?;
                        let cp = u32::from_str_radix(hex, 16).map_err(|_| "\\u 转义非法")?;
                        out.push(char::from_u32(cp).ok_or("\\u 码点非法")?);
                        *p += 4;
                    }
                    _ => return Err("非法转义".into()),
                }
                *p += 1;
            }
            Some(&c) if c < 0x20 => return Err("字符串含控制字符".into()),
            Some(&c) => {
                // UTF-8 原样透传
                let len = if c < 0x80 { 1 } else if c >> 5 == 0b110 { 2 } else if c >> 4 == 0b1110 { 3 } else { 4 };
                let seg = b.get(*p..*p + len).ok_or("UTF-8 序列不完整")?;
                out.push_str(std::str::from_utf8(seg).map_err(|_| "非法 UTF-8")?);
                *p += len;
            }
        }
    }
}

// ───────────── 序列化（仅用于固定出参；字符串转义完整以防万一） ─────────────

pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
