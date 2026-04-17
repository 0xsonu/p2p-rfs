use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

use chrono::Utc;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::peer_registry::{PeerInfo, PeerRegistry, PeerStatus};

/// mDNS/DNS-SD service type used for peer discovery on the LAN.
const SERVICE_TYPE: &str = "_fileshare._udp.local.";

/// TXT record key for the advertised display name.
const TXT_DISPLAY_NAME: &str = "display_name";

/// TXT record key for the certificate fingerprint.
const TXT_CERT_FINGERPRINT: &str = "cert_fingerprint";

/// TXT record key for the QUIC listen port.
const TXT_PORT: &str = "port";

/// Errors that can occur during mDNS discovery operations.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("mDNS service registration failed: {0}")]
    RegistrationFailed(String),

    #[error("mDNS browse failed: {0}")]
    BrowseFailed(String),
}

/// Information about the local peer that is advertised via mDNS.
#[derive(Clone, Debug)]
pub struct LocalPeerInfo {
    pub display_name: String,
    pub listen_port: u16,
    pub cert_fingerprint: String,
}

/// mDNS-based peer discovery service.
///
/// Advertises the local Tauri instance on the LAN and continuously browses
/// for other peers. Discovered peers are added to the shared `PeerRegistry`;
/// removed peers are evicted. A background task periodically calls
/// `PeerRegistry::evict_stale()` every 10 seconds.
pub struct DiscoveryService {
    daemon: ServiceDaemon,
    _peer_registry: Arc<PeerRegistry>,
    local_info: Arc<RwLock<LocalPeerInfo>>,
    /// The full mDNS service name used for the local registration.
    fullname: RwLock<String>,
}

impl DiscoveryService {
    /// Start advertising the local instance and browsing for peers.
    ///
    /// Registers a DNS-SD service record with TXT records containing
    /// `display_name`, `cert_fingerprint`, and `port`. Spawns background
    /// tasks for browse event processing and stale peer eviction.
    pub fn start(
        peer_registry: Arc<PeerRegistry>,
        display_name: &str,
        listen_port: u16,
        cert_fingerprint: &str,
    ) -> Result<Self, DiscoveryError> {
        let daemon =
            ServiceDaemon::new().map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

        let local_info = Arc::new(RwLock::new(LocalPeerInfo {
            display_name: display_name.to_string(),
            listen_port,
            cert_fingerprint: cert_fingerprint.to_string(),
        }));

        // Build the mDNS service info for advertising.
        let instance_name = format!(
            "fileshare-{}",
            &cert_fingerprint[..8.min(cert_fingerprint.len())]
        );
        let properties = [
            (TXT_DISPLAY_NAME, display_name),
            (TXT_CERT_FINGERPRINT, cert_fingerprint),
            (TXT_PORT, &listen_port.to_string()),
        ];

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &hostname_local(),
            "",
            listen_port,
            &properties[..],
        )
        .map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

        let fullname = service_info.get_fullname().to_string();

        // Register (advertise) the local service.
        daemon
            .register(service_info)
            .map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

        info!(
            display_name = display_name,
            port = listen_port,
            "mDNS service registered"
        );

        // Start browsing for peers.
        let browse_receiver = daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| DiscoveryError::BrowseFailed(e.to_string()))?;

        // Spawn browse event processing task.
        let registry_for_browse = Arc::clone(&peer_registry);
        let own_fingerprint = cert_fingerprint.to_string();
        std::thread::spawn(move || {
            Self::browse_loop(browse_receiver, registry_for_browse, own_fingerprint);
        });

        // Spawn stale peer eviction task (every 10 seconds).
        let registry_for_eviction = Arc::clone(&peer_registry);
        std::thread::spawn(move || {
            Self::eviction_loop(registry_for_eviction);
        });

        Ok(Self {
            daemon,
            _peer_registry: peer_registry,
            local_info,
            fullname: RwLock::new(fullname),
        })
    }

    /// Stop advertising and browsing. Called on shutdown.
    pub fn stop(&self) -> Result<(), DiscoveryError> {
        let fullname = self.fullname.read().unwrap();
        if let Err(e) = self.daemon.unregister(&fullname) {
            warn!(error = %e, "Failed to unregister mDNS service");
        }

        if let Err(e) = self.daemon.stop_browse(SERVICE_TYPE) {
            warn!(error = %e, "Failed to stop mDNS browse");
        }

        if let Err(e) = self.daemon.shutdown() {
            warn!(error = %e, "Failed to shut down mDNS daemon");
        }

        info!("mDNS discovery stopped");
        Ok(())
    }

    /// Update the advertised display name (e.g., after settings change).
    ///
    /// Unregisters the old service and re-registers with the new name.
    pub fn update_display_name(&self, name: &str) -> Result<(), DiscoveryError> {
        let mut local = self.local_info.write().unwrap();
        let mut fullname = self.fullname.write().unwrap();

        // Unregister old service.
        if let Err(e) = self.daemon.unregister(&fullname) {
            warn!(error = %e, "Failed to unregister old mDNS service during name update");
        }

        local.display_name = name.to_string();

        // Re-register with updated name.
        let instance_name = format!(
            "fileshare-{}",
            &local.cert_fingerprint[..8.min(local.cert_fingerprint.len())]
        );
        let properties = [
            (TXT_DISPLAY_NAME, name),
            (TXT_CERT_FINGERPRINT, local.cert_fingerprint.as_str()),
            (TXT_PORT, &local.listen_port.to_string()),
        ];

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &hostname_local(),
            "",
            local.listen_port,
            &properties[..],
        )
        .map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

        *fullname = service_info.get_fullname().to_string();

        self.daemon
            .register(service_info)
            .map_err(|e| DiscoveryError::RegistrationFailed(e.to_string()))?;

        info!(display_name = name, "mDNS display name updated");
        Ok(())
    }

    /// Get the local peer info for display in the UI.
    pub fn local_info(&self) -> LocalPeerInfo {
        self.local_info.read().unwrap().clone()
    }

    /// Background loop that processes mDNS browse events and updates the
    /// `PeerRegistry` on `ServiceResolved` / `ServiceRemoved`.
    fn browse_loop(
        receiver: flume::Receiver<ServiceEvent>,
        peer_registry: Arc<PeerRegistry>,
        own_fingerprint: String,
    ) {
        while let Ok(event) = receiver.recv() {
            match event {
                ServiceEvent::ServiceResolved(info) => {
                    let fingerprint = info
                        .get_properties()
                        .get_property_val_str(TXT_CERT_FINGERPRINT)
                        .unwrap_or_default()
                        .to_string();

                    // Skip our own advertisement.
                    if fingerprint == own_fingerprint {
                        continue;
                    }

                    let display_name = info
                        .get_properties()
                        .get_property_val_str(TXT_DISPLAY_NAME)
                        .unwrap_or_default()
                        .to_string();

                    let port_str = info
                        .get_properties()
                        .get_property_val_str(TXT_PORT)
                        .unwrap_or_default()
                        .to_string();
                    let port: u16 = port_str.parse().unwrap_or(info.get_port());

                    let addresses: Vec<SocketAddr> = info
                        .get_addresses()
                        .iter()
                        .map(|ip| SocketAddr::new(IpAddr::from(*ip), port))
                        .collect();

                    if addresses.is_empty() {
                        debug!(
                            fullname = info.get_fullname(),
                            "Resolved peer has no addresses, skipping"
                        );
                        continue;
                    }

                    let now = Utc::now();
                    let peer_id = if fingerprint.is_empty() {
                        info.get_fullname().to_string()
                    } else {
                        fingerprint.clone()
                    };

                    let peer_info = PeerInfo {
                        id: peer_id.clone(),
                        display_name,
                        addresses,
                        cert_fingerprint: fingerprint,
                        status: PeerStatus::Discovered,
                        discovered_at: now,
                        last_seen: now,
                    };

                    info!(
                        peer_id = %peer_id,
                        name = %peer_info.display_name,
                        "Peer discovered via mDNS"
                    );

                    peer_registry.upsert(peer_info);
                }
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    // Try to find the peer by matching the fullname or fingerprint.
                    // The fullname from ServiceRemoved may not carry TXT records,
                    // so we search the registry for a matching entry.
                    let peers = peer_registry.list();
                    for peer in &peers {
                        // If the fullname contains the peer's fingerprint prefix,
                        // or if the peer id matches the fullname, remove it.
                        if fullname
                            .contains(&peer.cert_fingerprint[..8.min(peer.cert_fingerprint.len())])
                            || peer.id == fullname
                        {
                            info!(
                                peer_id = %peer.id,
                                "Peer removed via mDNS"
                            );
                            peer_registry.remove(&peer.id);
                            break;
                        }
                    }
                }
                ServiceEvent::SearchStarted(_) => {
                    debug!("mDNS browse started");
                }
                ServiceEvent::SearchStopped(_) => {
                    debug!("mDNS browse stopped");
                    break;
                }
                _ => {}
            }
        }
    }

    /// Background loop that calls `evict_stale()` every 10 seconds.
    fn eviction_loop(peer_registry: Arc<PeerRegistry>) {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(10));
            let evicted = peer_registry.evict_stale();
            if !evicted.is_empty() {
                debug!(count = evicted.len(), "Evicted stale peers");
            }
        }
    }
}

/// Get the local hostname as a valid mDNS host string ending with `.local.`.
///
/// On macOS, `hostname::get()` often returns something like
/// `Sonus-MacBook-Pro.local` (without a trailing dot), while on Windows it
/// returns a plain NetBIOS name like `DESKTOP-ABC123`. The `mdns_sd` crate
/// requires the hostname to end with `.local.`, so we normalise here to
/// work correctly on both platforms.
fn hostname_local() -> String {
    let raw = hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "localhost".to_string());

    // Strip any existing trailing dot(s) first, then normalise.
    let trimmed = raw.trim_end_matches('.');

    if trimmed.ends_with(".local") {
        // Already has the `.local` suffix – just append the trailing dot.
        format!("{}.", trimmed)
    } else {
        // Plain hostname (common on Windows/Linux) – append `.local.`
        format!("{}.local.", trimmed)
    }
}
