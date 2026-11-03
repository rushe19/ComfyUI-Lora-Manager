//! Reading LoRA Manager `.metadata.json` sidecars, tolerant of the damage seen on real libraries,
//! and writing them back the way LoRA Manager does (2-space JSON, UTF-8, CRLF on Windows, atomic).

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct SidecarRead {
    pub data: Map<String, Value>,
    /// Set when the file was damaged but a value could be rescued.
    pub damage: Option<Damage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Damage {
    /// Valid JSON followed by extra bytes (e.g. a stray `}` from a non-truncating rewrite).
    TrailingData,
    /// Raw control characters inside strings (Python's `json` with strict=False wrote them).
    ControlCharacters,
}

#[derive(Debug)]
pub enum SidecarError {
    Io(std::io::Error),
    /// Unrecoverable JSON (or not an object).
    Invalid(String),
}

impl std::fmt::Display for SidecarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SidecarError::Io(e) => write!(f, "io: {e}"),
            SidecarError::Invalid(e) => write!(f, "invalid sidecar: {e}"),
        }
    }
}

/// Read and parse a sidecar. `Ok(None)` when the file does not exist.
pub fn read(path: &Path) -> Result<Option<SidecarRead>, SidecarError> {
    match std::fs::read(path) {
        Ok(bytes) => parse(&bytes).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(SidecarError::Io(e)),
    }
}

/// Parse sidecar bytes, rescuing the two damage patterns found on real libraries.
pub fn parse(bytes: &[u8]) -> Result<SidecarRead, SidecarError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match serde_json::from_slice::<Value>(bytes) {
        Ok(v) => into_object(v, None),
        Err(e) => {
            // 1) Trailing data: take the first complete JSON value.
            if e.classify() == serde_json::error::Category::Syntax && e.to_string().contains("trailing characters") {
                let mut it = serde_json::Deserializer::from_slice(bytes).into_iter::<Value>();
                if let Some(Ok(v)) = it.next() {
                    return into_object(v, Some(Damage::TrailingData));
                }
            }
            // 2) Raw control characters inside strings: escape them and retry.
            if e.to_string().contains("control character") {
                let fixed = escape_control_chars_in_strings(bytes);
                let mut it = serde_json::Deserializer::from_slice(&fixed).into_iter::<Value>();
                if let Some(Ok(v)) = it.next() {
                    return into_object(v, Some(Damage::ControlCharacters));
                }
            }
            Err(SidecarError::Invalid(e.to_string()))
        }
    }
}

fn into_object(v: Value, damage: Option<Damage>) -> Result<SidecarRead, SidecarError> {
    match v {
        Value::Object(data) => Ok(SidecarRead { data, damage }),
        other => Err(SidecarError::Invalid(format!("expected object, got {}", type_name(&other)))),
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Serialize like Python's `json.dump(d, f, indent=2, ensure_ascii=False)` in text mode:
/// key order kept, non-ASCII unescaped, floats in Python `repr` form (`5e-05`, `10.0`), and on
/// Windows `\r\n` line ends (raw newlines never occur inside JSON strings, so replacing every
/// `\n` is safe). Real LoRA Manager sidecars round-trip byte for byte.
pub fn to_bytes(map: &Map<String, Value>) -> Vec<u8> {
    use serde::Serialize;
    let mut out = Vec::with_capacity(4096);
    let mut ser = serde_json::Serializer::with_formatter(&mut out, PyFormatter(serde_json::ser::PrettyFormatter::new()));
    map.serialize(&mut ser).expect("a JSON map always serializes");
    if cfg!(windows) {
        let mut crlf = Vec::with_capacity(out.len() + out.len() / 16);
        for b in out {
            if b == b'\n' {
                crlf.push(b'\r');
            }
            crlf.push(b);
        }
        crlf
    } else {
        out
    }
}

/// Python's float `repr`: shortest round-trip digits, fixed notation for 1e-4 <= |x| < 1e16
/// (always with a fractional part), otherwise `d.ddde±XX`.
pub fn python_float_repr(f: f64) -> String {
    if !f.is_finite() {
        return if f.is_nan() { "NaN".into() } else if f > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // `{:e}` gives the shortest round-trip digits, e.g. "1.793714412542739e9" / "5e-5".
    let sci = format!("{:e}", f);
    let (mant, exp) = sci.split_once('e').expect("exponent form");
    let exp: i32 = exp.parse().expect("integer exponent");
    let (neg, mant) = match mant.strip_prefix('-') {
        Some(m) => (true, m),
        None => (false, mant),
    };
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let sign = if neg { "-" } else { "" };
    if (-4..16).contains(&exp) {
        let n = digits.len() as i32;
        let body = if exp < 0 {
            format!("0.{}{}", "0".repeat((-exp - 1) as usize), digits)
        } else if exp + 1 >= n {
            format!("{}{}.0", digits, "0".repeat((exp + 1 - n) as usize))
        } else {
            let (a, b) = digits.split_at((exp + 1) as usize);
            format!("{a}.{b}")
        };
        format!("{sign}{body}")
    } else {
        let m = if digits.len() > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits };
        format!("{sign}{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs())
    }
}

struct PyFormatter(serde_json::ser::PrettyFormatter<'static>);

impl serde_json::ser::Formatter for PyFormatter {
    fn write_f64<W: ?Sized + std::io::Write>(&mut self, w: &mut W, v: f64) -> std::io::Result<()> {
        w.write_all(python_float_repr(v).as_bytes())
    }
    fn write_f32<W: ?Sized + std::io::Write>(&mut self, w: &mut W, v: f32) -> std::io::Result<()> {
        self.write_f64(w, v as f64)
    }
    fn begin_array<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.0.begin_array(w)
    }
    fn end_array<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.0.end_array(w)
    }
    fn begin_array_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
        self.0.begin_array_value(w, first)
    }
    fn end_array_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.0.end_array_value(w)
    }
    fn begin_object<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.0.begin_object(w)
    }
    fn end_object<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.0.end_object(w)
    }
    fn begin_object_key<W: ?Sized + std::io::Write>(&mut self, w: &mut W, first: bool) -> std::io::Result<()> {
        self.0.begin_object_key(w, first)
    }
    fn begin_object_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.0.begin_object_value(w)
    }
    fn end_object_value<W: ?Sized + std::io::Write>(&mut self, w: &mut W) -> std::io::Result<()> {
        self.0.end_object_value(w)
    }
}

/// Copies of files before lmx changes or removes them: `<dir>/<drive>/<path>`.
#[derive(Debug, Clone)]
pub struct Backup {
    pub dir: PathBuf,
}

impl Backup {
    /// A fresh timestamped folder under `<data_dir>/backups`.
    pub fn new(data_dir: &Path, label: &str) -> Backup {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        Backup { dir: data_dir.join("backups").join(format!("{secs}-{label}")) }
    }

    pub fn target_for(&self, path: &Path) -> PathBuf {
        let norm = crate::paths::norm(path).replace(':', "");
        let mut t = self.dir.clone();
        for part in norm.split('/').filter(|p| !p.is_empty() && *p != "." && *p != "..") {
            t.push(part);
        }
        t
    }

    /// Copy `path` (if it exists and was not saved yet) into the backup tree.
    pub fn save(&self, path: &Path) -> std::io::Result<Option<PathBuf>> {
        if !path.exists() {
            return Ok(None);
        }
        let t = self.target_for(path);
        if t.exists() {
            return Ok(Some(t));
        }
        if let Some(dir) = t.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::copy(path, &t)?;
        Ok(Some(t))
    }
}

/// Write a sidecar atomically (temp file + rename), backing up the previous content first.
pub fn write(path: &Path, map: &Map<String, Value>, backup: Option<&Backup>) -> std::io::Result<()> {
    if let Some(b) = backup {
        b.save(path)?;
    }
    let tmp = crate::paths::append(path, ".lmx.tmp");
    std::fs::write(&tmp, to_bytes(map))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// Replace raw bytes < 0x20 that occur inside JSON strings with `\u00XX` escapes.
fn escape_control_chars_in_strings(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() + 16);
    let mut in_string = false;
    let mut escaped = false;
    for &b in input {
        if in_string {
            if escaped {
                escaped = false;
                out.push(b);
            } else if b == b'\\' {
                escaped = true;
                out.push(b);
            } else if b == b'"' {
                in_string = false;
                out.push(b);
            } else if b < 0x20 {
                out.extend_from_slice(format!("\\u{:04x}", b).as_bytes());
            } else {
                out.push(b);
            }
        } else {
            if b == b'"' {
                in_string = true;
            }
            out.push(b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_object() {
        let r = parse(br#"{"file_name":"a","size":1}"#).unwrap();
        assert_eq!(r.data["file_name"], "a");
        assert_eq!(r.damage, None);
    }

    #[test]
    fn strips_utf8_bom() {
        let r = parse(b"\xEF\xBB\xBF{\"a\":1}").unwrap();
        assert_eq!(r.data["a"], 1);
    }

    #[test]
    fn rescues_trailing_brace_seen_on_real_library() {
        // Shape of 24 real damaged sidecars: CRLF JSON whose last line is `"..": "{}"\r\n}}`.
        let raw = b"{\r\n  \"file_name\": \"x\",\r\n  \"usage_tips\": \"{}\"\r\n}}";
        let r = parse(raw).unwrap();
        assert_eq!(r.data["usage_tips"], "{}");
        assert_eq!(r.damage, Some(Damage::TrailingData));
    }

    #[test]
    fn rescues_raw_control_characters() {
        let raw = b"{\"notes\": \"line1\nline2\ttab\", \"ok\": true}";
        let r = parse(raw).unwrap();
        assert_eq!(r.data["notes"], "line1\nline2\ttab");
        assert_eq!(r.damage, Some(Damage::ControlCharacters));
    }

    #[test]
    fn escaping_does_not_touch_structural_whitespace() {
        let raw = b"{\r\n\t\"a\": \"b\\\"\nc\"\r\n}";
        let r = parse(raw).unwrap();
        assert_eq!(r.data["a"], "b\"\nc");
    }

    #[test]
    fn rejects_garbage_and_non_objects() {
        assert!(matches!(parse(b"{not json"), Err(SidecarError::Invalid(_))));
        assert!(matches!(parse(b"[1,2]"), Err(SidecarError::Invalid(_))));
    }

    #[test]
    fn missing_file_is_none() {
        let p = std::env::temp_dir().join("lmx-definitely-missing.metadata.json");
        assert!(read(&p).unwrap().is_none());
    }

    #[test]
    fn python_float_repr_matches_cpython() {
        for (f, s) in [
            (5e-05, "5e-05"),
            (0.0001, "0.0001"),
            (1793714412.542739, "1793714412.542739"),
            (10.0, "10.0"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (123456789012345.6, "123456789012345.6"),
            (-0.25, "-0.25"),
            (0.1, "0.1"),
            (1e-7, "1e-07"),
            (2.5e-10, "2.5e-10"),
            (0.0, "0.0"),
            (1e22, "1e+22"),
        ] {
            assert_eq!(python_float_repr(f), s, "{f}");
        }
    }

    #[test]
    fn writes_lm_format_and_backs_up_previous_content() {
        let dir = std::env::temp_dir().join(format!("lmx-side-w-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.metadata.json");
        // A real LoRA Manager sidecar is byte-identical after a parse/write round trip.
        let original = b"{\r\n  \"file_name\": \"a\",\r\n  \"size\": 228454596,\r\n  \"modified\": 1793714412.542739,\r\n  \"tags\": [\r\n    \"\xe6\x97\xa5\xe6\x9c\xac\",\r\n    \"x\"\r\n  ],\r\n  \"civitai\": {},\r\n  \"notes\": \"\",\r\n  \"usage_tips\": \"{}\"\r\n}";
        std::fs::write(&p, original).unwrap();
        let mut m = read(&p).unwrap().unwrap().data;
        if cfg!(windows) {
            assert_eq!(to_bytes(&m), original.to_vec());
        }
        m.insert("notes".into(), Value::String("hi\nthere".into()));
        let b = Backup { dir: dir.join("bk") };
        write(&p, &m, Some(&b)).unwrap();
        assert_eq!(std::fs::read(b.target_for(&p)).unwrap(), original.to_vec());
        let back = read(&p).unwrap().unwrap();
        assert_eq!(back.data["notes"], "hi\nthere");
        assert!(back.damage.is_none());
        assert!(!crate::paths::append(&p, ".lmx.tmp").exists());
        // Second write keeps the first backup (the true original).
        write(&p, &back.data, Some(&b)).unwrap();
        assert_eq!(std::fs::read(b.target_for(&p)).unwrap(), original.to_vec());
    }
}
