//! Read-only check: does parse -> to_bytes reproduce real sidecars byte for byte?
//! `cargo run --release -p lmx-core --example roundtrip -- <models root>`

use std::collections::BTreeMap;

fn main() {
    let root = std::env::args().nth(1).expect("root folder");
    let mut stats: BTreeMap<&str, usize> = BTreeMap::new();
    let mut samples: Vec<String> = Vec::new();
    for e in walkdir::WalkDir::new(&root).into_iter().filter_map(Result::ok) {
        let p = e.path();
        if !p.to_string_lossy().ends_with(".metadata.json") {
            continue;
        }
        let bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(_) => {
                *stats.entry("io_error").or_default() += 1;
                continue;
            }
        };
        match lmx_core::sidecar::parse(&bytes) {
            Ok(r) if r.damage.is_some() => *stats.entry("damaged").or_default() += 1,
            Ok(r) => {
                if lmx_core::sidecar::to_bytes(&r.data) == bytes {
                    *stats.entry("identical").or_default() += 1;
                } else {
                    *stats.entry("different").or_default() += 1;
                    if samples.len() < 5 {
                        let out = lmx_core::sidecar::to_bytes(&r.data);
                        let at = bytes.iter().zip(out.iter()).position(|(a, b)| a != b).unwrap_or(bytes.len().min(out.len()));
                        let lo = at.saturating_sub(40);
                        samples.push(format!(
                            "{}\n  orig: {:?}\n  ours: {:?}",
                            p.display(),
                            String::from_utf8_lossy(&bytes[lo..(at + 40).min(bytes.len())]),
                            String::from_utf8_lossy(&out[lo..(at + 40).min(out.len())])
                        ));
                    }
                }
            }
            Err(_) => *stats.entry("invalid").or_default() += 1,
        }
    }
    println!("{stats:?}");
    for s in samples {
        println!("{s}");
    }
}
