//! Parse Civitai links pasted by the user.

use std::sync::OnceLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivitaiRef {
    pub model_id: Option<i64>,
    pub version_id: Option<i64>,
}

fn re(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

/// Accepts `civitai.com|red|green/models/123[/slug][?modelVersionId=456]`,
/// `/api/download/models/456` (version) and `/model-versions/456`.
pub fn parse(input: &str) -> Option<CivitaiRef> {
    static HOST: OnceLock<Regex> = OnceLock::new();
    static MODEL: OnceLock<Regex> = OnceLock::new();
    static VERSION_Q: OnceLock<Regex> = OnceLock::new();
    static DOWNLOAD: OnceLock<Regex> = OnceLock::new();
    static VERSIONS: OnceLock<Regex> = OnceLock::new();
    let s = input.trim();
    if !re(r"(?i)^https?://([a-z0-9-]+\.)*civitai\.[a-z]+/", &HOST).is_match(s) {
        return None;
    }
    if let Some(c) = re(r"/api/download/models/(\d+)", &DOWNLOAD).captures(s) {
        return Some(CivitaiRef { model_id: None, version_id: c[1].parse().ok() });
    }
    if let Some(c) = re(r"/model-versions/(\d+)", &VERSIONS).captures(s) {
        return Some(CivitaiRef { model_id: None, version_id: c[1].parse().ok() });
    }
    let model_id = re(r"/models/(\d+)", &MODEL).captures(s).and_then(|c| c[1].parse().ok());
    let version_id = re(r"[?&]modelVersionId=(\d+)", &VERSION_Q).captures(s).and_then(|c| c[1].parse().ok());
    if model_id.is_none() && version_id.is_none() {
        return None;
    }
    Some(CivitaiRef { model_id, version_id })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_link_shapes() {
        assert_eq!(parse("https://civitai.com/models/1187259"), Some(CivitaiRef { model_id: Some(1187259), version_id: None }));
        assert_eq!(
            parse("https://civitai.red/models/1187259/frieren-4-outfits?modelVersionId=1336431"),
            Some(CivitaiRef { model_id: Some(1187259), version_id: Some(1336431) })
        );
        assert_eq!(parse("https://civitai.com/api/download/models/1336431"), Some(CivitaiRef { model_id: None, version_id: Some(1336431) }));
        assert_eq!(parse("  https://civitai.green/models/5?foo=1&modelVersionId=9  "), Some(CivitaiRef { model_id: Some(5), version_id: Some(9) }));
    }

    #[test]
    fn rejects_other_sites_and_pages() {
        assert_eq!(parse("https://example.com/models/123"), None);
        assert_eq!(parse("https://civitai.com/images/123"), None);
        assert_eq!(parse("not a url"), None);
    }
}
