# Wallpaper Responses progressive search and load-more acceptance

Date: 2026-08-29 (Asia/Shanghai)

Branch: `fix/wallpaper-responses-network-observability`

Scope: final acceptance for progressive `3 × 8` Responses search and one explicit `1 × 8` load-more request. The stable CLI default, hidden `auto` mode, fixed model/effort, OAuth boundary, and read-only X-only source contract remain unchanged.

## Outcome

The planned interactive Responses path is complete:

- an initial manual-preview search starts three complementary lanes concurrently;
- each lane targets eight candidates and may use at most three `x_search` calls;
- a validated lane is emitted immediately instead of waiting for all lanes;
- the final invoke remains authoritative for complete metadata and ranking;
- after the initial search finishes, the user may explicitly request one additional eight-item lane;
- load more appends and deduplicates against Host-owned cached identities, never clears the existing gallery, never retries automatically, and never falls back to CLI;
- cancellation, modal/source changes, replacement searches, and unmount invalidate the active generation and reject late results.

This improves perceived availability and gallery depth without changing the default route. It does not justify exposing `auto`: live latency and transport reliability still vary, and the CLI remains the more reliable high-volume fallback.

## Implementation commits

| Commit | Scope |
|---|---|
| `0a40cb50` | Add the progressive batch event contract and deterministic DTO tests |
| `ba93bc30` | Run three Responses lanes and emit validated batches in completion order |
| `82f8327c` | Render progressive batches with request/generation isolation |
| `bf2cb867` | Add one explicit, bounded load-more action while preserving the gallery |
| `f55277a2` | Record sanitized output/latency calibration and benchmark support |

## Deterministic verification before live acceptance

The feature-level verification passed:

- 79/79 targeted wallpaper and i18n tests;
- `pnpm typecheck`;
- `pnpm lint`;
- `cargo fmt --all -- --check`;
- `cargo clippy --all-targets -- -D warnings`;
- `cargo test --no-run`;
- 32/32 wallpaper Rust tests in a Windows harness with the required Common Controls v6 manifest;
- `git diff --check`.

The repository-wide code-quality gate still reports 78 files at or above 1,000 lines against a budget of 69. This is an existing repository baseline, not a new crossing: none of the production files changed by this feature moved from below 1,000 lines to above it. All other quality-gate checks passed.

## Final repository-wide verification

The final branch HEAD was revalidated after the implementation and research commits:

| Check | Result |
|---|---|
| `pnpm deps:check` | Passed; pnpm-only root remains clean |
| `pnpm audit:prod` | Passed; no known production vulnerability |
| `pnpm lint` | Passed |
| `pnpm typecheck` | Passed |
| `pnpm exec vitest run --maxWorkers=4` | 545 files, 6775 tests passed |
| `pnpm build:ui` | Passed; only existing dynamic-import and large-chunk warnings |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test --no-run` | Passed |
| Manifest-embedded Windows Rust harness | 1574 passed, 0 failed, 1 ignored |
| `py -3 scripts/check-code-quality-gates.py --mode final --json` | One existing failure: `FILES_OVER_1K_BUDGET` is 78 > 69; every other gate passed |
| `git diff --check` | Passed |

## Real Tauri acceptance

The checks below used the development Tauri application and the real Host pipeline. They intentionally retain only counts, elapsed time, stable behavior, and citation totals. No query text, username, media URL, account data, Authorization content, raw provider response, exit IP, or node identifier is recorded.

### Initial progressive search

The first uncached search completed in 65.4 seconds and returned 20 validated images with 20 canonical status citations. The gallery remained responsive and displayed the load-more action after terminal completion.

A second uncached search demonstrated real progressive delivery: five validated images became visible at 62.1 seconds while the remaining lanes were still active. The Host completed at 89.3 seconds with 13 validated images and 13 canonical status citations. This confirms that the UI no longer waits for the slowest lane before showing useful results.

### Load-more failure preservation

The first load-more attempt ended in a generic error after about 89 seconds. The original 20 images remained intact, no CLI fallback ran, and the one-attempt action was consumed for that search generation. A failed enrichment therefore cannot erase or replace a successful initial gallery.

### Cache behavior

Repeating the same initial search hit the Host cache and restored the 20-image gallery in about 1.37 seconds. The load-more action was available again because it belongs to the new UI search generation, while provider split timings from the original request were not presented as current cost.

### Load-more cancellation and late-result isolation

Cancelling load more returned the UI to idle in about 0.254 seconds. The gallery stayed at 20 images, no error appeared, and no late batch or final result changed it during an additional 11.6-second observation window. The explicit action became available again because cancellation does not consume the attempt.

### Successful load more

A subsequent user-triggered load-more request completed in about 82 seconds and grew the gallery from 20 to 28 images: exactly eight new validated items after Host-side exclusion and final deduplication. The action then disappeared, matching the one-request contract.

## Product decision

- Keep `cli` as the default and keep `auto` hidden.
- Keep progressive `3 × 8` inside the manually selected Responses preview.
- Keep load more explicit, one-lane, one-attempt, non-fallback, and non-retrying.
- Do not send authenticated search traffic as speculative prewarming. Safe preloading remains limited to local OAuth metadata, effective proxy/endpoint health, reusable client state, and fresh in-memory cache.
- If generic Web image search is added later, implement it as a separate source with explicit provenance; never mix it silently into X results.

## Privacy and workspace state

No credential, raw provider output, user query, media URL, username, or account identifier is present in this report or the application telemetry added by this work. No push, PR operation, tag, release artifact, changelog edit, or user proxy-setting change is part of this acceptance.

The unrelated `src-tauri/Cargo.toml` line-ending-only working-tree status remains excluded from every commit.
