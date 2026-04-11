//! Property tests for protocol versioning, unknown version rejection, and malformed binary handling.

use proptest::prelude::*;

use prost::Message as ProstMessage;
use protocol::proto::envelope::Payload;
use protocol::{Envelope, ProtocolCodec, ProtocolError, UploadRequest};

/// Reusable strategy: generate a simple valid payload for encoding.
fn arb_simple_payload() -> impl Strategy<Value = Payload> {
    (
        "[a-zA-Z0-9_.]{1,32}",
        any::<u64>(),
        "application/octet-stream",
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

proptest! {
    /// **Validates: Requirements 2.2**
    ///
    /// Property 2: Envelope Version Field Presence — Encoded envelope contains
    /// version matching codec's current version.
    ///
    /// Encode a payload via the codec, then decode the raw bytes directly as an
    /// Envelope using prost (bypassing the codec) and verify the version field
    /// matches the codec's `current_version`.
    #[test]
    fn envelope_version_field_presence(
        payload in arb_simple_payload(),
        current_version in 1u32..=100,
        correlation_id in "[a-f0-9]{8}-[a-f0-9]{4}",
    ) {
        let codec = ProtocolCodec::new(current_version, 1..=100);

        let encoded = codec.encode(&payload, &correlation_id)
            .expect("encoding should succeed");

        // Decode raw bytes directly via prost, not the codec
        let envelope = Envelope::decode(encoded.as_slice())
            .expect("prost decode should succeed on validly encoded data");

        prop_assert_eq!(envelope.version, current_version,
            "envelope version {} should match codec current_version {}",
            envelope.version, current_version);
    }

    /// **Validates: Requirements 2.3**
    ///
    /// Property 3: Unknown Version Rejection — Versions outside supported range
    /// return `UnsupportedVersion` error.
    ///
    /// Create an Envelope with a version outside the supported range, encode it
    /// to bytes, then try to decode via the codec and verify it returns
    /// `UnsupportedVersion`.
    #[test]
    fn unknown_version_rejection(
        payload in arb_simple_payload(),
        correlation_id in "[a-f0-9]{8}-[a-f0-9]{4}",
        bad_version in prop_oneof![
            0u32..1u32,        // below supported range
            11u32..=u32::MAX,  // above supported range
        ],
    ) {
        // Codec supports versions 1..=10
        let codec = ProtocolCodec::new(1, 1..=10);

        // Manually build an envelope with the bad version
        let envelope = Envelope {
            version: bad_version,
            correlation_id: correlation_id.clone(),
            payload: Some(payload),
        };
        let mut buf = Vec::with_capacity(envelope.encoded_len());
        envelope.encode(&mut buf).expect("prost encode should succeed");

        // Attempt to decode via the codec
        let result = codec.decode(&buf);

        match result {
            Err(ProtocolError::UnsupportedVersion { version }) => {
                prop_assert_eq!(version, bad_version,
                    "error should contain the offending version {}, got {}",
                    bad_version, version);
            }
            other => {
                prop_assert!(false,
                    "expected UnsupportedVersion error for version {}, got {:?}",
                    bad_version, other);
            }
        }
    }

    /// **Validates: Requirements 22.4**
    ///
    /// Property 4: Malformed Binary Parse Error with Byte Offset — Invalid byte
    /// sequences return `ParseError` with byte offset.
    ///
    /// Generate random byte sequences that are NOT valid protobuf Envelopes,
    /// try to decode them, and verify a `ParseError` is returned.
    #[test]
    fn malformed_binary_parse_error(
        data in proptest::collection::vec(any::<u8>(), 0..512)
            .prop_filter("must not be a valid Envelope", |bytes| {
                // Filter out byte sequences that happen to parse as valid Envelopes
                Envelope::decode(bytes.as_slice()).is_err()
            }),
    ) {
        let codec = ProtocolCodec::new(1, 1..=10);

        let result = codec.decode(&data);

        match result {
            Err(ProtocolError::ParseError { offset: _, reason }) => {
                // ParseError returned — verify it has a non-empty reason
                prop_assert!(!reason.is_empty(),
                    "ParseError reason should not be empty");
            }
            other => {
                prop_assert!(false,
                    "expected ParseError for malformed data of len {}, got {:?}",
                    data.len(), other);
            }
        }
    }
}
