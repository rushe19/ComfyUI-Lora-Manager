//! Phase 0 spike: measure SHA256 throughput on real model files.
//!
//! Usage: hashbench <file-list.txt> <threads> [expected.tsv]
//! file-list: one absolute path per line. expected.tsv: `<path>\t<sha256>` to verify results.

use std::{collections::HashMap, path::PathBuf, time::Instant};

use rayon::prelude::*;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(args.len() >= 3, "usage: hashbench <file-list> <threads> [expected.tsv]");
    let files: Vec<PathBuf> = std::fs::read_to_string(&args[1])?
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    let threads: usize = args[2].parse()?;
    let expected: HashMap<String, String> = match args.get(3) {
        Some(p) => std::fs::read_to_string(p)?
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(a, b)| (a.to_string(), b.to_lowercase()))
            .collect(),
        None => HashMap::new(),
    };

    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let start = Instant::now();
    let results: Vec<(PathBuf, u64, std::io::Result<String>)> = pool.install(|| {
        files
            .par_iter()
            .map(|p| {
                let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                (p.clone(), size, lmx_scan::sha256_file(p))
            })
            .collect()
    });
    let secs = start.elapsed().as_secs_f64();

    let total: u64 = results.iter().map(|r| r.1).sum();
    let mut mismatches = 0;
    let mut errors = 0;
    for (p, _, h) in &results {
        match h {
            Ok(h) => {
                if let Some(e) = expected.get(&p.to_string_lossy().to_string()) {
                    if e != h {
                        mismatches += 1;
                        eprintln!("MISMATCH {}", p.display());
                    }
                }
            }
            Err(e) => {
                errors += 1;
                eprintln!("ERROR {}: {e}", p.display());
            }
        }
    }
    println!(
        "threads={threads} files={} bytes={:.2}GB secs={secs:.2} MBps={:.0} per_file={:.3}s verified={} mismatches={mismatches} errors={errors}",
        results.len(),
        total as f64 / 1e9,
        total as f64 / 1e6 / secs,
        secs / results.len().max(1) as f64,
        expected.len()
    );
    Ok(())
}
