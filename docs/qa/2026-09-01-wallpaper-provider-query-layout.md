# Wallpaper provider query and source-layout acceptance

Date: 2026-09-01 (Asia/Shanghai)

Branch: `fix/wallpaper-responses-network-observability`

Scope: fix low-yield direct-library searches, preserve gallery interaction
during paging, and simplify the seven-source wallpaper dialog. This pass does
not change the X, Web, Imagine, Grok album, proxy, credential, or fallback
contracts.

## Outcome

- Openverse and Pexels now receive a provider-only query derived from the
  normalized user query. Standalone wallpaper-purpose, resolution, and
  localized wallpaper modifiers are removed before the provider call.
- If derivation would produce an empty query, the Host uses the original query
  instead of sending an empty provider request.
- Cache and pagination identity continue to use the original normalized query.
  The direct-library cache contract is version 2, so earlier low-yield entries
  cannot mask the corrected behavior.
- Initial search and load more use the same derivation. Web, X, and Imagine
  queries are not rewritten or silently translated.
- Existing gallery cards remain selectable while foreground paging is active.
  The real Lightbox integration test covers that interaction, and background
  prefetch remains invisible until the user requests more results.
- The source navigation remains above one full-width workspace but no longer
  bunches all seven choices into one framed cluster. Discovery sources use the
  left side, Imagine occupies the middle segment, and personal sources use the
  right side.
- Group frames, group headings, persistent source descriptions, empty-state
  teaching copy, and footer instructions are absent. At narrow widths the four
  discovery sources get a scrollable row without hiding their text labels.

## Real Windows Tauri acceptance

The development application was exercised with the real Host pipeline. The
evidence below records only counts, timing, and interaction state; it excludes
the query text, account data, credentials, provider payloads, media URLs, and
network-exit details.

| Scenario | Result |
|---|---|
| Corrected direct-library initial search | 20 validated cards in 25.3 seconds |
| Prefetched load more | Added 19 cards, reaching 39 total |
| Existing-card interaction after paging | A previously rendered card remained selectable and opened the real Lightbox |
| Viewer identity | The selected item opened as slide 16 of 39 |
| Stacked Escape ownership | Escape closed only the Lightbox and preserved the wallpaper dialog |
| Attribution | Source, author, and license actions remained attached to results |
| Source navigation | The three semantic segments used the available row instead of clustering at the left |

This confirms that the earlier one- or two-card direct-library result was not
caused by a one- or two-item page-size parameter. Provider matching was being
degraded by display-purpose and resolution modifiers in the query. Web results
can still be smaller because source discovery, page access, image validation,
quality filtering, and deduplication are separate attrition stages.

## Deterministic verification

| Check | Result |
|---|---|
| `pnpm deps:check` | Passed |
| `pnpm audit:prod` | Passed; no known production vulnerability |
| `pnpm test` | 565 files, 6875 tests passed |
| `pnpm typecheck` | Passed |
| `pnpm lint` | Passed |
| `pnpm build:ui` | Passed; only existing dynamic-import and large-chunk warnings |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test --no-run` | Passed |
| Manifest-embedded Windows Rust harness | 1600 passed, 0 failed, 1 ignored |
| `git diff --check` | Passed |
| `python scripts/check-code-quality-gates.py` | One inherited failure: 77 files at or above 1,000 lines against a budget of 69; every other gate passed |

No production file crossed the 1,000-line threshold in this scope. The Rust
provider module remains below the threshold at 986 lines; the two modified CSS
parts were already above the threshold before this pass, and the edited part 3
file decreased by two lines.

## Privacy and release state

No account identifier, credential, raw provider response, original query,
media URL, or proxy-exit detail is present in this report or the new tests.
This acceptance is local-only: no push, pull request, tag, release, or
deployment is part of the work.
