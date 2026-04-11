use std::path::{Path, PathBuf};
use std::sync::Arc;

use rcgen::generate_simple_self_signed;
use ring::digest::{digest, SHA256};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

const CERT_FILENAME: &str = "cert.der";
const KEY_FILENAME: &str = "key.der";

/// Errors that can occur during certificate operations.
#[derive(Debug, thiserror::Error)]
pub enum CertError {
    #[error("certificate generation failed: {0}")]
    GenerationFailed(String),
    #[error("certificate persistence failed: {0}")]
    PersistenceFailed(String),
}

/// Manages self-signed TLS certificates for QUIC connections.
///
/// On first launch, generates a new self-signed certificate and persists it
/// as DER files. On subsequent launches, loads the existing certificate.
/// Provides TLS configurations for both server (QUIC listener) and client
/// (outgoing connections with trust-on-first-use).
pub struct CertManager {
    cert_path: PathBuf,
    key_path: PathBuf,
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
    fingerprint: String,
}

impl CertManager {
    /// Load existing cert/key from disk, or generate new ones on first launch.
    pub fn load_or_generate(data_dir: &Path) -> Result<Self, CertError> {
        let cert_path = data_dir.join(CERT_FILENAME);
        let key_path = data_dir.join(KEY_FILENAME);

        let (cert_der, key_der) = if cert_path.exists() && key_path.exists() {
            let cert_der = std::fs::read(&cert_path)
                .map_err(|e| CertError::PersistenceFailed(format!("read cert: {e}")))?;
            let key_der = std::fs::read(&key_path)
                .map_err(|e| CertError::PersistenceFailed(format!("read key: {e}")))?;
            (cert_der, key_der)
        } else {
            let (cert_der, key_der) = Self::generate()?;
            Self::persist(data_dir, &cert_path, &key_path, &cert_der, &key_der)?;
            (cert_der, key_der)
        };

        let fingerprint = Self::compute_fingerprint(&cert_der);

        Ok(Self {
            cert_path,
            key_path,
            cert_der,
            key_der,
            fingerprint,
        })
    }

    /// Get the certificate fingerprint (SHA-256 of DER-encoded cert).
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Build a `rustls::ServerConfig` for the QUIC listener.
    pub fn server_tls_config(&self) -> Result<rustls::ServerConfig, CertError> {
        let cert_chain = vec![CertificateDer::from(self.cert_der.clone())];
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()));

        rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| CertError::GenerationFailed(format!("protocol versions: {e}")))?
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| CertError::GenerationFailed(format!("server config: {e}")))
    }

    /// Build a `rustls::ClientConfig` that trusts any self-signed cert (TOFU model).
    pub fn client_tls_config(&self) -> Result<rustls::ClientConfig, CertError> {
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| CertError::GenerationFailed(format!("protocol versions: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(TofuCertVerifier))
        .with_no_client_auth();

        Ok(config)
    }

    /// Generate a new self-signed certificate and private key.
    fn generate() -> Result<(Vec<u8>, Vec<u8>), CertError> {
        let subject_alt_names = vec!["fileshare".to_string(), "localhost".to_string()];
        let certified_key = generate_simple_self_signed(subject_alt_names)
            .map_err(|e| CertError::GenerationFailed(e.to_string()))?;

        let cert_der = certified_key.cert.der().to_vec();
        let key_der = certified_key.key_pair.serialize_der();

        Ok((cert_der, key_der))
    }

    /// Persist cert and key as DER files to disk.
    fn persist(
        data_dir: &Path,
        cert_path: &Path,
        key_path: &Path,
        cert_der: &[u8],
        key_der: &[u8],
    ) -> Result<(), CertError> {
        std::fs::create_dir_all(data_dir)
            .map_err(|e| CertError::PersistenceFailed(format!("create dir: {e}")))?;
        std::fs::write(cert_path, cert_der)
            .map_err(|e| CertError::PersistenceFailed(format!("write cert: {e}")))?;
        std::fs::write(key_path, key_der)
            .map_err(|e| CertError::PersistenceFailed(format!("write key: {e}")))?;
        Ok(())
    }

    /// Compute SHA-256 fingerprint of DER-encoded certificate bytes.
    fn compute_fingerprint(cert_der: &[u8]) -> String {
        let hash = digest(&SHA256, cert_der);
        hash.as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")
    }
}


/// Trust-on-first-use certificate verifier that accepts any self-signed certificate.
/// In a P2P context, peers are identified by their certificate fingerprint rather
/// than a CA chain.
#[derive(Debug)]
struct TofuCertVerifier;

impl rustls::client::danger::ServerCertVerifier for TofuCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        // Accept any certificate — peers are identified by fingerprint, not CA trust.
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_and_load_round_trip() {
        let dir = std::env::temp_dir().join("cert_manager_test_roundtrip");
        let _ = std::fs::remove_dir_all(&dir);

        // First call generates
        let cm1 = CertManager::load_or_generate(&dir).expect("generate should succeed");
        assert!(!cm1.fingerprint().is_empty());
        assert!(cm1.cert_path.exists());
        assert!(cm1.key_path.exists());

        // Second call loads from disk
        let cm2 = CertManager::load_or_generate(&dir).expect("load should succeed");
        assert_eq!(cm1.fingerprint(), cm2.fingerprint());
        assert_eq!(cm1.cert_der, cm2.cert_der);
        assert_eq!(cm1.key_der, cm2.key_der);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_is_sha256_hex_with_colons() {
        let dir = std::env::temp_dir().join("cert_manager_test_fingerprint");
        let _ = std::fs::remove_dir_all(&dir);

        let cm = CertManager::load_or_generate(&dir).expect("should succeed");
        let fp = cm.fingerprint();

        // SHA-256 = 32 bytes = 64 hex chars + 31 colons = 95 chars
        assert_eq!(fp.len(), 95);
        let parts: Vec<&str> = fp.split(':').collect();
        assert_eq!(parts.len(), 32);
        for part in &parts {
            assert_eq!(part.len(), 2);
            assert!(part.chars().all(|c| c.is_ascii_hexdigit()));
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn server_tls_config_builds_successfully() {
        let dir = std::env::temp_dir().join("cert_manager_test_server_tls");
        let _ = std::fs::remove_dir_all(&dir);

        let cm = CertManager::load_or_generate(&dir).expect("should succeed");
        let config = cm.server_tls_config();
        assert!(config.is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn client_tls_config_builds_successfully() {
        let dir = std::env::temp_dir().join("cert_manager_test_client_tls");
        let _ = std::fs::remove_dir_all(&dir);

        let cm = CertManager::load_or_generate(&dir).expect("should succeed");
        let config = cm.client_tls_config();
        assert!(config.is_ok());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn different_data_dirs_produce_different_fingerprints() {
        let dir1 = std::env::temp_dir().join("cert_manager_test_diff1");
        let dir2 = std::env::temp_dir().join("cert_manager_test_diff2");
        let _ = std::fs::remove_dir_all(&dir1);
        let _ = std::fs::remove_dir_all(&dir2);

        let cm1 = CertManager::load_or_generate(&dir1).expect("should succeed");
        let cm2 = CertManager::load_or_generate(&dir2).expect("should succeed");

        // Different key pairs should produce different fingerprints
        assert_ne!(cm1.fingerprint(), cm2.fingerprint());

        let _ = std::fs::remove_dir_all(&dir1);
        let _ = std::fs::remove_dir_all(&dir2);
    }

    #[test]
    fn compute_fingerprint_is_deterministic() {
        let data = b"test certificate data";
        let fp1 = CertManager::compute_fingerprint(data);
        let fp2 = CertManager::compute_fingerprint(data);
        assert_eq!(fp1, fp2);
    }

    // Feature: p2p-tauri-desktop, Property 26: Certificate Persistence Round-Trip
    // **Validates: Requirements 2.4**
    mod prop_cert_persistence {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn cert_persistence_round_trip(suffix in "[a-z0-9]{4,12}") {
                let dir = std::env::temp_dir()
                    .join(format!("cert_prop26_{}", suffix));
                let _ = std::fs::remove_dir_all(&dir);

                // Generate cert and record fingerprint
                let cm1 = CertManager::load_or_generate(&dir)
                    .expect("generate should succeed");
                let fp1 = cm1.fingerprint().to_owned();

                // Reload from same directory and check fingerprint matches
                let cm2 = CertManager::load_or_generate(&dir)
                    .expect("reload should succeed");
                let fp2 = cm2.fingerprint().to_owned();

                prop_assert_eq!(fp1, fp2, "fingerprint must survive persist-reload round-trip");

                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    }
}
