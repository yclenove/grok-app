# Wallpaper provider query and source-layout acceptance

Date: 2026-09-02 (Asia/Shanghai)

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
  The regression covers both a rejected Promise and the real Host shape: a
  resolved result carrying `errorCode: "provider_timeout"`.
- The source navigation remains above one full-width workspace. Desktop widths
  present all seven labeled choices with an even rhythm; narrow widths keep one
  horizontally scrollable row instead of stacking the choices into several
  crowded lines.
- Group frames, group headings, persistent source descriptions, empty-state
  teaching copy, and footer instructions are absent. Every source keeps both its
  icon and text label.
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
| Source navigation | Seven labeled sources rendered in one compact, even desktop row; no group frames, headings, persistent descriptions, or footer instruction remained |
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
| Current Web fresh search | 19 validated cards arrived in 59.9 seconds; all visible cards retained their source action |
| Current first load more | The prefetched result appended five cards in about 0.2 seconds, expanding the gallery from 19 to 24 |
| Current foreground load more | The next operation waited about 60 seconds before reporting a network timeout; an original card selected normally and opened as slide 19 of 24 in the real Lightbox while paging was active, all 24 cards remained afterward, and manual retry stayed available |
| Repeated Web fresh search | 13 validated cards arrived in 55.0 seconds, providing another non-low-yield result from the three-lane route |
| Repeated Web paging interaction | The buffered page added three cards; during the next paging request an existing card selected, enabled the apply action, and opened as slide 14 of 16 in the real Lightbox; the request then added six cards for 22 total |
| Final 1024 px layout pass | All seven icon-and-label sources remained in one row above the full-width workspace; X, Web, Openverse, Pexels, Imagine, Grok album, and library exposed only their actionable controls and real state, with no persistent source teaching or footer copy |
| Final Grok album first page | The official Saved page synchronized 20 visible items while 44 validated entries were already warm |
| Final Grok album load more | The first explicit action revealed 40 items immediately, then background warming advanced the cache to 62 without clearing, reordering, or remounting the visible gallery |
| Final Grok album interaction | Existing images remained selectable and opened in the real Lightbox; the local video filter exposed two available entries and their preview path opened normally |
| Final Web fresh search | 15 validated cards arrived in 56.5 seconds |
| Final Web foreground paging | Five new cards expanded the gallery from 15 to 20; an existing card selected and opened in the real Lightbox while the request was active, and the original selection remained afterward |
| Final Openverse fresh search | 11 validated cards arrived in 4.2 seconds; source, author, and Creative Commons license metadata remained visible and actionable |
| Localized Openverse no-result case | A long localized query returned no cards; this is the honest provider result because the source contract forbids silently translating the user's theme |

This confirms that the earlier one- or two-card direct-library result was not
caused by a one- or two-item page-size parameter. Provider matching was being
degraded by display-purpose and resolution modifiers in the query. Web results
can still be smaller because source discovery, page access, image validation,
quality filtering, and deduplication are separate attrition stages. The Web
timeout run verifies the failure-preservation contract. The repeated run also
completed a foreground Web expansion successfully, so the interaction contract
is now covered under both failed and successful paging. The final repeat pass
also confirms the one- or two-card symptom is not a fixed request-size limit:
the same production UI returned 15 Web cards and 11 Openverse cards, with five
more Web cards surviving the full foreground paging and validation path.

## Deterministic verification

| Check | Result |
|---|---|
| `pnpm deps:check` | Passed |
| `pnpm audit:prod` | Passed; no known production vulnerability |
| `pnpm test` | 587 files, 7052 tests passed |
| Focused post-change Vitest | 6 files, 44 tests passed |
| Focused post-change ESLint | Passed for the pagination controller and its regression suite |
| `pnpm typecheck` | Passed |
| `pnpm lint` | Passed |
| `pnpm build:ui` | Passed; only existing dynamic-import and large-chunk warnings |
| `cargo fmt --all -- --check` | Passed; the inherited drift in `src-tauri/src/lib.rs` and `src-tauri/src/plugin_mcp.rs` was normalized in an isolated formatting-only commit |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test --no-run` | Passed |
| Manifest-embedded Windows Rust harness | 1712 passed, 0 failed, 1 ignored |
| Manifest-embedded `wallpaper_x` tests after the module split | 32 passed, 0 failed |
| `git diff --check` | Passed |
| `python scripts/check-code-quality-gates.py` | Passed; 77 files at or above 1,000 lines against the 77-file budget |

The post-acceptance album-filter regression suite adds focused coverage for the
ready-to-verification transition and the single clear-action invariant. The
pagination controller also covers both resolved and rejected hidden-prefetch
failures, the successful fresh foreground request, and the foreground-failure
preservation path. The full and focused suite counts above include these final
pagination cases.

No production file crossed the 1,000-line threshold in this scope. The Rust
provider module remains below the threshold at 986 lines. Inline tests were
mechanically moved out of `wallpaper_x_responses.rs` and
`wallpaper_x_search.rs`, leaving the production modules at 691 and 760 lines
without changing behavior or reducing the test count. The repository-wide
budget is therefore back to 77/77. The two modified CSS parts were already
above the threshold before this pass, and the edited part 3 file decreased by
three lines.

## Privacy and release state

No account identifier, credential, raw provider response, original query,
media URL, or proxy-exit detail is present in this report or the new tests.
The final live Saved sample did not contain a blob-backed video, so this report
does not upgrade that deterministic-only path to a real-device claim. A valid
Pexels key also remains unavailable; authenticated Pexels acceptance is still
bounded by the separate 2026-09-02 cache-isolation report.
This acceptance is local-only: no push, pull request, tag, release, or
deployment is part of the work.
