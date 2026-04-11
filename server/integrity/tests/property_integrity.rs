//! Property tests for the IntegrityVerifier.

use proptest::prelude::*;

use integrity::{ChunkHash, HashAlgorithm, IntegrityError, IntegrityVerifier};

/// Helper: create a default verifier.
fn verifier() -> IntegrityVerifier {
    IntegrityVerifier::new(HashAlgorithm::Sha256)
}

proptest! {
    /// **Validates: Requirements 6.2, 6.3**
    ///
    /// Property 11: Chunk Hash Verification — hashing data then verifying the
    /// same data against that hash succeeds. Verifying against a different hash
    /// fails with `ChunkMismatch`.
    #[test]
    fn chunk_hash_verification(
        data in proptest::collection::vec(any::<u8>(), 0..4096),
        other_data in proptest::collection::vec(any::<u8>(), 1..4096),
    ) {
        let v = verifier();
        let hash = v.hash_chunk(&data, HashAlgorithm::Sha256);

        // Verifying the same data against its own hash must succeed.
        v.verify_chunk(&data, &hash).expect("verify same data should succeed");

        // Verifying different data against the original hash should fail,
        // unless the two byte sequences happen to be identical.
        if data != other_data {
            let result = v.verify_chunk(&other_data, &hash);
            match result {
                Err(IntegrityError::ChunkMismatch { expected, actual }) => {
                    prop_assert_eq!(&expected, &hash.value);
                    prop_assert_ne!(&actual, &hash.value);
                }
                Err(e) => {
                    prop_assert!(false, "unexpected error variant: {}", e);
                }
                Ok(()) => {
                    // SHA-256 collision — astronomically unlikely but technically possible.
                    // If this ever triggers, buy a lottery ticket.
                    prop_assert!(false, "SHA-256 collision detected — this should be impossible");
                }
            }
        }
    }

    /// **Validates: Requirements 6.5**
    ///
    /// Property 12: Hash Algorithm Identifier Inclusion — Every `ChunkHash`
    /// produced by the IntegrityVerifier includes a non-empty `algorithm` field.
    #[test]
    fn hash_algorithm_identifier_inclusion(
        data in proptest::collection::vec(any::<u8>(), 0..4096),
    ) {
        let v = verifier();
        let hash: ChunkHash = v.hash_chunk(&data, HashAlgorithm::Sha256);

        // The algorithm field must be present and identifiable.
        let algo_str = format!("{}", hash.algorithm);
        prop_assert!(!algo_str.is_empty(), "algorithm display string must be non-empty");

        // The hash value itself must also be non-empty.
        prop_assert!(!hash.value.is_empty(), "hash value must be non-empty");

        // The algorithm must match what we requested.
        prop_assert_eq!(hash.algorithm, HashAlgorithm::Sha256);
    }
}
