//! Content hashing used for change detection.

use sha2::{Digest, Sha256};

/// Lowercase hex SHA-256 of `bytes`.
#[must_use]
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        assert_eq!(
            content_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn differs_for_different_content() {
        assert_ne!(content_hash(b"a"), content_hash(b"b"));
    }
}
