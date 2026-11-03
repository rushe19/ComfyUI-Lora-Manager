//! File hashing primitives for lmx.

use std::{fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

/// Read buffer size. Large sequential reads keep NVMe queues busy.
pub const BUF_SIZE: usize = 4 * 1024 * 1024;

/// SHA256 of a file as lowercase hex (same form LoRA Manager stores in sidecars).
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; BUF_SIZE];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_vectors() {
        let dir = std::env::temp_dir().join(format!("lmx-scan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty.bin");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(
            sha256_file(&empty).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let abc = dir.join("abc.bin");
        std::fs::write(&abc, b"abc").unwrap();
        assert_eq!(
            sha256_file(&abc).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_spans_multiple_buffers() {
        let dir = std::env::temp_dir().join(format!("lmx-scan-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("big.bin");
        let data = vec![7u8; BUF_SIZE * 2 + 123];
        std::fs::write(&p, &data).unwrap();
        let expected = hex::encode(Sha256::digest(&data));
        assert_eq!(sha256_file(&p).unwrap(), expected);
    }
}
