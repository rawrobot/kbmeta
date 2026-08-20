use sha2::{Digest, Sha256};

/// Computes a lowercase hexadecimal SHA-256 hash for the input content.
pub fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
