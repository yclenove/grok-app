# Pexels wallpaper request cache isolation

Date: 2026-09-02

## Scope

This pass closes a correctness gap in the direct Pexels wallpaper source and
rechecks the compact wallpaper-source dialog. It does not change the X, Web,
Openverse, Imagine, Grok album, or local-library contracts.

## Live upstream evidence

A header-only request to the fixed official Pexels search endpoint, without an
Authorization header, returned an intermediary-cached `200 OK` with:

- `Cache-Control: public, max-age=3600`;
- `Age: 116572`;
- `Vary: Origin`.

The same fixed request shape with a fresh, credential-free query nonce returned
the expected `401 Unauthorized`. A second fresh nonce plus an intentionally
invalid Authorization value also returned `401 Unauthorized`. Both nonce runs
reported `Cache-Control: no-cache` and completed without reading or retaining a
response body.

This demonstrates that an upstream or intermediary cache can replay an old
success across authorization contexts when the URL is reused. It is not an
application-process cache collision: the Host cache already includes a
SHA-256 credential revision and never stores the API key itself.

## Permanent mitigation

- Pexels now uses its documented `query` parameter; Openverse continues to use
  its own documented `q` parameter.
- Every real Pexels request gets a fresh 48-character hexadecimal
  `_grokapp_cache_bust` value. It consists only of a per-process random UUID
  prefix and an atomic request sequence, so concurrent requests cannot reuse a
  URL and restarts do not restart the same namespace.
- The nonce generator has no credential or query input. The nonce is not logged,
  returned over IPC, included in the Host cache key, or used as paging state.
- Missing or malformed Pexels nonces fail closed before network I/O.
- The direct-library cache contract is version 3, preventing reuse of earlier
  entries after the request and parameter semantics changed.
- URL construction lives in a dedicated provider request module so the main
  provider adapter remains below the repository's 1000-line file budget.

## Verification

| Check | Result |
|---|---|
| Header-only official endpoint probe | Cached canonical URL reproduced `200`; two fresh nonce URLs returned `401` |
| Focused provider harness | 8 passed, 0 failed |
| Manifest-embedded Windows Rust harness | 1713 passed, 0 failed, 1 ignored |
| Frontend suite | 587 files, 7052 tests passed |
| ESLint | Passed with zero warnings |
| TypeScript typecheck | Passed |
| Production UI build | Passed; existing Rollup chunk warnings remain non-fatal |
| `cargo fmt --check` | Passed |
| Strict Clippy | Passed with `-D warnings` |
| Code-quality final gate | Passed, including the 77-file large-file budget |

## Real-device UI check

The development Windows app showed all seven labeled sources in one compact
row above a single full-width workspace. The X source had no persistent
explanation or footer teaching copy. The Pexels missing-key state showed only
the required status and key action. No grouped source frames, repeated source
descriptions, wrapping, clipping, or modal stacking issue was visible.

## Remaining live acceptance boundary

No valid Pexels key is configured on this machine. This pass therefore does not
claim a successful authenticated Pexels search, paging/prefetch, attribution
link, preview/apply, key-recovery, or source-switch cancellation run. Those
checks require the user to save a valid key through the in-app Pexels control;
the key must not be pasted into chat or added to repository fixtures.
