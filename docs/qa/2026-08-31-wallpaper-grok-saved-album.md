# Grok Imagine Saved album wallpaper acceptance

Date: 2026-08-31 (Asia/Shanghai)

Branch: `fix/wallpaper-responses-network-observability`

Scope: independent `Grok album` wallpaper source backed by the official
`https://grok.com/imagine/saved` page. This feature is separate from X search,
does not reuse Grok Build OAuth, and does not call undocumented consumer REST
endpoints.

## Current outcome

The implementation and deterministic gates are complete. Real Windows Tauri
acceptance has verified the isolated-window and security-challenge states, the
dedicated remote WebView's sanitized Manual proxy pin, persistent official
sign-in, paging/warming behavior, and the available image and video paths.
Automation did not click or solve the challenge; testing resumed only after the
human verification had been completed in the official window.

During the final live pass, Cloudflare served a newer full-page challenge whose
outer DOM no longer matched the original form and iframe selectors. The Host
initially classified that page as signed out. The fixed snapshot now also
recognizes a challenge asset only when the Grok application shell is absent,
which avoids misclassifying a normal Grok page that happens to load a challenge
resource. A rebuilt Tauri process then displayed the localized verification
state while the isolated window showed the current official challenge.

Static UX review also found that a card whose first thumbnail request failed
did not observe a later successful background warmup. The in-memory thumbnail
cache now notifies mounted cards on successful recovery and on account-media
invalidation, while LRU eviction remains non-notifying so long visible albums
do not churn downloads. The focused cache/component regression covers initial
success, image-element failure, background recovery, and source clearing.

The final viewer audit found one more identity edge case: multiple uncached
cards share the same bounded placeholder, so selecting by placeholder URL could
open the first lazy card instead of the card the user clicked. The viewer now
keeps each resolved slide's original input index, resolves sibling sources
concurrently, and hydrates the selected original without blocking the initial
Lightbox mount. Deterministic coverage locks both the duplicate-placeholder
selection and concurrent-resolution behavior.

The final source-dialog pass presents all seven labeled sources as one compact,
even row above the full-width gallery. Narrow windows keep the same row and
scroll it horizontally instead of stacking sources into crowded groups. It
removes group frames, persistent source descriptions, repeated Grok album
captions, and footer instructions. The ready toolbar is reduced to its status
dot and visible/cached counts; the complete status and privacy contract remain
available to assistive technology and the existing info tooltip. A stacked
layer regression also keeps Escape ownership with the Lightbox first, so one
key press closes the preview without closing the wallpaper dialog underneath.

The available Saved page contained no blob-backed video. That bounded transfer
path is covered by deterministic tests, but this report deliberately does not
claim a live blob-video result.

## Security and privacy contract

- The remote window has a dedicated persistent WebView profile and no Tauri
  capability entry.
- Windows/Linux Manual HTTP/SOCKS5 settings are pinned to the remote WebView;
  unsupported or authenticated routes fail closed. System proxy modes retain
  native WebView behavior, and a saved proxy change destroys the old window so
  the next open cannot keep a stale manual route.
- Top-level navigation is HTTPS-only and restricted to Grok/xAI/X plus the
  explicitly supported Google and Apple identity-provider hosts. New windows
  and downloads are denied.
- The Host executes only bundled scripts. Frontend IPC cannot supply
  JavaScript, endpoints, headers, or credentials.
- Cookies, tokens, storage, request signatures, headers, and raw network
  responses never cross the bridge and are never logged.
- Bridged metadata is bounded and allowlisted to generated media under
  `assets.grok.com`; unknown query values are removed before IPC.
- Thumbnails remain in memory. Only a user-selected original is written to the
  distinct `grok_album` wallpaper-library directory after URL, size, MIME, and
  real-signature validation.
- Page navigation, window close, account/page revision changes, source changes,
  and modal close invalidate cached media and cancel active transfers.

## Deterministic verification

| Check | Result |
|---|---|
| `pnpm deps:check` | Passed; pnpm-only root remains clean |
| `pnpm audit:prod` | Passed; no known production vulnerability |
| `pnpm lint` | Passed |
| `pnpm typecheck` | Passed |
| `pnpm test` | 587 files, 7052 tests passed on the finalized wallpaper source dialog |
| Source-dialog coverage in the full suite | Compact album status, single-row responsive navigation, HMR boundary, load-more card interaction, and stacked Escape ownership passed |
| Targeted Saved album Vitest | 13 files, 68 tests passed; lifecycle, initial sign-in confirmation, old and current Cloudflare challenge markup, background warming and recovery, load-more state, thumbnail invalidation, gallery, HMR, viewer identity/concurrency, and media preparation paths are covered |
| Targeted Saved album Rust tests | 16 passed after manifest embedding; page lifecycle, navigation, metadata validation, bridge limits, media races, and cancellation are covered |
| Targeted WebView proxy Rust tests | 3 passed after manifest embedding; manual pin, SOCKS normalization, native system route, credential rejection, unsupported scheme, and Direct fail-closed are covered |
| `pnpm build:ui` | Passed; only existing dynamic-import and large-chunk warnings |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --all-targets -- -D warnings` | Passed |
| `cargo test --no-run` | Passed |
| Manifest-embedded Windows Rust harness | 1712 passed, 0 failed, 1 ignored |
| `python scripts/check-code-quality-gates.py` | Passed; `FILES_OVER_1K_BUDGET` is 77/77 |
| `git diff --check` | Passed after the first-snapshot UX fix |

No production file introduced by the Saved album or source-dialog cleanup
crosses the 1,000-line threshold. The Host is split between a 911-line facade
and a 181-line cache module; the source modal is 920 lines, and the shared
viewer is 381 lines after the lazy-media, HMR, and stacked-layer hardening.

## Real Windows Tauri acceptance

The development application was started with `pnpm dev`. Evidence below
contains only states, counts, and behavior. It excludes account details,
challenge identifiers, media URLs, cookies, tokens, raw responses, and network
exit information.

| Scenario | Result | Evidence |
|---|---|---|
| Main application and dedicated album window coexist | Passed | Unique `Grok` and `Grok album` windows were observed under the development executable |
| Manual proxy reaches the dedicated WebView | Passed | The album WebView2 environment carried the expected sanitized loopback `--proxy-server` endpoint; seven related processes were found and none carried a different proxy endpoint |
| Official Cloudflare challenge | Passed | Isolated window displayed the official challenge; automation did not interact with it |
| Main-window challenge classification | Passed | After rebuilding with the current challenge-resource fallback, the source panel changed from the initial loading state to the localized verification state and `View in Grok` action while the isolated window showed the newer official challenge layout |
| First snapshot transition | Passed | An initial `sign_in` snapshot remains loading until the next automatic poll confirms sign-in or classifies verification; a later `ready` to `sign_in` transition still clears account media immediately |
| Stable layout during challenge polling | Passed | Existing modal content stayed mounted with no blank WebView or repeated shimmer |
| Persistent isolated sign-in | Passed | After a full development-app restart, the dedicated WebView reused its isolated profile and opened Saved without another login |
| Fast Refresh with the Saved modal mounted | Passed | After a cold reload discarded a stale pre-fix module graph, editing the shared viewer while the modal stayed open preserved the stable Context boundary; the next card click mounted Lightbox and played the real Saved video |
| Minimum-width source layout | Passed | At the app minimum of 886 x 948, all seven labeled sources stayed in one row without overlap; the strip retains horizontal overflow for narrower viewports, and the ready album toolbar showed 20 visible / 48 cached items without repeated explanatory copy |
| Stacked Escape ownership | Passed | Opening a real album card mounted Lightbox; Escape closed only Lightbox and left the wallpaper dialog open for the next action |

## Ready-state media matrix

| Scenario | Status | Evidence |
|---|---|---|
| Reuse the isolated signed-in session after closing and reopening the source | Passed | A full app restart reused the isolated login and returned directly to Saved |
| Sync the current Saved page and display the first 20 items | Passed | The first view showed 20 items while 45 validated metadata entries were ready |
| Warm exactly the next 20 items without revealing or reordering them | Passed | The thumbnail warmer targeted only the visible 20 plus one 20-item lookahead; the gallery stayed at 20 |
| Reveal the warm page immediately, then warm one further page | Passed | `Load more` exposed 40 immediately and background metadata later advanced to 72 |
| Preserve official-page scroll position during background prefetch | Passed | Background scrolling restored the official page to its prior position before yielding |
| Preview and apply a generated image | Passed | Image original download, lightbox preview, and wallpaper application succeeded; a cached repeat prepared in about 9 ms |
| Preserve the clicked gallery index | Passed | After the final viewer fix, clicking the sixth visible card opened `Slide 6 of 20` with that card's corresponding original; deterministic coverage additionally exercises two distinct lazy cards that share the same placeholder URL |
| Preview and apply a normal generated video | Passed | The available video downloaded as a real 15.04 s H.264/AAC MP4 at 1152 x 1728 (about 15.8 MB), looped in the lightbox, and applied as the dynamic background |
| Preview and apply a blob-backed generated video | Not observed | No blob-backed video existed in the live account sample; deterministic bounded-chunk and cleanup coverage passed |
| Cancel thumbnail/original work when closing the modal or changing source | Passed | Cancellation/cleanup tests passed and the live wallpaper directory contained no residual `.partial` file |
| Clear cached account media on navigation, logout, window close, and page revision | Passed | Lifecycle invalidation tests passed; live close/reopen and full-restart flows did not expose stale gallery state |
| Keep existing cards interactive during paging and warming | Passed | Existing album and remote-source cards remained selectable while later pages were being prefetched or loaded; the focused real-Lightbox regression keeps this behavior locked |

## Finalization outcome

The complete frontend and Rust gates, manifest-injected Windows harness,
code-quality audit, diff check, and secret/temporary-marker scan were rerun on
2026-09-02. Every deterministic gate is green, including the repository-wide
`FILES_OVER_1K_BUDGET` limit at 77/77. This scope adds no new 1,000-line
production file. This acceptance remains local-only: no pull request, tag,
release, or push is part of the work. Retest the blob-backed video path if a
real Saved sample becomes available; absence of that sample does not change the
supported validation and cleanup contract.
