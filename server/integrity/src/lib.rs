//! Integrity verifier: per-chunk and whole-file hash verification.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Supported hash algorithms.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
pub enum HashAlgorithm {
    Sha256,
}

impl std::fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HashAlgorithm::Sha256 => write!(f, "sha256"),
        }
    }
}

/// A hash value paired with the algorithm that produced it.
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct ChunkHash {
    pub algorithm: HashAlgorithm,
    pub value: String,
}

/// Errors returned by integrity verification operations.
#[derive(Debug, thiserror::Error)]
pub enum IntegrityError {
    #[error("Chunk hash mismatch: expected {expected}, got {actual}")]
    ChunkMismatch { expected: String, actual: String },
    #[error("File hash mismatch: expected {expected}, got {actual}")]
    FileMismatch { expected: String, actual: String },
}

/// Computes and verifies cryptographic hashes for chunks and whole files.
pub struct IntegrityVerifier {
    default_algorithm: HashAlgorithm,
}

impl IntegrityVerifier {
    /// Create a new verifier with the given default algorithm.
    pub fn new(default_algorithm: HashAlgorithm) -> Self {
        Self { default_algorithm }
    }

    /// Return the default hash algorithm.
    pub fn default_algorithm(&self) -> HashAlgorithm {
        self.default_algorithm
    }

    /// Compute the hash of a data chunk using the specified algorithm.
    pub fn hash_chunk(&self, data: &[u8], algo: HashAlgorithm) -> ChunkHash {
        let value = match algo {
            HashAlgorithm::Sha256 => {
                let mut hasher = Sha256::new();
                hasher.update(data);
                hex::encode(hasher.finalize())
            }
        };
        ChunkHash {
            algorithm: algo,
            value,
        }
    }

    /// Verify that `data` matches the expected hash.
    /// Returns `Ok(())` on match, `Err(IntegrityError::ChunkMismatch)` otherwise.
    pub fn verify_chunk(&self, data: &[u8], expected: &ChunkHash) -> Result<(), IntegrityError> {
        let actual = self.hash_chunk(data, expected.algorithm);
        if actual.value == expected.value {
            Ok(())
        } else {
            Err(IntegrityError::ChunkMismatch {
                expected: expected.value.clone(),
                actual: actual.value,
            })
        }
    }

    /// Verify a whole file by hashing concatenated chunk data.
    ///
    /// This is a simplified version that takes an iterator of chunk data slices
    /// rather than requiring a StorageEngine (which doesn't exist yet).
    /// The full async version will be wired up when StorageEngine is available.
    pub fn verify_file_from_chunks<'a>(
        &self,
        chunks: impl Iterator<Item = &'a [u8]>,
        expected_hash: &str,
    ) -> Result<(), IntegrityError> {
        let mut hasher = Sha256::new();
        for chunk in chunks {
            hasher.update(chunk);
        }
        let actual = hex::encode(hasher.finalize());
        if actual == expected_hash {
            Ok(())
        } else {
            Err(IntegrityError::FileMismatch {
                expected: expected_hash.to_string(),
                actual,
            })
        }
    }
}
