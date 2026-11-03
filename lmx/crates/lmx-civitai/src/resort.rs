//! Keyword search with a real sort order.
//!
//! Civitai's `/models` ignores `sort` whenever `query` is set (results come back by relevance,
//! identical for every sort; verified 2026-11-04) and stops paginating around 1,000 hits.
//! So for "keyword + sort" lmx collects the relevance-ranked pool (up to [`POOL_MAX`]),
//! sorts it itself and pages through it locally.

use serde_json::{json, Map, Value};

pub const POOL_MAX: usize = 1000;
pub const SORTS: &[&str] = &["Highest Rated", "Most Downloaded", "Newest", "Most Liked"];

/// Only keyword searches with an explicit sort need the pool.
pub fn wants_resort(query: &str, sort: Option<&str>) -> bool {
    !query.trim().is_empty() && sort.is_some_and(|s| SORTS.contains(&s))
}

fn num(m: &Value, ptr: &str) -> i64 {
    m.pointer(ptr).and_then(Value::as_i64).unwrap_or(0)
}

/// Newest publish date among the versions (ISO 8601 strings compare correctly).
fn newest(m: &Value) -> String {
    m.get("modelVersions")
        .and_then(Value::as_array)
        .and_then(|vs| vs.iter().filter_map(|v| v.get("publishedAt").and_then(Value::as_str)).max())
        .unwrap_or("")
        .to_string()
}

/// Sort descending by the chosen key; ties keep relevance order (stable sort).
pub fn sort_pool(items: &mut [Value], sort: &str) {
    match sort {
        "Most Downloaded" => items.sort_by_key(|m| std::cmp::Reverse(num(m, "/stats/downloadCount"))),
        "Highest Rated" | "Most Liked" => items.sort_by_key(|m| std::cmp::Reverse(num(m, "/stats/thumbsUpCount"))),
        "Newest" => items.sort_by_cached_key(|m| std::cmp::Reverse(newest(m))),
        _ => {}
    }
}

/// Keep only what search cards and ownership badges need (a pool of 1,000 full models is
/// tens of MB; this keeps it around 1-2 MB).
pub fn trim_model(m: &Value) -> Value {
    let mut out = Map::new();
    for k in ["id", "name", "type", "nsfw", "nsfwLevel", "tags", "stats"] {
        if let Some(v) = m.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    if let Some(u) = m.pointer("/creator/username") {
        out.insert("creator".into(), json!({ "username": u }));
    }
    let versions: Vec<Value> = m
        .get("modelVersions")
        .and_then(Value::as_array)
        .map(|vs| {
            vs.iter()
                .map(|v| {
                    let files: Vec<Value> = v
                        .get("files")
                        .and_then(Value::as_array)
                        .map(|fs| fs.iter().map(|f| json!({ "hashes": { "SHA256": f.pointer("/hashes/SHA256").cloned().unwrap_or(Value::Null) } })).collect())
                        .unwrap_or_default();
                    let images: Vec<Value> = v.get("images").and_then(Value::as_array).map(|a| a.iter().take(3).cloned().collect()).unwrap_or_default();
                    json!({
                        "id": v.get("id").cloned().unwrap_or(Value::Null),
                        "baseModel": v.get("baseModel").cloned().unwrap_or(Value::Null),
                        "publishedAt": v.get("publishedAt").cloned().unwrap_or(Value::Null),
                        "files": files,
                        "images": images,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.insert("modelVersions".into(), Value::Array(versions));
    Value::Object(out)
}

/// Local page cursor for a pooled search: `r<offset>`.
pub fn parse_cursor(c: Option<&str>) -> usize {
    c.and_then(|c| c.strip_prefix('r')).and_then(|n| n.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: i64, dl: i64, likes: i64, dates: &[&str]) -> Value {
        json!({
            "id": id, "name": format!("m{id}"), "stats": {"downloadCount": dl, "thumbsUpCount": likes},
            "modelVersions": dates.iter().map(|d| json!({"id": id * 10, "publishedAt": d, "files": [{"hashes": {"SHA256": "AA"}, "name": "x"}], "images": [1, 2, 3, 4]})).collect::<Vec<_>>(),
            "description": "big"
        })
    }

    #[test]
    fn resort_only_for_keyword_plus_sort() {
        assert!(wants_resort("frieren", Some("Newest")));
        assert!(!wants_resort("  ", Some("Newest")));
        assert!(!wants_resort("frieren", None));
        assert!(!wants_resort("frieren", Some("Relevance")));
    }

    #[test]
    fn sorts_descending_and_stably() {
        let mut v = vec![m(1, 5, 9, &["2024-01-01"]), m(2, 50, 1, &["2023-01-01", "2026-01-01"]), m(3, 5, 3, &["2025-06-01"])];
        sort_pool(&mut v, "Most Downloaded");
        assert_eq!(v.iter().map(|x| x["id"].as_i64().unwrap()).collect::<Vec<_>>(), [2, 1, 3], "tie keeps relevance order");
        sort_pool(&mut v, "Newest");
        assert_eq!(v.iter().map(|x| x["id"].as_i64().unwrap()).collect::<Vec<_>>(), [2, 3, 1], "newest version counts");
        sort_pool(&mut v, "Highest Rated");
        assert_eq!(v[0]["id"], 1);
    }

    #[test]
    fn trim_keeps_card_fields_only() {
        let t = trim_model(&m(1, 5, 9, &["2024-01-01"]));
        assert!(t.get("description").is_none());
        assert_eq!(t["modelVersions"][0]["files"][0]["hashes"]["SHA256"], "AA");
        assert!(t["modelVersions"][0]["files"][0].get("name").is_none());
        assert_eq!(t["modelVersions"][0]["images"].as_array().unwrap().len(), 3);
        assert_eq!(parse_cursor(Some("r80")), 80);
        assert_eq!(parse_cursor(Some("80")), 0);
        assert_eq!(parse_cursor(None), 0);
    }
}
