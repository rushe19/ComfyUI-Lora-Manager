//! Reading LoRA Manager `.metadata.json` sidecars, tolerant of the damage seen on real libraries.
//!
//! Phase 1 is read-only: nothing in this module writes to disk.

use std::path::Path;

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
}
