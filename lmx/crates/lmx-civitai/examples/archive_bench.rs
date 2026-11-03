//! `cargo run --release -p lmx-civitai --example archive_bench -- <civitai.sqlite> <idx.sqlite> <lmx index.sqlite>`

use std::time::Instant;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let arch = lmx_civitai::archive::Archive::open(a[1].as_ref(), a[2].as_ref())?;
    println!("open: {:.2}s rows={} rebuilt={:?}", t.elapsed().as_secs_f64(), arch.rows, arch.rebuilt_secs);
    let idx = rusqlite::Connection::open_with_flags(&a[3], rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut q = idx.prepare("SELECT record FROM models")?;
    let recs: Vec<serde_json::Value> = q.query_map([], |r| r.get::<_, String>(0))?.filter_map(|r| r.ok()).filter_map(|s| serde_json::from_str(&s).ok()).collect();
    let (mut n, mut hit, mut hit_unlinked, mut unlinked) = (0, 0, 0, 0);
    let t = Instant::now();
    let mut sample = None;
    for r in &recs {
        let Some(sha) = r["sha256"].as_str() else { continue };
        n += 1;
        let linked = r["civitai_version_id"].is_i64();
        if !linked {
            unlinked += 1;
        }
        if let Some(found) = arch.lookup(sha) {
            hit += 1;
            if !linked {
                hit_unlinked += 1;
                sample.get_or_insert(found);
            }
        }
    }
    println!("lookup {n} hashes: {:.3}s, hits {hit}, unlinked {unlinked} of which in archive {hit_unlinked}", t.elapsed().as_secs_f64());
    if let Some((m, v)) = sample {
        let t = Instant::now();
        let (ver, _) = arch.version(m, v)?.unwrap();
        println!("version payload {m}/{v}: {:.1} ms, name={} base={} files={}", t.elapsed().as_secs_f64() * 1000.0, ver["model"]["name"], ver["baseModel"], ver["files"].as_array().map(|f| f.len()).unwrap_or(0));
    }
    Ok(())
}
