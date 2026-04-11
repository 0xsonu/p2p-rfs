# Contributing Guide

This document covers how to set up the development environment, add new features, modify existing code, and run tests for the P2P File Sharing desktop app.

## Prerequisites

| Tool                         | Version | Purpose                                  |
| ---------------------------- | ------- | ---------------------------------------- |
| Rust                         | 1.75+   | Backend (P2P engine, library crates)     |
| Node.js                      | 20+     | Frontend (React UI, Vite, Vitest)        |
| npm                          | 10+     | Package management                       |
| Protobuf compiler (`protoc`) | 3.x     | Only if modifying `proto/messages.proto` |

Tauri CLI is installed as an npm devDependency — no global install needed.

## Project Layout

```
├── frontend/
│   ├── src/                    # React UI (TypeScript)
│   │   ├── screens/            # Page components (Peers, Send, Receive, History, Settings)
│   │   ├── components/         # Shared UI components
│   │   ├── hooks/              # React hooks (useP2PEngine, useTransfer)
│   │   ├── services/           # Tauri IPC wrappers (p2pBridge.ts, tauriBridge.ts)
│   │   └── __tests__/property/ # Frontend property tests (fast-check)
│   └── src-tauri/              # Rust backend (Tauri app)
│       ├── src/                # P2P engine modules
│       └── tests/              # Rust property tests (proptest)
├── server/                     # Reusable Rust library crates
│   ├── protocol/               # Protobuf codec (generated from proto/)
│   ├── transfer/               # Transfer sessions, chunk layout, rate control
│   ├── integrity/              # SHA-256 hashing and verification
│   ├── storage/                # Direct-offset file I/O
│   ├── rate_control/           # Rate limiting primitives
│   ├── observability/          # Structured logging and metrics
│   └── tests/                  # Integration tests
├── proto/
│   └── messages.proto          # Protobuf schema
└── .kiro/specs/                # Feature specs
```

## Getting Started

```bash
# Install frontend dependencies
cd frontend && npm install

# Run the app in dev mode (compiles Rust + starts Vite + opens window)
npm run tauri dev

# First build takes a few minutes. Subsequent builds are incremental.
```

## Development Workflow

### Running Tests

```bash
# Rust tests (Tauri app — unit + property tests)
cd frontend/src-tauri && cargo test

# Rust tests (server library crates — unit + property + integration)
cd server && cargo test --workspace

# Frontend tests (TypeScript property tests)
cd frontend && npm run test

# Lint frontend
cd frontend && npm run lint

# Type-check Rust without building
cd frontend/src-tauri && cargo check
```

### Dev Commands

| Command                  | Directory             | What it does                                      |
| ------------------------ | --------------------- | ------------------------------------------------- |
| `npm run tauri dev`      | `frontend/`           | Full dev mode (Vite + Rust + desktop window)      |
| `npm run dev`            | `frontend/`           | Vite dev server only (no Rust, no desktop window) |
| `npm run tauri build`    | `frontend/`           | Production build → native installer               |
| `cargo check`            | `frontend/src-tauri/` | Fast Rust type-check                              |
| `cargo test`             | `frontend/src-tauri/` | Run all Rust tests for the Tauri app              |
| `cargo test --workspace` | `server/`             | Run all server library crate tests                |
| `npm run test`           | `frontend/`           | Run TypeScript property tests                     |

---

## Adding a New Feature

### Step 1: Decide where the code goes

| What you're building             | Where it goes                                         |
| -------------------------------- | ----------------------------------------------------- |
| New P2P protocol message         | `proto/messages.proto` → `server/protocol/`           |
| New transfer/hashing logic       | `server/transfer/`, `server/integrity/`, etc.         |
| New Tauri backend functionality  | `frontend/src-tauri/src/`                             |
| New Tauri command (Rust → React) | `frontend/src-tauri/src/commands.rs`                  |
| New Tauri event (React ← Rust)   | `frontend/src-tauri/src/events.rs`                    |
| New UI screen or component       | `frontend/src/screens/` or `frontend/src/components/` |
| New React hook                   | `frontend/src/hooks/`                                 |

### Step 2: If adding a new protocol message

1. Edit `proto/messages.proto` — add your message type and a new field in the `Envelope` oneof:

```protobuf
message MyNewMessage {
    string id = 1;
    // ...
}

// Inside the Envelope oneof, add:
MyNewMessage my_new_message = 106;  // use next available field number
```

2. Rebuild the generated Rust code:

```bash
cd server/protocol && cargo build
```

`prost-build` runs automatically via `build.rs` and regenerates the Rust structs.

3. Re-export the new type in `server/protocol/src/lib.rs`:

```rust
pub use proto::{
    // ... existing types ...
    MyNewMessage,
};
```

4. Use it in the Tauri app (`frontend/src-tauri/src/p2p_engine.rs`):

```rust
use protocol::MyNewMessage;
```

### Step 3: If adding a new Tauri command

This is how the React UI calls into the Rust backend.

1. Add the command function in `frontend/src-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub async fn my_new_command(
    state: State<'_, AppState>,
    some_arg: String,
) -> Result<SomeResponse, CommandError> {
    let engine_guard = state.engine.read().await;
    let engine = engine_guard
        .as_ref()
        .ok_or(CommandError::from(P2PError::NotRunning))?;
    // ... delegate to engine ...
    Ok(response)
}
```

2. Register it in `frontend/src-tauri/src/lib.rs`:

```rust
.invoke_handler(tauri::generate_handler![
    // ... existing commands ...
    commands::my_new_command,
])
```

3. Add the TypeScript wrapper in `frontend/src/services/p2pBridge.ts`:

```typescript
export function myNewCommand(someArg: string): Promise<SomeResponse> {
  return invoke<SomeResponse>("my_new_command", { someArg });
}
```

4. Call it from a React component:

```typescript
import { myNewCommand } from "../services/p2pBridge";

const result = await myNewCommand("hello");
```

### Step 4: If adding a new Tauri event

This is how the Rust backend pushes real-time updates to the React UI.

1. Add the event constant and payload in `frontend/src-tauri/src/events.rs`:

```rust
pub const EVENT_MY_EVENT: &str = "my-event";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MyEventPayload {
    pub id: String,
    pub value: u64,
}

pub fn emit_my_event(app_handle: &tauri::AppHandle, payload: &MyEventPayload) {
    if let Err(e) = app_handle.emit(EVENT_MY_EVENT, payload.clone()) {
        tracing::warn!(error = %e, "Failed to emit my-event");
    }
}
```

2. Add the TypeScript listener in `frontend/src/services/p2pBridge.ts`:

```typescript
export interface MyEventPayload {
  id: string;
  value: number;
}

export function onMyEvent(
  callback: (payload: MyEventPayload) => void,
): Promise<UnlistenFn> {
  return listen<MyEventPayload>("my-event", (event) => {
    callback(event.payload);
  });
}
```

3. Subscribe in a React component:

```typescript
useEffect(() => {
  const unlisten = onMyEvent((payload) => {
    console.log("Got event:", payload);
  });
  return () => {
    unlisten.then((fn) => fn());
  };
}, []);
```

### Step 5: If adding a new UI screen

1. Create `frontend/src/screens/MyScreen.tsx`
2. Add it to the navigation in `frontend/src/App.tsx`:

```typescript
type Screen = "peers" | "send" | "receive" | "history" | "settings" | "myscreen";

// In the NavBar items array:
{ key: "myscreen", label: "My Screen" },

// In the render:
{screen === "myscreen" && <MyScreen />}
```

### Step 6: If adding a new reusable Rust library crate

1. Create the crate under `server/`:

```bash
cd server && cargo init --lib my_crate
```

2. Add it to `server/Cargo.toml` workspace members:

```toml
[workspace]
members = [
    # ... existing ...
    "my_crate",
]
```

3. Add it as a dependency in `frontend/src-tauri/Cargo.toml`:

```toml
my_crate = { path = "../../server/my_crate" }
```

4. Use it in the Tauri app:

```rust
use my_crate::SomeType;
```

---

## Modifying Existing Code

### Changing P2P engine behavior

The core P2P logic lives in `frontend/src-tauri/src/p2p_engine.rs`. Key methods:

| Method                                      | What it does                                               |
| ------------------------------------------- | ---------------------------------------------------------- |
| `P2PEngine::start()`                        | Initializes certs, QUIC listener, mDNS, background tasks   |
| `P2PEngine::shutdown()`                     | Graceful teardown of all subsystems                        |
| `P2PEngine::connect_to_peer()`              | Outgoing QUIC connection + PeerInfoExchange handshake      |
| `P2PEngine::send_file()`                    | Chunk file → send TransferRequest → stream chunks → verify |
| `P2PEngine::accept_transfer()`              | Accept incoming → receive chunks → verify → write to disk  |
| `P2PEngine::pause/cancel/resume_transfer()` | Transfer state transitions                                 |

### Changing peer discovery

mDNS logic is in `frontend/src-tauri/src/discovery.rs`. The service type is `_fileshare._udp.local.` with TXT records for `display_name`, `cert_fingerprint`, and `port`.

The peer registry (`peer_registry.rs`) is a `DashMap` — thread-safe, lock-free reads. Stale peers are evicted every 10 seconds.

### Changing settings

Settings struct is in `frontend/src-tauri/src/settings.rs`. If you add a new field:

1. Add the field to `P2PSettings` (with `#[serde(default)]` for backward compatibility)
2. Update `validate_p2p_settings()` if the field needs validation
3. Update `P2PConfig::from_settings()` if the engine needs the value
4. Add the field to the TypeScript `P2PSettings` interface in `p2pBridge.ts`
5. Add the input field in `frontend/src/screens/SettingsScreen.tsx`

### Changing the Protobuf schema

1. Edit `proto/messages.proto`
2. Run `cd server/protocol && cargo build` to regenerate
3. Update re-exports in `server/protocol/src/lib.rs` if you added new message types
4. Never remove or renumber existing fields — only add new ones

### Changing server library crates

The crates under `server/` (protocol, transfer, integrity, storage, observability) are compiled into the Tauri binary via path dependencies. Changes to these crates are picked up automatically by `cargo build` in `frontend/src-tauri/`.

After modifying a server crate, run its tests:

```bash
cd server && cargo test -p <crate_name>
# e.g. cargo test -p integrity
```

---

## Testing Guidelines

### Property-based tests

We use property-based testing (PBT) to verify correctness properties. There are 26 formal properties defined in the design spec.

| Language   | Library      | Location                                                               |
| ---------- | ------------ | ---------------------------------------------------------------------- |
| Rust       | `proptest`   | `frontend/src-tauri/src/*.rs` (inline) and `frontend/src-tauri/tests/` |
| TypeScript | `fast-check` | `frontend/src/__tests__/property/`                                     |

When adding a new property test:

1. Tag it with `// Feature: p2p-tauri-desktop, Property N: Title`
2. Use at least 100 iterations (proptest default is 256)
3. Test both the happy path and the failure/edge cases

### Test naming conventions

- Rust property tests: `prop_<description>`
- Rust unit tests: `test_<description>` or descriptive name
- TypeScript property tests: describe block with `Property N: Title`

### What to test when

| Change                  | Tests to run                          |
| ----------------------- | ------------------------------------- |
| Modified a server crate | `cd server && cargo test -p <crate>`  |
| Modified Tauri backend  | `cd frontend/src-tauri && cargo test` |
| Modified React UI       | `cd frontend && npm run test`         |
| Modified proto schema   | `cd server && cargo test -p protocol` |
| Before committing       | All three test suites                 |

---

## Architecture Decisions

### Why path dependencies instead of a separate server?

The `server/` crates are compiled directly into the Tauri binary. There is no separate server process. This gives us:

- Single native executable
- No network overhead for backend calls (Tauri IPC is in-process)
- Shared Rust types between the P2P engine and the library crates

### Why `std::sync::RwLock` in discovery.rs instead of `tokio::sync::RwLock`?

The discovery service's `local_info` and `fullname` fields are accessed from both sync (mDNS callbacks on OS threads) and async (Tauri command handlers) contexts. `tokio::sync::RwLock::blocking_read()` panics when called from within a tokio runtime. `std::sync::RwLock` works in both contexts since the lock is held briefly for simple data reads.

### Why does `start_engine` return existing info instead of erroring?

The `start_engine` command is idempotent — if the engine is already running, it returns the existing `EngineInfo`. This avoids race conditions between the Tauri setup hook and the React UI's `useP2PEngine` hook both trying to start the engine.

---

## Commit Message Convention

Use conventional commits:

```
feat: add batch file transfer support
fix: resolve mDNS discovery timeout on Windows
refactor: extract chunk verification into separate module
test: add property test for resume with corrupted chunks
docs: update contributing guide with new crate instructions
```

## Code Style

- Rust: follow `rustfmt` defaults, use `clippy` for lints
- TypeScript: ESLint config in `frontend/eslint.config.js`
- Keep functions small and focused
- Prefer `Result<T, E>` over panics in Rust
- Use `tracing::info!`/`warn!`/`error!` for logging, not `println!`
