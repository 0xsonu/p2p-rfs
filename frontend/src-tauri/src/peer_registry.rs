use std::net::SocketAddr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Unique peer identifier — certificate fingerprint or UUID.
pub type PeerId = String;

/// Status of a discovered peer.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PeerStatus {
    Discovered,
    Connected,
    Unreachable,
}

/// Information about a discovered peer on the local network.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub id: PeerId,
    pub display_name: String,
    pub addresses: Vec<SocketAddr>,
    pub cert_fingerprint: String,
    pub status: PeerStatus,
    pub discovered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

/// Thread-safe in-memory store of discovered peers.
///
/// Uses `DashMap` for lock-free concurrent reads and writes from multiple
/// async tasks (mDNS browse callbacks, Tauri command handlers, eviction timer).
pub struct PeerRegistry {
    peers: DashMap<PeerId, PeerInfo>,
    stale_timeout: Duration,
}

impl PeerRegistry {
    /// Create a new registry with the given stale timeout.
    ///
    /// Peers whose `last_seen` is older than `stale_timeout` will be removed
    /// when `evict_stale()` is called. Per Requirement 3.4 the default is 30 seconds.
    pub fn new(stale_timeout: Duration) -> Self {
        Self {
            peers: DashMap::new(),
            stale_timeout,
        }
    }

    /// Add or update a peer from mDNS discovery.
    ///
    /// If the peer already exists, its fields are replaced with the new values.
    pub fn upsert(&self, peer: PeerInfo) {
        self.peers.insert(peer.id.clone(), peer);
    }

    /// Remove a peer that is no longer reachable.
    pub fn remove(&self, peer_id: &PeerId) {
        self.peers.remove(peer_id);
    }

    /// List all currently known peers.
    pub fn list(&self) -> Vec<PeerInfo> {
        self.peers.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Get a specific peer by ID.
    pub fn get(&self, peer_id: &PeerId) -> Option<PeerInfo> {
        self.peers.get(peer_id).map(|entry| entry.value().clone())
    }

    /// Update the status of a peer. No-op if the peer is not in the registry.
    pub fn set_status(&self, peer_id: &PeerId, status: PeerStatus) {
        if let Some(mut entry) = self.peers.get_mut(peer_id) {
            entry.status = status;
        }
    }

    /// Remove peers not seen within `stale_timeout`. Returns the IDs of evicted peers.
    ///
    /// Called periodically by a background task (every 10 seconds per the design).
    pub fn evict_stale(&self) -> Vec<PeerId> {
        let cutoff = Utc::now() - self.stale_timeout;
        let stale_ids: Vec<PeerId> = self
            .peers
            .iter()
            .filter(|entry| entry.value().last_seen < cutoff)
            .map(|entry| entry.key().clone())
            .collect();

        for id in &stale_ids {
            self.peers.remove(id);
        }

        stale_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};
    use proptest::prelude::*;

    /// Helper to create a PeerInfo with sensible defaults.
    fn make_peer(id: &str, last_seen: DateTime<Utc>) -> PeerInfo {
        PeerInfo {
            id: id.to_string(),
            display_name: format!("Peer {}", id),
            addresses: vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)),
                4433,
            )],
            cert_fingerprint: format!("fp-{}", id),
            status: PeerStatus::Discovered,
            discovered_at: last_seen,
            last_seen,
        }
    }

    #[test]
    fn upsert_and_get_returns_peer() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        let peer = make_peer("a", Utc::now());
        registry.upsert(peer.clone());

        let retrieved = registry.get(&"a".to_string());
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().display_name, "Peer a");
    }

    #[test]
    fn upsert_overwrites_existing_peer() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        let mut peer = make_peer("a", Utc::now());
        registry.upsert(peer.clone());

        peer.display_name = "Updated".to_string();
        registry.upsert(peer);

        let retrieved = registry.get(&"a".to_string()).unwrap();
        assert_eq!(retrieved.display_name, "Updated");
    }

    #[test]
    fn remove_deletes_peer() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        registry.upsert(make_peer("a", Utc::now()));
        registry.remove(&"a".to_string());
        assert!(registry.get(&"a".to_string()).is_none());
    }

    #[test]
    fn remove_nonexistent_is_noop() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        registry.remove(&"nonexistent".to_string()); // should not panic
    }

    #[test]
    fn list_returns_all_peers() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        registry.upsert(make_peer("a", Utc::now()));
        registry.upsert(make_peer("b", Utc::now()));
        registry.upsert(make_peer("c", Utc::now()));

        let peers = registry.list();
        assert_eq!(peers.len(), 3);
    }

    #[test]
    fn list_empty_registry() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        assert!(registry.list().is_empty());
    }

    #[test]
    fn get_nonexistent_returns_none() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        assert!(registry.get(&"missing".to_string()).is_none());
    }

    #[test]
    fn set_status_updates_peer() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        registry.upsert(make_peer("a", Utc::now()));
        registry.set_status(&"a".to_string(), PeerStatus::Connected);

        let peer = registry.get(&"a".to_string()).unwrap();
        assert_eq!(peer.status, PeerStatus::Connected);
    }

    #[test]
    fn set_status_nonexistent_is_noop() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        registry.set_status(&"missing".to_string(), PeerStatus::Connected); // should not panic
    }

    #[test]
    fn evict_stale_removes_old_peers() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        let old_time = Utc::now() - chrono::Duration::seconds(60);
        let fresh_time = Utc::now();

        registry.upsert(make_peer("old", old_time));
        registry.upsert(make_peer("fresh", fresh_time));

        let evicted = registry.evict_stale();
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0], "old");
        assert!(registry.get(&"old".to_string()).is_none());
        assert!(registry.get(&"fresh".to_string()).is_some());
    }

    #[test]
    fn evict_stale_returns_empty_when_all_fresh() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        registry.upsert(make_peer("a", Utc::now()));
        registry.upsert(make_peer("b", Utc::now()));

        let evicted = registry.evict_stale();
        assert!(evicted.is_empty());
        assert_eq!(registry.list().len(), 2);
    }

    #[test]
    fn evict_stale_on_empty_registry() {
        let registry = PeerRegistry::new(Duration::from_secs(30));
        let evicted = registry.evict_stale();
        assert!(evicted.is_empty());
    }

    // Feature: p2p-tauri-desktop, Property 17: Peer Registry Round-Trip
    // **Validates: Requirements 3.3, 14.1**

    fn arb_peer_status() -> impl Strategy<Value = PeerStatus> {
        prop_oneof![
            Just(PeerStatus::Discovered),
            Just(PeerStatus::Connected),
            Just(PeerStatus::Unreachable),
        ]
    }

    fn arb_socket_addr() -> impl Strategy<Value = SocketAddr> {
        (any::<[u8; 4]>(), 1u16..=65535u16).prop_map(|(octets, port)| {
            SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(octets[0], octets[1], octets[2], octets[3])),
                port,
            )
        })
    }

    fn arb_peer_info() -> impl Strategy<Value = PeerInfo> {
        (
            "[a-zA-Z0-9]{1,32}",           // id
            "[ -~]{0,64}",                  // display_name
            prop::collection::vec(arb_socket_addr(), 1..5), // addresses (at least 1)
            "[a-f0-9]{64}",                 // cert_fingerprint (SHA-256 hex)
            arb_peer_status(),
        )
            .prop_map(|(id, display_name, addresses, cert_fingerprint, status)| {
                let now = Utc::now();
                PeerInfo {
                    id,
                    display_name,
                    addresses,
                    cert_fingerprint,
                    status,
                    discovered_at: now,
                    last_seen: now,
                }
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_peer_registry_round_trip(peer in arb_peer_info()) {
            let registry = PeerRegistry::new(Duration::from_secs(30));
            registry.upsert(peer.clone());

            let retrieved = registry.get(&peer.id);
            prop_assert!(retrieved.is_some(), "Peer should be retrievable after upsert");

            let retrieved = retrieved.unwrap();
            prop_assert_eq!(&retrieved.display_name, &peer.display_name,
                "display_name mismatch");
            prop_assert_eq!(&retrieved.addresses, &peer.addresses,
                "addresses mismatch");
            prop_assert_eq!(&retrieved.cert_fingerprint, &peer.cert_fingerprint,
                "cert_fingerprint mismatch");
            prop_assert_eq!(&retrieved.status, &peer.status,
                "status mismatch");
        }
    }

    // Feature: p2p-tauri-desktop, Property 18: Stale Peer Eviction
    // **Validates: Requirements 3.4**

    /// Generate a peer with a specific last_seen offset (in seconds before now).
    fn make_peer_with_age(id: &str, age_secs: i64) -> PeerInfo {
        let last_seen = Utc::now() - chrono::Duration::seconds(age_secs);
        make_peer(id, last_seen)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_stale_peer_eviction(
            stale_timeout_secs in 5u64..120u64,
            // Generate pairs of (unique_suffix, age_in_seconds).
            // Ages range from 0 to 240s so we get a mix of stale and fresh peers.
            peer_ages in prop::collection::vec((0u32..10000u32, 0u64..240u64), 1..30)
        ) {
            let stale_timeout = Duration::from_secs(stale_timeout_secs);
            let registry = PeerRegistry::new(stale_timeout);

            // Deduplicate peer IDs by using the index as part of the ID
            let mut expected_stale_ids: Vec<String> = Vec::new();
            let mut expected_fresh_ids: Vec<String> = Vec::new();
            // Peers near the boundary (within 2s of stale_timeout) are ambiguous
            // due to clock drift between peer creation and evict_stale() call.
            let mut ambiguous_ids: Vec<String> = Vec::new();

            for (i, &(suffix, age_secs)) in peer_ages.iter().enumerate() {
                let id = format!("peer-{}-{}", i, suffix);
                let peer = make_peer_with_age(&id, age_secs as i64);
                registry.upsert(peer);

                let diff = (age_secs as i64) - (stale_timeout_secs as i64);
                if diff.abs() <= 1 {
                    // Within 1 second of the boundary — ambiguous due to timing
                    ambiguous_ids.push(id);
                } else if age_secs > stale_timeout_secs {
                    expected_stale_ids.push(id);
                } else {
                    expected_fresh_ids.push(id);
                }
            }

            let total_before = registry.list().len();
            prop_assert_eq!(total_before, peer_ages.len(),
                "All peers should be in registry before eviction");

            let evicted = registry.evict_stale();
            let evicted_set: std::collections::HashSet<&String> = evicted.iter().collect();

            // All clearly stale peers must be evicted
            for id in &expected_stale_ids {
                prop_assert!(evicted_set.contains(id),
                    "Stale peer {} should have been evicted", id);
                prop_assert!(registry.get(id).is_none(),
                    "Stale peer {} should have been removed from registry", id);
            }

            // All clearly fresh peers must remain
            for id in &expected_fresh_ids {
                prop_assert!(!evicted_set.contains(id),
                    "Fresh peer {} should NOT have been evicted", id);
                prop_assert!(registry.get(id).is_some(),
                    "Fresh peer {} should still be present in registry", id);
            }

            // Ambiguous peers may or may not be evicted — just verify consistency:
            // if evicted, they should be gone; if not evicted, they should be present.
            for id in &ambiguous_ids {
                if evicted_set.contains(id) {
                    prop_assert!(registry.get(id).is_none(),
                        "Ambiguous peer {} was evicted but still in registry", id);
                } else {
                    prop_assert!(registry.get(id).is_some(),
                        "Ambiguous peer {} was not evicted but missing from registry", id);
                }
            }

            // Total evicted should be stale + some subset of ambiguous
            let evicted_ambiguous_count = ambiguous_ids.iter()
                .filter(|id| evicted_set.contains(id))
                .count();
            prop_assert_eq!(evicted.len(), expected_stale_ids.len() + evicted_ambiguous_count,
                "Evicted count should be stale peers + evicted ambiguous peers");

            // Registry size should be fresh + non-evicted ambiguous
            let remaining_ambiguous = ambiguous_ids.len() - evicted_ambiguous_count;
            prop_assert_eq!(registry.list().len(), expected_fresh_ids.len() + remaining_ambiguous,
                "Registry should contain fresh peers + non-evicted ambiguous peers");
        }
    }
}
