# Wallpaper X search stage 4 quality report

Date: 2026-08-28
Branch: `feat/wallpaper-responses-search`
Scope: shared normalize, validate, dedupe, and rank pipeline plus conditional supplement search

## Decision

Keep `cli` as the default and keep `responses_preview` manual-only. The shared safety and result-quality pipeline passed its deterministic checks, but the live Responses route did not meet the rollout gate in this sample:

- 7/8 Responses searches succeeded; 6/8 returned at least six validated images (75%, below the 80% target).
- Responses p50 was 63.4 s versus CLI p50 56.8 s in the same alternating run, so the current-window 30% speed target was not met.
- One Responses first round exceeded its two-call contract. The response was rejected, and the product router now explicitly avoids a CLI fallback for this error so an over-budget request cannot trigger a second subscription search.
- Every returned item in both routes passed HTTPS allowlisting, redirect revalidation, bounded streaming, image MIME/signature validation, and final deduplication. Every returned item in this run also carried a canonical X status citation.

The sample is intentionally small and network/model behavior is variable. It is sufficient to reject automatic routing, not to claim a stable long-term performance ranking.

## Live matrix

The matrix used the same eight fixed topic ids as the stage 0 baseline, alternating Responses and CLI per topic. Logs contained only ids, durations, counts, stable error codes, model effort, and tool-call counts; no query text, media URL, username, token, header, or raw response body was retained.

| Topic id | Responses | Responses valid/candidates | X calls | CLI | CLI valid/candidates |
|---|---:|---:|---:|---:|---:|
| `misty-mountain` | 63.4 s | 15/15 | 2 | 65.1 s | 15/15 |
| `cyberpunk-vertical-zh` | 67.3 s | 1/1 | 3 | 52.9 s | 16/16 |
| `ocean-ultrawide` | 71.8 s, budget error | 0/0 | rejected | 50.6 s | 14/15 |
| `ink-landscape-zh` | 63.4 s | 10/10 | 2 | 51.6 s | 9/9 |
| `space-nebula` | 96.7 s | 16/16 | 3 | 89.1 s | 5/7 |
| `abstract-ai-prompt` | 81.3 s | 12/12 | 2 | 56.8 s | 16/16 |
| `macro-flower-vertical` | 49.8 s | 7/7 | 2 | 58.6 s | 16/16 |
| `rainy-anime-street` | 61.3 s | 8/8 | 2 | 62.0 s | 12/12 |

## Aggregate

Percentiles use the repository benchmark's nearest-rank method and include successful rows only.

| Metric | Responses | CLI |
|---|---:|---:|
| Successes | 7/8 (87.5%) | 8/8 (100%) |
| At least 6 validated images | 6/8 (75.0%) | 7/8 (87.5%) |
| p50 | 63.4 s | 56.8 s |
| p95 | 96.7 s | 89.1 s |
| Median validated images | 10 | 14 |
| Validated/candidates on successful rows | 69/69 (100%) | 103/106 (97.2%) |
| Canonical citations among returned items | 69/69 (100%) | 103/103 (100%) |
| Duplicate items returned | 0 | 0 |

## Deterministic verification

- Shared pipeline tests cover HTTPS-only URLs, user-info/non-default-port rejection, per-hop redirect checks, CDN variant dedupe, status id plus media-index dedupe, real PNG signature/dimensions, MIME mismatch rejection, ranking, private evidence fields, and the six-result supplement threshold.
- Responses protocol tests cover the two-call first round, one-call supplement prompt, actual custom-tool counting, strict structured output, over-budget rejection, redirect credential isolation, and stable HTTP/transport error classification.
- Router tests cover CLI default, hidden `auto`, opt-in Responses, single fallback, circuit behavior, credential revision reset, and no CLI double-spend on 429 or a tool-budget overrun.

Final local verification after removing the live-only matrix harness:

- `rustfmt --check` passed for the three touched Rust source files, and `git diff --check` passed.
- `cargo test wallpaper_x --no-run` compiled successfully. After applying the repository's Windows test manifest to the generated harness, `wallpaper_source` passed 20/20, `wallpaper_x_search` passed 9/9, and `wallpaper_x_responses` passed 6/6.
- `pnpm test -- src/lib/wallpaperSource.test.ts src/lib/xEvidenceCitation.test.ts` passed 27/27, and `pnpm typecheck` passed.
- Repository-wide `cargo fmt --check` remains blocked by pre-existing formatting drift in `models_aux.rs`, `process_util.rs`, and `providers.rs`; none of those unrelated files was changed.

## Remaining interpretation

This stage validates technical image integrity and source evidence, not subjective aesthetics. The manual preview remains useful for experimentation, but `auto` should not be exposed until a later sample meets both the quality threshold and the 30% p50 speed gate.
