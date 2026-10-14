# Fuzzing

This is an independent cargo-fuzz crate, outside the computer-use workspace.
Run only on Unix (Linux/macOS, x86-64 or AArch64), never Windows.

From `computer-use/` on the build machine:

```sh
cargo +nightly fuzz run mcp_codec -- -max_total_time=30 -max_len=1048577
cargo +nightly fuzz run host_codec -- -max_total_time=30 -max_len=16777220
```

For execution worktrees use the remote compiler:

```sh
rcargo --sh 'cargo +nightly fuzz run mcp_codec -- -max_total_time=30 -max_len=1048577'
```

Requires nightly Rust, llvm-tools-preview, and cargo-fuzz. The target checks
chunk-boundary independence, malformed UTF-8/JSON, bounded line recovery, and
EOF handling through the same NDJSON decoder used by stdio. Reproduce a saved
crash with `cargo +nightly fuzz run mcp_codec fuzz/artifacts/mcp_codec/<file>`.
CI scheduling belongs to P9.
