# Wallpaper Responses output-count / latency calibration

Date: 2026-08-29 (Asia/Shanghai)

Branch: `fix/wallpaper-responses-network-observability`

Baseline commit: `29bfc6e2`

Scope: read-only live calibration of the Grok Build Responses wallpaper-search candidate contract. No production route, setting, fallback, model, or default was changed.

## Decision

The single-request sweep first found this balance:

- one hosted `x_search` call;
- a target of 12 distinct items when possible;
- the existing `grok-4.6` + `low`, strict schema, source citation, and local image-validation rules.

The later concurrency and per-lane target sweeps superseded that single-request candidate for interactive use. The final tested recommendation is three complementary Responses lanes in parallel, each targeting eight items, with progressive delivery as each validated lane completes. The observed `3 × 8` profile moved a useful first batch earlier than `3 × 12` while keeping a 19-image median; a later one-lane/eight-item request should be explicit enrichment, not an automatic fourth lane.

This report does not approve exposing `auto` or changing the stable CLI default. The `3 × 8` matrix still had one correlated all-lane connection failure, so a product patch must keep CLI constants independent, preserve the existing bounded aggregate fallback, and repeat the normal deterministic and Tauri lifecycle gates.

## Method

The sanitized repository benchmark gained an optional `--target-count 4..16` argument for equal-output experiments. When present, it:

- asks for exactly the target count when possible, allowing fewer only when candidates cannot be confirmed;
- caps the schema array with `maxItems`;
- rejects a response when observed `x_search` calls exceed the declared budget, matching the product safety rule;
- retains the existing online image probes and only prints topic ids, stable error codes, counts, timings, model/effort, and token totals.

All scored runs used the fixed Build endpoint, `grok-4.6`, effort `low`, the 10809 residential proxy route, and image probes. Requests ran serially. No token, Authorization header, auth contents, raw response, query text, media URL, username, account data, exit IP, or node tag was retained.

The decision order was:

1. reject tool-budget overruns and malformed output;
2. require at least six validated images for a useful gallery;
3. compare useful-gallery rate;
4. compare successful-sample p50/p95 total latency;
5. compare median validated images and milliseconds per delivered image.

Percentiles use the repository benchmark's nearest-rank method. `p50`, `p95`, median images, and successful milliseconds per image include successful rows only. `all-attempt seconds / delivered image` also charges failed-attempt time against images actually delivered by that profile. For tool-budget failures, that metric stops at the recorded search time and does not charge an image probe the product would never run.

## Scored result

| First-round profile | Attempts | Successful result | At least 6 valid | p50 | p95 | Median valid | Valid / candidates | Successful seconds / valid image | All-attempt seconds / delivered image |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 call, target 8 | 4 | 3/4 | 2/4 (50.0%) | 45.2 s | 55.7 s | 7 | 17/17 (100%) | 8.1 s | 9.3 s |
| 1 call, target 10 | 4 | 1/4 | 1/4 (25.0%) | 74.8 s | 74.8 s | 10 | 10/10 (100%) | 7.5 s | 12.5 s |
| **1 call, target 12** | **8** | **6/8** | **6/8 (75.0%)** | **69.0 s** | **78.6 s** | **12** | **70/72 (97.2%)** | **5.9 s** | **6.3 s** |
| 1 call, target 16 | 8 | 5/8 | 5/8 (62.5%) | 69.6 s | 89.0 s | 13 | 58/61 (95.1%) | 5.9 s | 7.9 s |
| 2 calls, target 8 | 4 | 2/4 | 2/4 (50.0%) | 68.7 s | 81.2 s | 8 | 16/16 (100%) | 9.4 s | 17.7 s |

“Successful result” follows the benchmark's at-least-one-valid-image rule. The stricter “at least 6 valid” column is the product-usefulness measure and drives the decision. The target-10 sample had two transport failures in one time window, so it is not evidence that the number 10 itself causes network failure. It nevertheless could not meet the quality gate in this sample and did not justify expansion.

### Target-12 rows

| Topic id | Result | Total | Valid / candidates |
|---|---|---:|---:|
| `ocean-ultrawide` | success | 58.1 s | 12/12 |
| `cyberpunk-vertical-zh` | structured-output parse failure | 22.1 s | 0/0 |
| `abstract-ai-prompt` | success | 60.9 s | 12/12 |
| `macro-flower-vertical` | success | 78.6 s | 12/12 |
| `misty-mountain` | success | 71.2 s | 12/12 |
| `space-nebula` | connect timeout | 10.7 s | 0/0 |
| `ink-landscape-zh` | success | 69.0 s | 12/12 |
| `rainy-anime-street` | success | 73.2 s | 10/12 |

Across successful target-12 rows, all 72 candidates were unique and carried canonical X status citations. Seventy passed the bounded image probe. The successful search-stage p50 was 58.9 seconds and probe-stage p50 was 5.0 seconds, confirming that hosted model/search work remained the dominant latency.

## Interpretation

### Why 8 is fast but not the best experience

Target 8 had a 45.2-second p50, but one completed row returned only two valid images and only 2/4 attempts reached the six-image usefulness floor. It saved about 23.8 seconds at p50 versus target 12 while giving a median of five fewer images. Its successful seconds per valid image were also worse (8.1 versus 5.9).

### Why 16 is beyond the useful knee

Target 16 produced only one additional median valid image over target 12 (13 versus 12). Its p50 was effectively unchanged, but p95 grew by about 10.5 seconds, the useful-gallery rate fell from 75.0% to 62.5%, and one response exceeded the one-call tool budget. Several successful rows returned only 7, 9, or 13 candidates despite the higher target. The larger target therefore increased tail cost without reliably increasing choice depth.

### Why a second first-round search is not worthwhile

Two calls targeting 8 had nearly the same p50 as one call targeting 12, but delivered a median of only eight images and reached the usefulness floor in only 2/4 attempts. One row returned eight unreachable links and another executed six observed search calls despite the declared two-call limit. Counting failed time, it cost 17.7 seconds per delivered image, versus 6.3 seconds for the target-12 profile.

The fixed overhead is dominated by hosted model/search work. Asking one useful search result to expose more attached media amortizes that overhead better than asking the model to perform another search loop.

## Infrastructure event and excluded rows

During the exploratory matrix, the local Xray process restarted at 00:25:06. One response completed its hosted search just before the restart, then all 13 image probes failed; immediately following attempts received local `connection_refused` in 11–14 milliseconds. Those rows were classified as infrastructure interruption and excluded from the scored table.

The target-12 ocean row had already completed with 12/12 validated images before the restart and was retained. After recovery, every subsequent scored command checked the listener's owning PID before and after the sample; the PID stayed stable for all of them. Normal `connect_timeout` rows with a stable local PID remain counted as real end-to-end transport failures. No proxy process was manually restarted and no user proxy setting was changed.

An early calibration used “up to N” wording. It encouraged valid early stopping and could not fairly compare output targets, so those rows were retained only as exploratory evidence and excluded from the formal ranking. The scored matrix used “return N when possible; fewer only when candidates cannot be confirmed.”

## Single-request recommendation at this stage

Before the multi-lane experiments below, the single-request evidence supported:

1. Give Responses its own first-round constants instead of changing the constants shared with the stable CLI route.
2. Try one `x_search` call and request 12 distinct items in the first round.
3. Keep the existing strict budget verification, URL normalization, image signature/MIME/dimension probes, dedupe, ranking, citation honesty, cancellation, cache, and no-redirect credential policy.
4. Keep CLI as the default and `auto` hidden.
5. Do not automatically race Responses and CLI.
6. Decide separately whether 1–5 valid first-round images should trigger the existing automatic supplement or return immediately with an explicit “find more” action. The latter improves perceived latency but is a product UX change and was not implemented here.

That recommendation was an intermediate result. The expanded matrices below selected progressive `3 × 8` instead. Any production change still requires real Host validation, no tool-budget double spend, p50/p95 split timings, cancellation under two seconds, no late-result pollution, and all deterministic security/privacy gates.

## Exploratory two-batch scheduling preflight

After the single-batch calibration, the benchmark gained an explicit multi-batch mode to test a larger gallery without raising the per-response target beyond the observed 12-item knee. The initial preflight used two batches. Multi-batch mode is Responses-only, supports two to four complementary search lanes, and provides:

- `serial`: start batch 2 only after batch 1 and its probes finish;
- `parallel`: start both batches together;
- `staggered`: start batch 2 after a bounded delay (10 seconds in this preflight).

The combined row reports the first time at least one validated image is available, the first time six unique validated images are available, final wall time, unique validated images, cross-batch duplicates, and sanitized per-batch outcomes. Media identities used for cross-batch dedupe stay in process memory and are never serialized.

This was a deliberately small preflight: two topics, each strategy once, with strategy order rotated between topics. It is enough to reject unsafe assumptions, not enough to select a production default.

| Strategy | Topics reaching 6 valid | Topics reaching 18 valid | First useful gallery | Final wall time | Final unique valid | Batch success |
|---|---:|---:|---:|---:|---:|---:|
| Serial | 2/2 | 1/2 | 51.7 s / 55.3 s | 52.9 s / 118.4 s | 9 / 19 | 3/4 |
| Parallel | 1/2 | 0/2 | 54.0 s / not reached | 66.9 s / 79.2 s | 15 / 5 | 4/4 |
| Staggered 10 s | 2/2 | 1/2 | 90.4 s / 83.9 s | 91.2 s / 83.9 s | 22 / 14 | 4/4 |

The serial failure was a fast second-batch network error after a useful nine-image first batch. The local 10809 listener remained present and its owning process had been stable for about 75 minutes, so the row was retained as a real transport failure rather than excluded as another proxy restart.

The parallel ocean row exposed the most important UX trap: one validated image appeared at 23.9 seconds, but the combined result never reached the six-image usefulness floor and finished with only five validated images. “Time to first image” therefore cannot be presented as search completion. Conversely, serial ocean reached seven validated images at 55.3 seconds and accumulated 19 at 118.4 seconds, demonstrating why batch 2 can be useful as non-blocking enrichment but should not hold the initial gallery open.

The 10-second stagger avoided a total empty/usefulness failure in these two rows and produced 14 or 22 unique images, but it did not improve first-useful latency: both useful galleries arrived only after 83.9–90.4 seconds. Parallel and staggered requests were accepted without 429 or declared tool-budget overrun, but returned quantity and image reachability varied too much to justify automatic dual-request rollout.

### Expanded parallel matrix: score time and delivered images

A subsequent full-topic matrix tested the user's primary product question directly: start two 12-item batches together and score total wall time plus final unique validated images. Each request declared up to three `x_search` calls. The benchmark's `--allow-tool-overrun` switch kept counting an output even if the observed call count exceeded that declaration; call count remained diagnostic data and did not reject an otherwise usable gallery.

| Topic id | First 6 valid | Final wall time | Final unique valid | Cross-batch duplicates | Batch outcome |
|---|---:|---:|---:|---:|---:|
| `misty-mountain` | not reached | 90.0 s | 0 | 0 | 0/2 |
| `cyberpunk-vertical-zh` | 74.7 s | 75.6 s | 23 | 0 | 2/2 |
| `ocean-ultrawide` | 78.0 s | 94.7 s | 13 | 0 | 2/2 |
| `ink-landscape-zh` | 72.5 s | 72.5 s | 6 | 2 | 2/2 |
| `space-nebula` | 53.4 s | 64.7 s | 22 | 2 | 2/2 |
| `abstract-ai-prompt` | 94.2 s | 94.2 s | 17 | 0 | 2/2 |
| `macro-flower-vertical` | 54.0 s | 79.3 s | 21 | 3 | 2/2 |
| `rainy-anime-street` | 65.2 s | 90.0 s | 11 | 0 | 1/2 |

Aggregate result:

- 7/8 topics (87.5%) reached at least six unique validated images;
- 3/8 topics (37.5%) reached at least 18; none returned a full 24 after validation and dedupe;
- total wall-time p50 was 79.3 seconds and p95 was 94.7 seconds;
- time-to-six p50 was 72.5 seconds and p95 was 94.2 seconds among the seven useful topics;
- median final output was 13 unique images across all attempts, or 17 among non-empty attempts;
- 120/128 candidates passed the image probe before cross-batch dedupe; 113 unique images were delivered;
- seven cross-batch duplicates were removed; every candidate retained a canonical X status citation;
- 13/16 batches returned usable images; one full topic lost both batches, and another useful topic had one batch time out;
- no 429 was observed. One batch emitted more observed searches than declared, but its images remained scored in this performance-only matrix.

The scored matrix initially encountered a local Xray restart near the final two topics: one in-flight branch ended with a network error and the next two requests received immediate `connection_refused`. Those contaminated rows were excluded. Both topics were repeated after recovery while comparing the 10809 listener owner before and after each run; the listener stayed stable throughout both replacements. The earlier full-topic failure occurred while the prior listener process remained stable and therefore remains a real end-to-end failure.

Compared with the one-call/target-12 profile, parallel two-batch search raised the at-least-six rate from 75.0% to 87.5% and the successful-output median from 12 to 17 images, while increasing total p50 from 69.0 to 79.3 seconds and p95 from 78.6 to 94.7 seconds. It is therefore technically viable as an experimental high-volume mode, but it is not yet a reliable “24 images” contract. Progressive display remains important because waiting for both branches can add 10–25 seconds after one branch has already produced a useful gallery.

### Three- and four-lane expansion

The benchmark was then generalized with `--batch-count 2..4`. The additional lanes received explicit, non-overlapping roles: direct interpretation, visual variation, discovery/bilingual framing, and long-tail related scenes. All lanes still targeted 12 candidates, began together in the parallel profile, allowed up to three declared searches, and scored outputs even when the observed diagnostic call count exceeded that declaration.

These are cross-window exploratory comparisons rather than randomized same-window A/B trials, so small latency or count differences must not be treated as causal. The large reliability and output-distribution differences are nevertheless sufficient to locate the current concurrency knee.

| Parallel profile | Topics ≥6 | Topics ≥18 | Topics ≥24 | Topics ≥30 | Total p50 / p95 | Median unique valid | Batch success |
|---|---:|---:|---:|---:|---:|---:|---:|
| 2 × 12 | 7/8 (87.5%) | 3/8 (37.5%) | 0/8 | 0/8 | 79.3 / 94.7 s | 13 overall, 17 non-empty | 13/16 |
| **3 × 12** | **8/8 (100%)** | **6/8 (75.0%)** | **4/8 (50.0%)** | **2/8 (25.0%)** | **88.0 / 93.2 s** | **21** | **22/24** |
| 4 × 12 | 6/8 (75.0%) | 3/8 (37.5%) | 3/8 (37.5%) | 3/8 (37.5%) | 90.0 / 94.9 s | 15 overall, 16 non-empty | 19/32 |

Three lanes delivered 187 unique validated images from 215 candidates across the eight topics. Time-to-six p50/p95 was 71.5/87.2 seconds. Final per-topic counts were 13, 15, 18, 21, 25, 28, 33, and 34. Twelve cross-lane duplicates were removed, all citations were canonical, no 429 occurred, and every topic remained useful even when one lane timed out.

Four lanes had a higher upside but a substantially worse failure tail. Stable runs delivered 35 or 41 unique images, and a repeat of one earlier failed topic delivered 44. However, the scored first attempt for two topics ended with all four connections timing out together in about 10.7 seconds while the local proxy listener remained stable. Another topic returned only six images after two lanes timed out. The formal matrix retains those first failures rather than replacing them with successful repeats: 148 unique validated images from 170 candidates, two entirely empty topics, and only 19/32 useful lanes. No 429 occurred.

The current concurrency knee is therefore three lanes. It improved useful-gallery reliability and large-gallery frequency without materially extending the 90-second request envelope. A fourth simultaneous lane is not suitable as a default: its occasional 35–44-image result does not compensate for correlated all-lane connection failures and a lower median. If further exploration is desired, the next justified profile is three lanes immediately plus a delayed fourth lane only when early progress remains below a volume target—not five simultaneous requests.

### Three-lane per-lane target sweep

The next experiment kept three parallel lanes, `grok-4.6`, low reasoning effort, the same fixed topic set and live image probing, while reducing the requested item count per lane. The target-6 and target-10 profiles were four-topic screening runs. Target 8 was expanded to all eight topics after it won the screen. The target-12 row is the preceding eight-topic matrix and is therefore a nearby cross-window reference, not a randomized causal comparison.

| Profile | Topics | Topics ≥6 | Topics ≥18 | Topics ≥24 | Final p50 / p95 | First-6 p50 / p95 | Median unique valid | Batch success |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 3 × 6 | 4 | 4/4 | 1/4 | 0/4 | 74.3 / 90.0 s | 63.9 / 77.2 s | 12 | 9/12 |
| **3 × 8** | **8** | **7/8** | **6/8** | **0/8** | **78.8 / 91.8 s** | **58.9 / 66.3 s** | **19** | **20/24** |
| 3 × 10 | 4 | 3/4 | 2/4 | 1/4 | 90.0 / 90.0 s | 76.7 / 77.0 s | 19 | 7/12 |
| 3 × 12 | 8 | 8/8 | 6/8 | 4/8 | 88.0 / 93.2 s | 71.5 / 87.2 s | 21 | 22/24 |

Latency percentiles continue to include successful topic rows only; the usefulness and batch-success columns retain all attempts. The scored target-8 matrix produced final counts of 0, 14, 18, 19, 19, 20, 21, and 22 unique validated images. Its empty topic was a correlated three-lane `connect_timeout` while the local proxy listener remained stable, so the failure remains scored. No 429 was observed.

Target 8 is the best current interactive-load candidate. Relative to target 12, it moved first-useful p50 earlier by 12.6 seconds (17.6%) and final p50 earlier by 9.2 seconds (10.5%), while giving up two median images and every 24-image result. Target 6 did not improve first-useful latency despite its smaller output ceiling and delivered only 12 median images. Target 10 was slower, had two terminal lane timeouts in three otherwise successful topics, and had two lanes exceed the declared tool budget. Smaller requested output is therefore not a linear latency control: hosted search and reasoning dominate, and eight items per lane is the observed knee for an initial gallery.

The first attempted target-6 screen was excluded before scoring because the benchmark shell had not enabled Node's environment-proxy support; all twelve direct connections timed out together in about 10.6 seconds. Both local proxy listeners and the fixed Responses host were healthy through the intended route. The scored rerun and every later profile used a process-scoped proxy configuration. This incident did not modify system proxy state or expose credentials, but it justifies adding an explicit benchmark preflight before future comparisons.

### Adjacent-window comparison with the current CLI

Immediately after the target-8 matrix, the current CLI contract ran all eight identical topics through the same intended route. This is a paired adjacent-window comparison rather than a fully interleaved A/B, so it is directional evidence and should not be treated as a permanent speed ratio.

| Route | Useful topics | Topics ≥18 | Topics ≥24 | Final p50 / p95 | First-6 p50 / p95 | Median unique valid |
|---|---:|---:|---:|---:|---:|---:|
| Current CLI | 8/8 | 8/8 | 5/8 | 103.0 / 125.5 s | final-only | 28 |
| Responses 3 × 8 | 7/8 | 6/8 | 0/8 | 78.8 / 91.8 s | 58.9 / 66.3 s | 19 |

Among successful samples, target-8 Responses reduced final p50 by 24.2 seconds (23.5%) and p95 by 33.7 seconds (26.9%) relative to this CLI window. Showing the first validated six-image batch instead of waiting for CLI completion would move p50 perceived availability earlier by 44.1 seconds (42.8%) and p95 earlier by 59.2 seconds (47.2%). Six of the seven successful same-topic Responses rows finished before their CLI counterpart; one was 15.3% slower.

The speed gain is not free output. CLI delivered 202 validated images across the eight topics and a median of 28; target-8 Responses delivered 133 and a median of 19. CLI also succeeded on the topic where all three Responses connections failed. The correct product interpretation is therefore an eight-item-per-lane progressive initial load, the existing observable fallback only when the aggregate has no usable result, and optional user-triggered enrichment—not a claim that Responses is unconditionally faster or higher quality than CLI.

### Interaction implication

The safest candidate is progressive delivery, not automatic request racing:

1. Return the first validated batch as soon as it reaches the UI; do not wait for a 24-image terminal result.
2. Treat one to five images as honest partial progress, not “done”; continue only after explicit or clearly signaled background enrichment.
3. Offer “load more” only after the initial lanes finish. Run one additional bounded lane on explicit user intent, then append and dedupe rather than replacing the gallery.
4. Keep the parallel strategy inside the manually selected Responses preview until repeated windows meet the rollout gate; do not expose `auto` from these data.
5. Pre-warm only local state: OAuth metadata, effective proxy health, a reusable HTTP client/connection pool, and fresh in-memory cache. Do not spend an authenticated `x_search` before explicit user intent merely to create the appearance of speed.

Connection/DNS/TLS warming may remove a small setup cost or fail fast on a broken route, but it cannot remove the observed 45–80 second hosted search stage. The material interaction gains come from preserving prior results, non-blocking progress, partial batch delivery, thumbnail-first loading, and background/scroll-triggered enrichment.

## Verification and workspace state

Passed for the benchmark change:

```text
node --check scripts/benchmark-wallpaper-x-search.mjs
node scripts/benchmark-wallpaper-x-search.mjs --help
git diff --check
```

The benchmark work itself did not modify production source. The pre-existing `src-tauri/Cargo.toml` line-ending-only status remains unstaged and is not part of this work. No proxy setting, changelog, tag, release artifact, push, or PR operation was part of the calibration.
