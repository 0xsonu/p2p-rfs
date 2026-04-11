//! Property test: Protocol Message Round-Trip
//!
//! **Validates: Requirements 2.5, 22.1, 22.2, 22.3**
//!
//! Property 1: For any valid typed message, `decode(encode(msg)) == msg`.

use proptest::prelude::*;

use protocol::proto::envelope::Payload;
use protocol::{
    AuthRequest, AuthResponse, ChunkAck, ChunkData, DownloadAck, DownloadComplete,
    DownloadRequest, ErrorResponse, FileInfo, FileListRequest, FileListResponse,
    RevokeSessionRequest, SessionInfo, SessionListRequest, SessionListResponse,
    TransferHistoryEntry, TransferHistoryRequest, TransferHistoryResponse, UploadAck,
    UploadComplete, UploadRequest, ProtocolCodec,
};

/// Strategy for generating arbitrary AuthRequest payloads.
fn arb_auth_request() -> impl Strategy<Value = Payload> {
    ("[a-zA-Z0-9]{1,32}", "[a-zA-Z0-9]{1,64}").prop_map(|(username, password)| {
        Payload::AuthRequest(AuthRequest { username, password })
    })
}

/// Strategy for generating arbitrary AuthResponse payloads.
fn arb_auth_response() -> impl Strategy<Value = Payload> {
    ("[a-zA-Z0-9]{16,64}", any::<u64>(), "(admin|standard)").prop_map(
        |(session_token, expires_at, role)| {
            Payload::AuthResponse(AuthResponse {
                session_token,
                expires_at,
                role,
            })
        },
    )
}

/// Strategy for generating arbitrary UploadRequest payloads.
fn arb_upload_request() -> impl Strategy<Value = Payload> {
    (
        "[a-zA-Z0-9_.]{1,64}",
        any::<u64>(),
        "(application/octet-stream|text/plain|image/png)",
        "[a-f0-9]{64}",
        "sha256",
    )
        .prop_map(
            |(filename, file_size, mime_type, whole_file_hash, hash_algorithm)| {
                Payload::UploadRequest(UploadRequest {
                    filename,
                    file_size,
                    mime_type,
                    whole_file_hash,
                    hash_algorithm,
                })
            },
        )
}

/// Strategy for generating arbitrary UploadAck payloads.
fn arb_upload_ack() -> impl Strategy<Value = Payload> {
    ("[a-f0-9]{32}", any::<u64>(), any::<u64>()).prop_map(
        |(session_id, chunk_size, total_chunks)| {
            Payload::UploadAck(UploadAck {
                session_id,
                chunk_size,
                total_chunks,
            })
        },
    )
}

/// Strategy for generating arbitrary ChunkData payloads.
fn arb_chunk_data() -> impl Strategy<Value = Payload> {
    (
        "[a-f0-9]{32}",
        any::<u64>(),
        any::<u64>(),
        proptest::collection::vec(any::<u8>(), 0..256),
        "[a-f0-9]{64}",
        "sha256",
    )
        .prop_map(
            |(session_id, chunk_index, offset, data, hash, hash_algorithm)| {
                Payload::ChunkData(ChunkData {
                    session_id,
                    chunk_index,
                    offset,
                    data,
                    hash,
                    hash_algorithm,
                })
            },
        )
}

/// Strategy for generating arbitrary ChunkAck payloads.
fn arb_chunk_ack() -> impl Strategy<Value = Payload> {
    ("[a-f0-9]{32}", any::<u64>()).prop_map(|(session_id, chunk_index)| {
        Payload::ChunkAck(ChunkAck {
            session_id,
            chunk_index,
        })
    })
}

/// Strategy for generating arbitrary UploadComplete payloads.
fn arb_upload_complete() -> impl Strategy<Value = Payload> {
    ("[a-f0-9]{32}", "[a-f0-9]{32}", "[a-f0-9]{64}").prop_map(
        |(session_id, file_id, whole_file_hash)| {
            Payload::UploadComplete(UploadComplete {
                session_id,
                file_id,
                whole_file_hash,
            })
        },
    )
}

/// Strategy for generating arbitrary DownloadRequest payloads.
fn arb_download_request() -> impl Strategy<Value = Payload> {
    "[a-f0-9]{32}".prop_map(|file_id| Payload::DownloadRequest(DownloadRequest { file_id }))
}

/// Strategy for generating arbitrary DownloadAck payloads.
fn arb_download_ack() -> impl Strategy<Value = Payload> {
    (
        "[a-f0-9]{32}",
        "[a-f0-9]{32}",
        "[a-zA-Z0-9_.]{1,64}",
        any::<u64>(),
        any::<u64>(),
        any::<u64>(),
        "[a-f0-9]{64}",
    )
        .prop_map(
            |(session_id, file_id, filename, file_size, chunk_size, total_chunks, whole_file_hash)| {
                Payload::DownloadAck(DownloadAck {
                    session_id,
                    file_id,
                    filename,
                    file_size,
                    chunk_size,
                    total_chunks,
                    whole_file_hash,
                })
            },
        )
}

/// Strategy for generating arbitrary DownloadComplete payloads.
fn arb_download_complete() -> impl Strategy<Value = Payload> {
    ("[a-f0-9]{32}", "[a-f0-9]{32}").prop_map(|(session_id, file_id)| {
        Payload::DownloadComplete(DownloadComplete {
            session_id,
            file_id,
        })
    })
}

/// Strategy for generating arbitrary ErrorResponse payloads.
fn arb_error_response() -> impl Strategy<Value = Payload> {
    (
        any::<u32>(),
        "[a-zA-Z0-9 ]{1,128}",
        proptest::option::of(any::<u64>()),
        proptest::option::of("[a-zA-Z0-9 ]{1,64}"),
    )
        .prop_map(|(code, message, byte_offset, retry_hint)| {
            Payload::ErrorResponse(ErrorResponse {
                code,
                message,
                byte_offset,
                retry_hint,
            })
        })
}

/// Strategy for generating arbitrary SessionListRequest payloads.
fn arb_session_list_request() -> impl Strategy<Value = Payload> {
    Just(Payload::SessionListRequest(SessionListRequest {}))
}

/// Strategy for generating arbitrary SessionListResponse payloads.
fn arb_session_list_response() -> impl Strategy<Value = Payload> {
    proptest::collection::vec(
        (
            "[a-f0-9]{32}",
            "[a-zA-Z0-9 ]{1,32}",
            "(active|inactive)",
            any::<u64>(),
        )
            .prop_map(|(session_id, device_name, status, last_active)| SessionInfo {
                session_id,
                device_name,
                status,
                last_active,
            }),
        0..5,
    )
    .prop_map(|sessions| Payload::SessionListResponse(SessionListResponse { sessions }))
}

/// Strategy for generating arbitrary RevokeSessionRequest payloads.
fn arb_revoke_session_request() -> impl Strategy<Value = Payload> {
    "[a-f0-9]{32}"
        .prop_map(|session_id| Payload::RevokeSessionRequest(RevokeSessionRequest { session_id }))
}

/// Strategy for generating arbitrary FileListRequest payloads.
fn arb_file_list_request() -> impl Strategy<Value = Payload> {
    Just(Payload::FileListRequest(FileListRequest {}))
}

/// Strategy for generating arbitrary FileListResponse payloads.
fn arb_file_list_response() -> impl Strategy<Value = Payload> {
    proptest::collection::vec(
        (
            "[a-f0-9]{32}",
            "[a-zA-Z0-9_.]{1,64}",
            any::<u64>(),
            "(application/octet-stream|text/plain)",
            any::<u64>(),
            "[a-zA-Z0-9]{1,32}",
        )
            .prop_map(
                |(file_id, filename, size, mime_type, uploaded_at, uploaded_by)| FileInfo {
                    file_id,
                    filename,
                    size,
                    mime_type,
                    uploaded_at,
                    uploaded_by,
                },
            ),
        0..5,
    )
    .prop_map(|files| Payload::FileListResponse(FileListResponse { files }))
}

/// Strategy for generating arbitrary TransferHistoryRequest payloads.
fn arb_transfer_history_request() -> impl Strategy<Value = Payload> {
    Just(Payload::HistoryRequest(TransferHistoryRequest {}))
}

/// Strategy for generating arbitrary TransferHistoryResponse payloads.
fn arb_transfer_history_response() -> impl Strategy<Value = Payload> {
    proptest::collection::vec(
        (
            "[a-f0-9]{32}",
            "[a-f0-9]{32}",
            "[a-zA-Z0-9_.]{1,64}",
            "(upload|download)",
            any::<u64>(),
            "(completed|failed)",
            any::<u64>(),
            any::<u64>(),
            // Use finite f64 values only — NaN breaks PartialEq round-trip
            (0u64..=u64::MAX).prop_map(|bits| f64::from_bits(bits))
                .prop_filter("must be finite", |v| v.is_finite()),
        )
            .prop_map(
                |(
                    session_id,
                    file_id,
                    filename,
                    direction,
                    file_size,
                    status,
                    started_at,
                    completed_at,
                    avg_throughput_bps,
                )| {
                    TransferHistoryEntry {
                        session_id,
                        file_id,
                        filename,
                        direction,
                        file_size,
                        status,
                        started_at,
                        completed_at,
                        avg_throughput_bps,
                    }
                },
            ),
        0..5,
    )
    .prop_map(|entries| {
        Payload::HistoryResponse(TransferHistoryResponse { entries })
    })
}

/// Combined strategy that generates any valid Payload variant using prop_oneof!
fn arb_payload() -> impl Strategy<Value = Payload> {
    prop_oneof![
        arb_auth_request(),
        arb_auth_response(),
        arb_upload_request(),
        arb_upload_ack(),
        arb_chunk_data(),
        arb_chunk_ack(),
        arb_upload_complete(),
        arb_download_request(),
        arb_download_ack(),
        arb_download_complete(),
        arb_error_response(),
        arb_session_list_request(),
        arb_session_list_response(),
        arb_revoke_session_request(),
        arb_file_list_request(),
        arb_file_list_response(),
        arb_transfer_history_request(),
        arb_transfer_history_response(),
    ]
}

proptest! {
    /// **Validates: Requirements 2.5, 22.1, 22.2, 22.3**
    ///
    /// Property 1: Protocol Message Round-Trip — For any valid typed message,
    /// `decode(encode(msg)) == msg`.
    #[test]
    fn protocol_message_round_trip(
        payload in arb_payload(),
        correlation_id in "[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}",
    ) {
        let codec = ProtocolCodec::new(1, 1..=1);

        // Encode the payload
        let encoded = codec.encode(&payload, &correlation_id)
            .expect("encoding should succeed for any valid payload");

        // Decode the bytes back
        let (decoded_payload, decoded_correlation_id) = codec.decode(&encoded)
            .expect("decoding should succeed for validly encoded data");

        // Assert the decoded payload equals the original
        prop_assert_eq!(&decoded_payload, &payload);

        // Assert the decoded correlation_id equals the original
        prop_assert_eq!(&decoded_correlation_id, &correlation_id);
    }
}
