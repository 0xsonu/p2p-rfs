use std::sync::Arc;
use tokio::sync::RwLock;

use crate::p2p_engine::P2PEngine;
use crate::settings::P2PSettings;

/// Shared application state registered as Tauri managed state.
///
/// - `engine`: The P2P engine instance, wrapped in `Arc<RwLock<Option<...>>>` so it
///   can be lazily initialized after app startup and safely shared across async
///   Tauri command handlers.
/// - `settings`: Current application settings, wrapped in `Arc` for cheap cloning
///   across command handlers.
pub struct AppState {
    pub engine: Arc<RwLock<Option<P2PEngine>>>,
    pub settings: Arc<RwLock<P2PSettings>>,
}

impl AppState {
    pub fn new(settings: P2PSettings) -> Self {
        Self {
            engine: Arc::new(RwLock::new(None)),
            settings: Arc::new(RwLock::new(settings)),
        }
    }
}
