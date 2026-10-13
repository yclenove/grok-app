# Computer Use workspace

Independent Rust workspace for Grok Computer Use v2. It lives outside
`src-tauri` and builds separately from the desktop App. The technical contract
is [architecture §2](../docs/plans/computer-use-v2/03-architecture.md#2-crate-划分与依赖规则).

```text
computer-use/
├── Cargo.toml                 Workspace packages, dependencies, lints, profiles
├── Cargo.lock                 Reproducible dependency resolution
├── rust-toolchain.toml        Pinned stable Rust, rustfmt, clippy
├── deny.toml                  License, advisory, and source policy
├── .cargo/config.toml         cargo xtask alias
├── crates/
│   ├── gcu-protocol/           Shared tool parameters, results, errors, capabilities
│   ├── gcu-host-api/           Host traits: approval, overlay, policy, events, clock
│   ├── gcu-host-protocol/      Versioned wire protocol for external Hosts
│   ├── gcu-core/               Platform-independent engine logic and Driver trait
│   ├── gcu-driver-macos/       macOS driver, gated by target_os = "macos"
│   ├── gcu-driver-windows/     Windows driver, gated by windows
│   ├── gcu-driver-linux/       Linux driver, gated by target_os = "linux"
│   ├── gcu-driver-fake/        Deterministic desktop for offline tests
│   ├── gcu-script/             Optional script sandbox
│   ├── gcu-mcp/                MCP transport and tool registration
│   ├── gcu-overlay/            Standalone Host overlay
│   └── gcu-cli/                gcu binary
├── apps/fixture/               README only; egui + AccessKit app belongs to P1-01
├── fixtures-native/            README only; native fixtures belong to P1-01
└── xtask/                      Development gates and dependency-rule tests
```

Crate entry points are documented skeletons; product APIs and behavior belong
to their subsequent roadmap cards. Platform driver libraries are empty on other
operating systems, so all workspace members can build together on each platform.

`src-tauri` has no pinned toolchain and its CI uses stable. This workspace pins
the current stable release, **Rust 1.99.0**, with the minimal profile plus rustfmt
and clippy. Install Rust with rustup; let the toolchain file select the version.
All packages inherit edition 2021, version 0.0.0, MIT, and `publish = false`.
Public crate entry points deny missing documentation; the workspace warns on
missing docs and denies unsafe operations inside unsafe functions and Clippy
unwrap/expect usage. Crate unit tests and the xtask test module allow unwrap/expect
in assertions.

Run these commands from this directory (source `~/.cargo/env` if Cargo is not
already on `PATH`):

```sh
cargo build --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask check-deps
cargo xtask golden --check
cargo xtask budget
cargo install --locked cargo-deny # once, if needed; use version >= 0.18
cargo deny check
```

`cargo run -p xtask -- check-deps` is equivalent to the alias. The gate checks
every declared dependency edge from `cargo metadata --no-deps`, including optional,
target, dev, build, and renamed dependencies. It enforces the architecture table
in `xtask/src/deps.rs`; unknown workspace members require an explicit rule.
Drivers may depend on protocol/core and external native/support crates, but
cannot depend on other computer-use crates. Architecture §2 does not specify
restrictions for script, overlay, or xtask; those entries are explicit in the table.
Dependency names cannot prove restrictions on source-level IO, API use, or the
loopback-only behavior of axum; those remain architectural review requirements.

The same gate statically reads `../src-tauri/Cargo.toml` without building Tauri.
Among computer-use crates, App may depend only on `gcu-protocol` and
`gcu-host-protocol`. Target/dev/build tables and `package` aliases are inspected;
inherited aliases are resolved from an in-manifest workspace dependency table,
and unresolved inheritance is an error. Read or metadata failures return nonzero.

`golden` and `budget` currently print `SKIP: not implemented yet: P0-03` and
return 0. Schema generation and byte-budget validation belong to P0-03; a skip
does not certify either gate. These subcommands will be replaced there.

The license allowlist rejects GPL/AGPL/LGPL; architecture §2 additionally requires
MPL-2.0 dependencies to be dynamically linked. The source policy permits only
crates.io and the advisory policy has no exceptions. Network/database failures
must be reported and must not be addressed by relaxing `deny.toml`.
