# ADR: Broker-owned surface executors

Date: 2026-09-13

Status: accepted; implementation remains partial until every product surface has a real executor

Context: R1

## Decision

`ComputerUseBroker` owns one `SurfaceExecutorRegistry`. The registry maps each
`SurfaceKind` to one process-lifetime `Arc<dyn ComputerUseAdapter>` and an
immutable executor generation. Desktop is registered by the Broker constructor;
other product adapters are registered explicitly by the Host.

An authorized binding records both its surface and executor generation. Every
generic execution and lifecycle operation resolves that binding through the
registry before doing work. An unknown, unavailable, mismatched, or replaced
executor fails closed with a typed error. It never resolves to Desktop.

## Ownership and generations

- The registry owns adapter references; each adapter owns its surface-specific
  target handles and cleanup rules.
- Registering the same `Arc` twice is idempotent. Registering a different
  adapter for an occupied surface is rejected instead of silently replacing a
  live executor.
- Authorization snapshots the executor generation. Observe, act, alive,
  capabilities, preview, pause, stop, and release reject a generation mismatch
  before dispatch.
- Only a Desktop binding acquires `DesktopLease`. Managed Browser, Existing
  Tabs, and App WebView never contend for or inherit that lease.

## Synchronization boundary

The Broker and adapter contract remains synchronous. Tauri async commands move
blocking Broker work off the async worker. A blocking managed-browser HTTP
client must additionally run on a plain OS thread as defined by the blocking
HTTP ADR; no Broker mutex may be held across worker I/O.

The registry lock protects only map access. It is released before any adapter
method runs. Broker run-state locks are likewise released before observe, act,
abort, worker I/O, or target release.

## Cleanup semantics

Stop and pause first revoke dispatch by advancing the run generation and
cancelling its token. Surface and browser cleanup are then attempted without a
Broker state lock.

- Failed stop cleanup leaves `stop_requested` and a pending marker. A repeated
  stop retries cleanup with the same revoked generation. The target is released
  only after every required cleanup succeeds.
- Failed pause cleanup leaves the run paused and a pending marker. Resume is
  rejected until an idempotent pause retry completes.
- Preview stop and adapter abort are surface-routed. A target release may clean
  up only the matching binding, so a late cleanup cannot unbind a newer run.
- Cleanup errors remain visible; they are not converted to `stopped` or
  swallowed to make a gate pass.

## Product registration state

- Desktop: registered by `ComputerUseBroker::new`.
- App WebView: registered by `ensure_host_runtime`; its generic MCP route is
  Broker-owned.
- Managed Browser: must receive a real adapter over the existing Host worker;
  until then generic authorization returns `surface_unavailable`.
- Existing Tabs: remains unavailable until authenticated extension transport
  exists. A registry entry or synthetic tab is not a substitute for transport.

## Consequences

Compatibility `browser_*` tools may remain temporarily, but they must converge
on the same executor, grant, action ledger, cancellation, and cleanup state.
Direct adapter calls are allowed only for explicit Host provisioning or isolated
adapter probes; they cannot count as a product Computer Use loop.

This ADR does not claim cross-platform, installed-App, Existing Tabs, or real
model acceptance. Those remain independent R2-R13 gates.
