# Wallpaper X search stage 5 lifecycle report

Date: 2026-08-28
Branch: `feat/wallpaper-responses-search`
Scope: request identity, progress, cooperative cancellation, late-result protection, and bounded in-memory caching

## Decision

Stage 5 is ready for local integration. It does not change the rollout decision from stage 4: `cli` remains the default, `responses_preview` remains an explicit manual preview, and `auto` remains hidden.

The lifecycle work is production-shaped rather than a UI-only shortcut:

- Every search has a UUID request id shared by the frontend invoke, Host registry, progress events, result metadata, and cancel command.
- The frontend owns one request generation at a time. Cancel, replacement, tab switch, modal close, or unmount invalidates that generation before awaiting Host cleanup, so a late result cannot repopulate the UI.
- The Host cancellation signal is sticky and shared by all waiters. It interrupts Responses request/body reads, image validation, and both CLI search rounds. Windows CLI cancellation terminates the process tree.
- Cancellation never starts the CLI fallback and never writes a cache entry.
- Successful non-empty results use a Host-local 32-entry LRU cache with a 10-minute TTL. Keys include normalized query, sort, requested route contract, and the Build credential revision where the Responses route depends on it.
- Cache hits preserve the actual route and fallback metadata while replacing the request id, marking `cacheHit`, and measuring only cache-return time.

No token, auth payload, request header, raw Responses body, media URL, or username is logged by this lifecycle layer.

## User-visible behavior

- Progress stages: preparing, searching X, validating image quality, supplementing sparse results, and falling back.
- The search button becomes an explicit cancel action while X search is active.
- X search may be cancelled by the button, by switching to Imagine or the library, by closing the modal, or by starting a replacement search.
- A cache hit is shown as “loaded from cache” alongside the real route instead of as a synthetic route.
- All new copy is present in the 15 shipped locale catalogs.

## Deterministic verification

Rust compilation and focused harnesses:

- `cargo test wallpaper_x --no-run` passed.
- After embedding the repository Windows test manifest in the generated harness, `wallpaper_source` passed 22/22, `wallpaper_x_search` passed 13/13, and `wallpaper_x_responses` passed 7/7 (42/42 total).
- Coverage includes sticky multi-waiter cancellation, pre-registration cancel tombstones, registry cleanup, sub-two-second CLI cancellation, cancellable Responses HTTP/body work, no fallback after cancel, TTL/LRU/cache-key separation, and honest cached route metadata.

Frontend checks:

- `pnpm typecheck` passed.
- ESLint passed for all changed frontend files.
- Seven focused Vitest files passed 98/98.
- Hook tests cover active-request-only progress, prompt cancellation, late-result rejection, replacement search, and unmount cleanup.
- Modal tests cover progress copy, the cancel action, tab-switch cancellation, and close cancellation.

## Windows live QA

The development build was exercised against the real Grok Build CLI on Windows. The test retained no raw search response or source identifiers.

| Scenario | Result |
|---|---|
| Manual cancel while searching | Returned to idle in about 0.8 s; no fallback or late result appeared |
| Switch to Imagine while searching | Search cancelled in about 0.8 s; Imagine controls recovered |
| Close modal while searching | Modal closed; reopening showed no result from the cancelled generation |
| Full real search | Grok Build CLI returned 14 validated images in 54.8 s |
| Repeat same key | UI reported `cache · Grok Build CLI · 0.0 s`; the state was settled within the 1.1 s automation observation window, including an intentional 0.8 s wait |
| Dark theme | Modal, scrim, progress, cancel action, gallery, and route badges were readable with no clipping or stacking issue |
| Light theme | Modal, scrim, cached route, gallery, filters, and footer were readable with no clipping or stacking issue |
| Theme cleanup | Original dark theme was restored after QA |
| Development log | No new warning, panic, auth dump, or lifecycle error was emitted during the scenarios |

## Known boundaries

- The cache is intentionally process-local and disappears on restart. It does not persist media metadata or credentials to disk.
- Cache entries are not shared across route modes. Responses entries also change with the credential file revision.
- Cancellation is cooperative at the request lifecycle level. The frontend becomes idle immediately; Host process-tree cleanup can finish just after the UI transition.
- The cache improves repeated searches, not the first search. Stage 4 remains the source of truth for first-search route quality and latency.
