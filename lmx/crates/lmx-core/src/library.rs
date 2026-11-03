//! In-memory library with fast filtering, sorting, paging and facets.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use serde::Serialize;

use crate::{model_type::ModelType, record::ModelRecord};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Date,
    Size,
}

impl SortKey {
    pub fn parse(s: &str) -> SortKey {
        match s {
            "date" | "modified" => SortKey::Date,
            "size" => SortKey::Size,
            _ => SortKey::Name,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Query {
    pub model_type: Option<ModelType>,
    /// Whitespace-separated terms, all must match (case-insensitive substring).
    pub text: String,
    /// Any of these base models (empty = all).
    pub base_models: Vec<String>,
    pub tag: Option<String>,
    /// Folder relative to root; `None` = everywhere.
    pub folder: Option<String>,
    /// Include subfolders of `folder`.
    pub recursive: bool,
    pub favorites_only: bool,
    /// Only models whose sidecar is damaged or unreadable.
    pub damaged_only: bool,
    pub include_excluded: bool,
    pub sort: SortKey,
    pub descending: bool,
    pub offset: usize,
    pub limit: usize,
}

struct Entry {
    rec: Arc<ModelRecord>,
    hay: String,
    name_lower: String,
}

#[derive(Default)]
pub struct Library {
    entries: Vec<Entry>,
    by_id: HashMap<i64, usize>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Count {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct FolderCount {
    pub path: String,
    /// Models directly in this folder.
    pub direct: usize,
    /// Models in this folder and all subfolders.
    pub total: usize,
}

impl Library {
    pub fn new(records: Vec<ModelRecord>) -> Library {
        let mut entries: Vec<Entry> = records
            .into_iter()
            .map(|r| Entry { hay: r.haystack(), name_lower: r.model_name.to_lowercase(), rec: Arc::new(r) })
            .collect();
        entries.sort_by(|a, b| a.name_lower.cmp(&b.name_lower).then_with(|| a.rec.file_path.cmp(&b.rec.file_path)));
        let by_id = entries.iter().enumerate().map(|(i, e)| (e.rec.id, i)).collect();
        Library { entries, by_id }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, id: i64) -> Option<Arc<ModelRecord>> {
        self.by_id.get(&id).map(|&i| self.entries[i].rec.clone())
    }

    /// All records keyed by scan file key (input for the next incremental scan).
    pub fn previous_map(&self) -> HashMap<String, ModelRecord> {
        self.entries
            .iter()
            .map(|e| (crate::scan::file_key(e.rec.model_type, &e.rec.file_path), (*e.rec).clone()))
            .collect()
    }

    pub fn counts(&self) -> BTreeMap<ModelType, usize> {
        let mut m = BTreeMap::new();
        for e in &self.entries {
            if !e.rec.exclude {
                *m.entry(e.rec.model_type).or_insert(0) += 1;
            }
        }
        m
    }

    fn matches(&self, e: &Entry, q: &Query, terms: &[String]) -> bool {
        let r = &e.rec;
        if q.model_type.is_some_and(|t| t != r.model_type) {
            return false;
        }
        if r.exclude && !q.include_excluded {
            return false;
        }
        if q.favorites_only && !r.favorite {
            return false;
        }
        if q.damaged_only && r.damage.is_none() && r.sidecar_error.is_none() {
            return false;
        }
        if !q.base_models.is_empty() && !q.base_models.iter().any(|b| b.eq_ignore_ascii_case(&r.base_model)) {
            return false;
        }
        if let Some(tag) = &q.tag {
            if !r.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                return false;
            }
        }
        if let Some(folder) = &q.folder {
            let in_folder = if q.recursive {
                folder.is_empty() || r.folder == *folder || r.folder.starts_with(&format!("{folder}/"))
            } else {
                r.folder == *folder
            };
            if !in_folder {
                return false;
            }
        }
        terms.iter().all(|t| e.hay.contains(t.as_str()))
    }

    /// Filter, sort and page. Returns (page, total matches).
    pub fn query(&self, q: &Query) -> (Vec<Arc<ModelRecord>>, usize) {
        let terms: Vec<String> = q.text.to_lowercase().split_whitespace().map(str::to_string).collect();
        let mut hits: Vec<&Entry> = self.entries.iter().filter(|e| self.matches(e, q, &terms)).collect();
        // Entries are pre-sorted by name; only re-sort for other keys.
        match q.sort {
            SortKey::Name => {}
            SortKey::Date => hits.sort_by(|a, b| a.rec.modified.partial_cmp(&b.rec.modified).unwrap_or(Ordering::Equal)),
            SortKey::Size => hits.sort_by_key(|e| e.rec.size),
        }
        if q.descending {
            hits.reverse();
        }
        let total = hits.len();
        let limit = if q.limit == 0 { 100 } else { q.limit.min(500) };
        let page = hits.into_iter().skip(q.offset).take(limit).map(|e| e.rec.clone()).collect();
        (page, total)
    }

    pub fn base_models(&self, t: Option<ModelType>) -> Vec<Count> {
        let mut m: HashMap<&str, usize> = HashMap::new();
        for e in self.entries.iter().filter(|e| t.is_none_or(|t| e.rec.model_type == t) && !e.rec.exclude) {
            *m.entry(e.rec.base_model.as_str()).or_insert(0) += 1;
        }
        let mut v: Vec<Count> = m.into_iter().map(|(name, count)| Count { name: name.to_string(), count }).collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        v
    }

    pub fn tags(&self, t: Option<ModelType>, top: usize) -> Vec<Count> {
        let mut m: HashMap<String, usize> = HashMap::new();
        for e in self.entries.iter().filter(|e| t.is_none_or(|t| e.rec.model_type == t) && !e.rec.exclude) {
            for tag in &e.rec.tags {
                *m.entry(tag.to_lowercase()).or_insert(0) += 1;
            }
        }
        let mut v: Vec<Count> = m.into_iter().map(|(name, count)| Count { name, count }).collect();
        v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
        v.truncate(top);
        v
    }

    /// Every folder (including intermediate ones) with direct and recursive counts, sorted by path.
    pub fn folders(&self, t: Option<ModelType>) -> Vec<FolderCount> {
        let mut direct: BTreeMap<String, usize> = BTreeMap::new();
        let mut total: BTreeMap<String, usize> = BTreeMap::new();
        for e in self.entries.iter().filter(|e| t.is_none_or(|t| e.rec.model_type == t) && !e.rec.exclude) {
            *direct.entry(e.rec.folder.clone()).or_insert(0) += 1;
            let mut acc = String::new();
            *total.entry(String::new()).or_insert(0) += 1;
            if !e.rec.folder.is_empty() {
                for part in e.rec.folder.split('/') {
                    if !acc.is_empty() {
                        acc.push('/');
                    }
                    acc.push_str(part);
                    *total.entry(acc.clone()).or_insert(0) += 1;
                }
            }
        }
        total
            .into_iter()
            .map(|(path, total)| FolderCount { direct: direct.get(&path).copied().unwrap_or(0), path, total })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::FileFacts;
    use std::path::Path;

    fn rec(id: i64, name: &str, folder: &str, base: &str, size: u64, modified: f64) -> ModelRecord {
        let path = if folder.is_empty() { format!("H:/L/{name}.safetensors") } else { format!("H:/L/{folder}/{name}.safetensors") };
        let facts = FileFacts {
            model_type: ModelType::Lora,
            path: Path::new(&path),
            root: Path::new("H:/L"),
            size,
            modified,
            sidecar_mtime: None,
            sidecar_size: None,
        };
        let mut r = ModelRecord::build(&facts, None, None);
        r.id = id;
        r.base_model = base.into();
        r
    }

    fn lib() -> Library {
        let mut a = rec(1, "Alpha", "Char", "Illustrious", 30, 3.0);
        a.tags = vec!["girl".into()];
        a.trained_words = vec!["alpha_trigger".into()];
        let mut b = rec(2, "beta", "Char/Sub", "Pony", 10, 1.0);
        b.favorite = true;
        let c = rec(3, "Gamma", "", "Illustrious", 20, 2.0);
        let mut c = c;
        c.damage = Some(crate::sidecar::Damage::TrailingData);
        let mut x = rec(4, "Hidden", "", "Illustrious", 5, 9.0);
        x.exclude = true;
        let mut ck = rec(5, "Ckpt", "", "SDXL", 1, 1.0);
        ck.model_type = ModelType::Checkpoint;
        Library::new(vec![c, b, a, x, ck])
    }

    fn names(page: &[Arc<ModelRecord>]) -> Vec<&str> {
        page.iter().map(|r| r.model_name.as_str()).collect()
    }

    #[test]
    fn default_query_is_name_sorted_case_insensitive_and_hides_excluded() {
        let l = lib();
        let (page, total) = l.query(&Query { model_type: Some(ModelType::Lora), ..Default::default() });
        assert_eq!(total, 3);
        assert_eq!(names(&page), vec!["Alpha", "beta", "Gamma"]);
    }

    #[test]
    fn text_search_covers_trigger_words_and_requires_all_terms() {
        let l = lib();
        let q = |t: &str| l.query(&Query { text: t.into(), ..Default::default() }).1;
        assert_eq!(q("ALPHA_trig"), 1);
        assert_eq!(q("alpha girl"), 1);
        assert_eq!(q("alpha pony"), 0);
    }

    #[test]
    fn filters_base_folder_recursive_favorites_tag() {
        let l = lib();
        let base = l.query(&Query { base_models: vec!["illustrious".into()], model_type: Some(ModelType::Lora), ..Default::default() });
        assert_eq!(names(&base.0), vec!["Alpha", "Gamma"]);
        let flat = l.query(&Query { folder: Some("Char".into()), ..Default::default() });
        assert_eq!(names(&flat.0), vec!["Alpha"]);
        let rec = l.query(&Query { folder: Some("Char".into()), recursive: true, ..Default::default() });
        assert_eq!(names(&rec.0), vec!["Alpha", "beta"]);
        let fav = l.query(&Query { favorites_only: true, ..Default::default() });
        assert_eq!(names(&fav.0), vec!["beta"]);
        let dmg = l.query(&Query { damaged_only: true, ..Default::default() });
        assert_eq!(names(&dmg.0), vec!["Gamma"]);
        let tag = l.query(&Query { tag: Some("GIRL".into()), ..Default::default() });
        assert_eq!(names(&tag.0), vec!["Alpha"]);
    }

    #[test]
    fn sort_and_paging() {
        let l = lib();
        let q = Query { model_type: Some(ModelType::Lora), sort: SortKey::Size, descending: true, offset: 1, limit: 1, ..Default::default() };
        let (page, total) = l.query(&q);
        assert_eq!(total, 3);
        assert_eq!(names(&page), vec!["Gamma"]);
        let (by_date, _) = l.query(&Query { model_type: Some(ModelType::Lora), sort: SortKey::Date, ..Default::default() });
        assert_eq!(names(&by_date), vec!["beta", "Gamma", "Alpha"]);
    }

    #[test]
    fn facets_and_folders() {
        let l = lib();
        assert_eq!(l.base_models(Some(ModelType::Lora))[0], Count { name: "Illustrious".into(), count: 2 });
        let f = l.folders(Some(ModelType::Lora));
        let get = |p: &str| f.iter().find(|x| x.path == p).unwrap();
        assert_eq!((get("").direct, get("").total), (1, 3));
        assert_eq!((get("Char").direct, get("Char").total), (1, 2));
        assert_eq!((get("Char/Sub").direct, get("Char/Sub").total), (1, 1));
        assert_eq!(l.counts()[&ModelType::Lora], 3);
        assert_eq!(l.get(2).unwrap().model_name, "beta");
    }
}
