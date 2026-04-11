use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// User-facing settings persisted as JSON in the Tauri app data directory.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct P2PSettings {
    pub display_name: String,
    pub listen_port: u16,
    pub chunk_size: usize,
    pub parallel_streams: usize,
    pub per_transfer_rate_limit: u64, // bytes/sec, 0 = unlimited
    pub download_dir: PathBuf,
}

impl Default for P2PSettings {
    fn default() -> Self {
        Self {
            display_name: "My Device".into(),
            listen_port: 4433,
            chunk_size: 256 * 1024, // 256 KiB
            parallel_streams: 4,
            per_transfer_rate_limit: 0, // unlimited
            download_dir: PathBuf::from("."),
        }
    }
}

/// Runtime configuration extending P2PSettings with additional engine fields.
#[derive(Clone, Debug)]
pub struct P2PConfig {
    pub display_name: String,
    pub listen_port: u16,
    pub chunk_size: usize,
    pub parallel_streams: usize,
    pub per_transfer_rate_limit: u64,
    pub global_rate_limit: u64,
    pub download_dir: PathBuf,
    pub data_dir: PathBuf,
    pub idle_connection_timeout: Duration,
    pub max_retries: u32,
}

impl P2PConfig {
    /// Build a P2PConfig from user settings plus runtime defaults.
    pub fn from_settings(settings: &P2PSettings, data_dir: PathBuf) -> Self {
        Self {
            display_name: settings.display_name.clone(),
            listen_port: settings.listen_port,
            chunk_size: settings.chunk_size,
            parallel_streams: settings.parallel_streams,
            per_transfer_rate_limit: settings.per_transfer_rate_limit,
            global_rate_limit: 0,
            download_dir: settings.download_dir.clone(),
            data_dir,
            idle_connection_timeout: Duration::from_secs(60),
            max_retries: 3,
        }
    }
}

/// A single validation error for a settings field.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ValidationError {
    pub field: String,
    pub reason: String,
}

/// Validate P2PSettings, returning one error per invalid field.
pub fn validate_p2p_settings(settings: &P2PSettings) -> Vec<ValidationError> {
    let mut errors = vec![];
    if settings.display_name.trim().is_empty() {
        errors.push(ValidationError {
            field: "display_name".into(),
            reason: "must not be empty".into(),
        });
    }
    if settings.listen_port == 0 {
        errors.push(ValidationError {
            field: "listen_port".into(),
            reason: "must be 1-65535".into(),
        });
    }
    if settings.chunk_size == 0 {
        errors.push(ValidationError {
            field: "chunk_size".into(),
            reason: "must be a positive integer".into(),
        });
    }
    if settings.parallel_streams == 0 {
        errors.push(ValidationError {
            field: "parallel_streams".into(),
            reason: "must be a positive integer".into(),
        });
    }
    errors
}

const SETTINGS_FILE: &str = "settings.json";

/// Load settings from the app data directory. Returns defaults if the file
/// does not exist or cannot be parsed.
pub fn load_settings(data_dir: &Path) -> P2PSettings {
    let path = data_dir.join(SETTINGS_FILE);
    match std::fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => P2PSettings::default(),
    }
}

/// Save settings to the app data directory as JSON. Creates the directory if
/// it does not exist.
pub fn save_settings(data_dir: &Path, settings: &P2PSettings) -> Result<(), SettingsError> {
    std::fs::create_dir_all(data_dir).map_err(|e| SettingsError::IoError(e.to_string()))?;
    let path = data_dir.join(SETTINGS_FILE);
    let json =
        serde_json::to_string_pretty(settings).map_err(|e| SettingsError::IoError(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| SettingsError::IoError(e.to_string()))?;
    Ok(())
}

/// Errors that can occur during settings persistence.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("validation failed")]
    ValidationFailed { errors: Vec<ValidationError> },
    #[error("I/O error: {0}")]
    IoError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use proptest::prelude::*;

    #[test]
    fn default_settings_are_valid() {
        let settings = P2PSettings::default();
        let errors = validate_p2p_settings(&settings);
        assert!(errors.is_empty());
    }

    #[test]
    fn empty_display_name_is_invalid() {
        let settings = P2PSettings {
            display_name: "   ".into(),
            ..Default::default()
        };
        let errors = validate_p2p_settings(&settings);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "display_name");
    }

    #[test]
    fn zero_port_is_invalid() {
        let settings = P2PSettings {
            listen_port: 0,
            ..Default::default()
        };
        let errors = validate_p2p_settings(&settings);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "listen_port");
    }

    #[test]
    fn zero_chunk_size_is_invalid() {
        let settings = P2PSettings {
            chunk_size: 0,
            ..Default::default()
        };
        let errors = validate_p2p_settings(&settings);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "chunk_size");
    }

    #[test]
    fn zero_parallel_streams_is_invalid() {
        let settings = P2PSettings {
            parallel_streams: 0,
            ..Default::default()
        };
        let errors = validate_p2p_settings(&settings);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].field, "parallel_streams");
    }

    #[test]
    fn multiple_invalid_fields_produce_multiple_errors() {
        let settings = P2PSettings {
            display_name: "".into(),
            listen_port: 0,
            chunk_size: 0,
            parallel_streams: 0,
            per_transfer_rate_limit: 0,
            download_dir: PathBuf::from("/tmp"),
        };
        let errors = validate_p2p_settings(&settings);
        assert_eq!(errors.len(), 4);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("p2p_settings_test");
        let _ = std::fs::remove_dir_all(&dir);

        let settings = P2PSettings {
            display_name: "Test Device".into(),
            listen_port: 9999,
            chunk_size: 512 * 1024,
            parallel_streams: 8,
            per_transfer_rate_limit: 1_000_000,
            download_dir: PathBuf::from("/downloads"),
        };

        save_settings(&dir, &settings).expect("save should succeed");
        let loaded = load_settings(&dir);

        assert_eq!(loaded.display_name, settings.display_name);
        assert_eq!(loaded.listen_port, settings.listen_port);
        assert_eq!(loaded.chunk_size, settings.chunk_size);
        assert_eq!(loaded.parallel_streams, settings.parallel_streams);
        assert_eq!(loaded.per_transfer_rate_limit, settings.per_transfer_rate_limit);
        assert_eq!(loaded.download_dir, settings.download_dir);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_returns_defaults_when_file_missing() {
        let dir = std::env::temp_dir().join("p2p_settings_missing");
        let _ = std::fs::remove_dir_all(&dir);
        let loaded = load_settings(&dir);
        let defaults = P2PSettings::default();
        assert_eq!(loaded.display_name, defaults.display_name);
        assert_eq!(loaded.listen_port, defaults.listen_port);
    }

    #[test]
    fn p2p_config_from_settings() {
        let settings = P2PSettings::default();
        let config = P2PConfig::from_settings(&settings, PathBuf::from("/data"));
        assert_eq!(config.display_name, settings.display_name);
        assert_eq!(config.listen_port, settings.listen_port);
        assert_eq!(config.data_dir, PathBuf::from("/data"));
        assert_eq!(config.max_retries, 3);
    }

    // Feature: p2p-tauri-desktop, Property 19: Settings Validation
    // **Validates: Requirements 17.2, 17.3**
    //
    // For any P2PSettings, validate returns empty errors iff display_name
    // non-empty, port 1-65535, chunk_size > 0, parallel_streams > 0.
    fn arb_p2p_settings() -> impl Strategy<Value = P2PSettings> {
        (
            // display_name: mix of empty/whitespace-only and non-empty strings
            prop_oneof![
                Just(String::new()),
                Just("   ".to_string()),
                "[a-zA-Z0-9 _-]{1,50}".prop_map(|s| s),
            ],
            // listen_port: full u16 range (0 is the only invalid value)
            any::<u16>(),
            // chunk_size: include 0 as invalid case
            prop_oneof![Just(0usize), 1..=10_000_000usize],
            // parallel_streams: include 0 as invalid case
            prop_oneof![Just(0usize), 1..=128usize],
            // per_transfer_rate_limit: any valid value (not validated)
            any::<u64>(),
        )
            .prop_map(|(display_name, listen_port, chunk_size, parallel_streams, rate_limit)| {
                P2PSettings {
                    display_name,
                    listen_port,
                    chunk_size,
                    parallel_streams,
                    per_transfer_rate_limit: rate_limit,
                    download_dir: PathBuf::from("/tmp"),
                }
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_validate_empty_errors_iff_all_fields_valid(settings in arb_p2p_settings()) {
            let errors = validate_p2p_settings(&settings);

            let name_valid = !settings.display_name.trim().is_empty();
            let port_valid = settings.listen_port >= 1; // u16, so max is always 65535
            let chunk_valid = settings.chunk_size > 0;
            let streams_valid = settings.parallel_streams > 0;
            let all_valid = name_valid && port_valid && chunk_valid && streams_valid;

            // Empty errors iff all four conditions hold
            prop_assert_eq!(errors.is_empty(), all_valid,
                "Expected errors.is_empty() == {} for settings: \
                 display_name={:?}, port={}, chunk_size={}, parallel_streams={}",
                all_valid, settings.display_name, settings.listen_port,
                settings.chunk_size, settings.parallel_streams);
        }

        #[test]
        fn prop_each_invalid_field_produces_exactly_one_error(settings in arb_p2p_settings()) {
            let errors = validate_p2p_settings(&settings);
            let error_fields: Vec<&str> = errors.iter().map(|e| e.field.as_str()).collect();

            // display_name invalid => exactly one error for that field
            if settings.display_name.trim().is_empty() {
                prop_assert!(error_fields.iter().filter(|&&f| f == "display_name").count() == 1,
                    "Expected exactly one display_name error");
            } else {
                prop_assert!(!error_fields.contains(&"display_name"),
                    "Expected no display_name error for valid name");
            }

            // listen_port == 0 => exactly one error for that field
            if settings.listen_port == 0 {
                prop_assert!(error_fields.iter().filter(|&&f| f == "listen_port").count() == 1,
                    "Expected exactly one listen_port error");
            } else {
                prop_assert!(!error_fields.contains(&"listen_port"),
                    "Expected no listen_port error for valid port");
            }

            // chunk_size == 0 => exactly one error for that field
            if settings.chunk_size == 0 {
                prop_assert!(error_fields.iter().filter(|&&f| f == "chunk_size").count() == 1,
                    "Expected exactly one chunk_size error");
            } else {
                prop_assert!(!error_fields.contains(&"chunk_size"),
                    "Expected no chunk_size error for valid chunk_size");
            }

            // parallel_streams == 0 => exactly one error for that field
            if settings.parallel_streams == 0 {
                prop_assert!(error_fields.iter().filter(|&&f| f == "parallel_streams").count() == 1,
                    "Expected exactly one parallel_streams error");
            } else {
                prop_assert!(!error_fields.contains(&"parallel_streams"),
                    "Expected no parallel_streams error for valid parallel_streams");
            }

            // Total error count equals the number of invalid fields
            let expected_count =
                usize::from(settings.display_name.trim().is_empty())
                + usize::from(settings.listen_port == 0)
                + usize::from(settings.chunk_size == 0)
                + usize::from(settings.parallel_streams == 0);
            prop_assert_eq!(errors.len(), expected_count,
                "Expected {} errors, got {}", expected_count, errors.len());
        }
    }
}
