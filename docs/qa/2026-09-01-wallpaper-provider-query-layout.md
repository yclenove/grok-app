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
- A failed hidden prefetch stays silent and does not consume the user's explicit
  load-more action. That click starts one fresh foreground paging request; a
  second failure keeps the existing gallery and the manual retry available.
- The source navigation remains above one full-width workspace but no longer
  bunches all seven choices into one framed cluster. Discovery sources use the
  left side, Imagine occupies the middle segment, and personal sources use the
  right side.
- Group frames, group headings, persistent source descriptions, empty-state
  teaching copy, and footer instructions are absent. At narrow widths the four
  discovery sources get a scrollable row without hiding their text labels.
- When the official Grok Saved page leaves its ready state, stale local text and
  media-kind filters are cleared with the transient album snapshot. Verification
  or navigation therefore cannot leave a misleading zero-count filter row, and
  a filtered empty state exposes only one clear action.

## Real Windows Tauri acceptance

The development application was exercised with the real Host pipeline. The
evidence below records only counts, timing, and interaction state; it excludes
the query text, account data, credentials, provider payloads, media URLs, and
network-exit details.

| Scenario | Result |
|---|---|
| Source navigation | Seven sources rendered as four discovery choices, one Imagine choice, and two personal choices across the available row; no group frames, headings, persistent descriptions, or footer instruction remained |
| Grok album verification | The dedicated official-page window opened and presented its verification state; the main dialog kept the actionable guidance while hiding stale zero-count filters and duplicate clear actions |
| Openverse fresh search | 18 validated cards arrived in 9.1 seconds |
| Openverse cached search | The first batch restored immediately from the in-process cache |
| Openverse load more | 20 prefetched cards appended immediately, taking the gallery from 18 to 38 |
| Openverse interaction after paging | A card from the original batch remained selectable and opened the real Lightbox; source, author, and license actions remained available |
| Pexels without a key | The dialog showed the required-key state, kept search and the empty save action disabled, and made no provider request |
| Web fresh search | Four validated cards arrived in about 32 seconds; a card clicked while validation was still running opened in the real Lightbox after the concurrent update |
| Web load-more failure | The request reported a network timeout and appended no cards; all four existing cards stayed in place and remained selectable afterward |
| Web post-fix fresh search | Three validated cards arrived in 55.0 seconds |
| Web post-fix load more | The explicit action remained active for about 70 seconds before reporting a network timeout instead of immediately replaying the hidden failure; the three existing cards stayed selectable and opened in the real Lightbox while paging was active, no result was falsely appended, and manual load more remained available |

This confirms that the earlier one- or two-card direct-library result was not
caused by a one- or two-item page-size parameter. Provider matching was being
degraded by display-purpose and resolution modifiers in the query. Web results
can still be smaller because source discovery, page access, image validation,
quality filtering, and deduplication are separate attrition stages. The Web
run also verifies the interaction-preservation contract, but it does not count
as successful Web pagination because the load-more request timed out.

## Deterministic verification

| Check | Result |
|---|---|
| `pnpm deps:check` | Passed |
| `pnpm audit:prod` | Passed; no known production vulnerability |
| `pnpm test` | 565 files, 6877 tests passed |
| Focused post-change Vitest | 3 files, 22 tests passed |
| Focused post-change ESLint | Passed for the pagination controller and its regression suite |
| `pnpm typecheck` | Passed |
| `pnpm lint` | Passed |
| `pnpm build:ui` | Passed; only existing dynamic-import and large-chunk warnings |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test --no-run` | Passed |
| Manifest-embedded Windows Rust harness | 1646 passed, 0 failed, 1 ignored |
| `git diff --check` | Passed |
| `python scripts/check-code-quality-gates.py` | One inherited failure: 77 files at or above 1,000 lines against a budget of 69; every other gate passed |

The post-acceptance album-filter regression suite adds focused coverage for the
ready-to-verification transition and the single clear-action invariant. The
pagination controller also covers both the successful fresh request after a
hidden prefetch failure and the foreground-failure preservation path. The full
suite count above predates these final two pagination cases; focused post-change
checks passed 3 files and 22 tests before the final local commit.

No production file crossed the 1,000-line threshold in this scope. The Rust
provider module remains below the threshold at 986 lines; the two modified CSS
parts were already above the threshold before this pass, and the edited part 3
file decreased by two lines.

## Privacy and release state

No account identifier, credential, raw provider response, original query,
media URL, or proxy-exit detail is present in this report or the new tests.
This acceptance is local-only: no push, pull request, tag, release, or
deployment is part of the work.
