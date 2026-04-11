//! Property-based tests for the StorageEngine.
//!
//! **Validates: Requirements 9.1, 9.2**

use proptest::prelude::*;
use std::sync::Arc;
use storage::{StorageEngine, StorageEngineConfig};
use tempfile::TempDir;

/// Generate a vector of non-overlapping (offset, data) chunks that fit within
/// `file_size`.  Each chunk is between 1 and `max_chunk` bytes.
fn non_overlapping_chunks(
    file_size: u64,
    max_chunks: usize,
    max_chunk: usize,
) -> BoxedStrategy<Vec<(u64, Vec<u8>)>> {
    // We'll generate a variable number of chunks, then lay them out
    // sequentially so they never overlap.
    (1..=max_chunks)
        .prop_flat_map(move |n| {
            // For each chunk generate a size in [1, max_chunk]
            proptest::collection::vec(1..=max_chunk, n)
        })
        .prop_filter("chunks must fit in file", move |sizes| {
            let total: usize = sizes.iter().sum();
            (total as u64) <= file_size
        })
        .prop_flat_map(|sizes| {
            // For each size, generate random data of that length
            let strats: Vec<_> = sizes
                .into_iter()
                .map(|s| proptest::collection::vec(any::<u8>(), s))
                .collect();
            strats
        })
        .prop_map(|data_vecs| {
            // Lay out chunks sequentially so offsets never overlap
            let mut offset = 0u64;
            let mut chunks = Vec::new();
            for data in data_vecs {
                chunks.push((offset, data.clone()));
                offset += data.len() as u64;
            }
            chunks
        })
        .boxed()
}

// **Property 17: Concurrent Direct-Offset Write Integrity**
//
// Non-overlapping concurrent writes to the same file must all read back
// correctly.
//
// **Validates: Requirements 9.1, 9.2**
proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    #[test]
    fn concurrent_writes_read_back_correctly(
        chunks in non_overlapping_chunks(4096, 8, 512)
    ) {
        if chunks.is_empty() {
            return Ok(());
        }

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let tmp = TempDir::new().unwrap();
            let engine = Arc::new(StorageEngine::new(StorageEngineConfig {
                data_dir: tmp.path().to_path_buf(),
                max_concurrent_writes: 4,
                write_buffer_size: 4096,
            }));

            let file_id = "test-file".to_string();
            let file_size: u64 = chunks.iter().map(|(o, d)| o + d.len() as u64).max().unwrap_or(0);
            engine.allocate_file(file_id.clone(), file_size).await.unwrap();

            // Write all chunks concurrently
            let mut handles = Vec::new();
            for (offset, data) in &chunks {
                let eng = Arc::clone(&engine);
                let fid = file_id.clone();
                let off = *offset;
                let d = data.clone();
                handles.push(tokio::spawn(async move {
                    eng.write_chunk(fid, off, &d).await
                }));
            }

            for h in handles {
                h.await.unwrap().unwrap();
            }

            // Read each chunk back and verify
            for (offset, expected) in &chunks {
                let actual = engine
                    .read_chunk(file_id.clone(), *offset, expected.len())
                    .await
                    .unwrap();
                prop_assert_eq!(&actual, expected);
            }

            Ok(())
        })?;
    }
}
