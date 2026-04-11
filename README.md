# P2P File Sharing Desktop App

A peer-to-peer file sharing application built with Tauri, React, and Rust. Each instance acts as both sender and receiver — no server required. Peers discover each other automatically via mDNS on the local network and transfer files over direct QUIC connections with TLS 1.3 encryption.

## Architecture

```
┌──────────────────────────────────────────────────────┐
│  Tauri App (Single Native Process)                   │
│                                                      │
│  ┌────────────┐     ┌─────────────────────────────┐  │
│  │  React UI  │────→│  Rust P2P Engine             │  │
│  │  (WebView) │     │  ┌───────────┐ ┌──────────┐ │  │
│  │            │     │  │ Discovery │ │ Transfer │ │  │
│  │  Peers     │     │  │  (mDNS)   │ │ Manager  │ │  │
│  │  Send      │     │  ├───────────┤ ├──────────┤ │  │
│  │  Receive   │     │  │ QUIC/TLS  │ │Integrity │ │  │
│  │  History   │     │  │ Listener  │ │ Verifier │ │  │
│  │  Settings  │     │  ├───────────┤ ├──────────┤ │  │
│  │            │     │  │   Cert    │ │ Storage  │ │  │
│  │            │     │  │  Manager  │ │ Engine   │ │  │
│  └────────────┘     │  └───────────┘ └──────────┘ │  │
│    Tauri IPC        └─────────────────────────────┘  │
│  (invoke/listen)           ↕ QUIC + TLS 1.3          │
└──────────────────────────────────────────────────────┘
                             ↕
                    Other Tauri App instances
                    on the local network
```

## Features

- **Zero-config peer discovery** — mDNS/DNS-SD automatically finds peers on the LAN
- **Direct P2P transfers** — no relay server, files go straight between devices
- **Encrypted connections** — QUIC with TLS 1.3 using self-signed certificates (TOFU model)
- **Chunked parallel transfers** — large files split into chunks sent across multiple QUIC streams
- **Resumable transfers** — interrupted transfers resume from the last completed chunk
- **Integrity verification** — per-chunk SHA-256 hashing plus whole-file hash verification
- **Rate limiting** — per-session and global rate limits with backpressure
- **Transfer history** — chronological log of all completed and failed transfers
- **Cross-platform** — macOS, Windows, and Linux via Tauri

## Project Structure

```
├── frontend/                # Tauri desktop app
│   ├── src/                 # React UI (TypeScript + Tailwind)
│   │   ├── screens/         # Peers, Send, Receive, History, Settings
│   │   ├── components/      # TransferProgress bar
│   │   ├── hooks/           # useP2PEngine, useTransfer
│   │   └── services/        # p2pBridge (Tauri IPC), tauriBridge (file dialogs)
│   └── src-tauri/           # Rust backend embedded in Tauri
│       └── src/
│           ├── p2p_engine.rs    # QUIC listener, connections, file send/receive
│           ├── discovery.rs     # mDNS advertise + browse
│           ├── peer_registry.rs # Thread-safe peer tracking (DashMap)
│           ├── cert_manager.rs  # Self-signed TLS cert generation + persistence
│           ├── commands.rs      # Tauri command handlers
│           ├── events.rs        # Tauri event emission helpers
│           ├── settings.rs      # P2P settings validation + JSON persistence
│           ├── state.rs         # Shared AppState
│           └── lib.rs           # App entry point, command registration
├── server/                  # Reusable Rust library crates
│   ├── protocol/            # Protobuf codec (prost) with versioned envelopes
│   ├── transfer/            # Transfer sessions, chunk layout, rate control
│   ├── integrity/           # SHA-256 chunk/file hashing and verification
│   ├── storage/             # Direct-offset file I/O with concurrency control
│   ├── rate_control/        # Token bucket rate limiting + backpressure
│   └── observability/       # Structured JSON logging + metrics
├── proto/
│   └── messages.proto       # Protobuf schema (includes P2P message types)
└── README.md
```

## Prerequisites

- **Rust** 1.75+ with `cargo`
- **Node.js** 20+ with `npm`
- **Tauri CLI** — installed via `npm` (included in devDependencies)

## Quick Start

```bash
cd frontend
npm install
npm run tauri dev
```

This compiles the Rust backend, starts the Vite dev server, and opens the desktop app. The P2P engine starts automatically — you'll see the Peers screen with your device info and any discovered peers on the network.

## Building for Production

```bash
cd frontend
npm run tauri build
```

Produces a native installer in `frontend/src-tauri/target/release/bundle/`.

## Testing

### Rust tests (91 tests — unit, property, integration)

```bash
cd frontend/src-tauri && cargo test
cd server && cargo test --workspace
```

### Frontend tests (12 property tests via fast-check)

```bash
cd frontend && npm run test
```

Property-based tests cover 26 correctness properties including protocol round-trips, chunk layout coverage, hash verification, session completion, rate limiting, backpressure hysteresis, and settings validation.

## How It Works

1. On launch, the app generates a self-signed TLS certificate (persisted for reuse)
2. A QUIC listener binds on port 4433 (with automatic fallback if busy)
3. mDNS advertises the instance as `_fileshare._udp.local.` with display name and cert fingerprint
4. Discovered peers appear in the Peers screen — connect manually or via mDNS
5. To send: pick a file, select a peer, the app chunks the file and streams it over QUIC
6. To receive: accept the incoming request, choose a save location, chunks are verified and written to disk
7. Whole-file hash is verified after all chunks are received

## License

Private — all rights reserved.
