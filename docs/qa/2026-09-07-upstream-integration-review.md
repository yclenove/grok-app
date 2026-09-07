# Wallpaper review and upstream integration

Date: 2026-09-07. Status: review, upstream integration, full automated regression
and representative Windows runtime checks completed; limitations below. No push
or PR.

## Scope and baseline

Review the current wallpaper changes, merge the latest source project's main
branch, resolve integration conflicts, run the full repository regression, and
exercise the resulting Windows desktop build. The separate wallpaper module
optimization plan still contains future work; this report does not close it.

Starting branch: `fix/wallpaper-responses-network-observability`,
starting HEAD: `f7b88193`. Upstream fetched successfully from
`https://github.com/RongleCat/grok-app.git`; observed main: `7780d648`,
12 commits not yet in the starting branch.

## Review fixes

- Replacing a library file then refreshing its catalog previously revived the
  old remote-origin lookup and parent association. Replacements now receive a
  new persisted media ID; obsolete URL mappings are removed, and generation
  records use the actual source record ID. Unchanged legacy records retain IDs.
  Detection uses the existing file size/mtime contract; content hashing is a
  separate optimization-plan item.
- A pending library page captured old rows and could undo a favorite update
  when it completed or failed. Both paths now merge into the current entry.
- Closing media details now invalidates a parent preview still resolving its
  URL or image size, preventing the preview from opening after its owner closes.
  A regression test covers the not-yet-visible preview case.
- Reviewed details/parent navigation, attribution sanitization, separate prompt
  drafts, generation cancellation and audit boundaries, catalog persistence,
  source-history fencing, and original-preview request limits.

## Pre-merge evidence

- TypeScript and whole-frontend ESLint passed before the final review fixes.
- Seven focused frontend suites: 102 tests passed.
- After the review fixes: library, details, and source-modal suites: 33 passed,
  including pending-page success/failure preserving a favorite mutation.
- Current Windows Rust library harness compiled; Common Controls v6 manifest
  embedded using the repository CI procedure. Catalog regression: 7 passed,
  including replacement, refresh, old-origin rejection and new-parent lookup.
- Rust formatting, final code-quality gate and website self-tests passed.

## Merge and post-merge evidence

- Local implementation commit: `6633e1b8`; merge commit: `51fbdf64`.
- `upstream/main` at `7780d648` is an ancestor of the merge. No unmerged paths
  remain. Version is now 0.2.33.
- Two conflicts resolved: Cargo.lock keeps both `webpki-roots` and
  `webview2-com`; `win_file_drop.rs` retains the STGMEDIUM RAII release and
  path-validation changes while including upstream's OLE import correction.
- Whole frontend regression: **622 files, 7357 tests passed**, zero failures.
- TypeScript, whole-frontend ESLint, dependency hygiene, final code-quality
  gate, website publishing self-tests (3), and UI production build passed.
- `cargo clippy --all-targets -- -D warnings` passed.
- Production dependency audit completed: no known vulnerabilities.
- Vite reports existing large output chunks. Imported upstream Markdown uses
  intentional two-space line breaks flagged by raw `git diff --check`; source
  code has no whitespace errors. These are not suppressed test failures.

- Full current Windows harness execution completed: **1829 passed, 1 ignored,
  0 failed**; the binary harness also passed (0 tests). The ignored case is the
  explicit golden-fixture regeneration helper. All harnesses exited zero.
  A previously interrupted execution without a summary was discarded.
- Desktop build at 20:32 completed with exit zero. A later details-lifecycle
  fix requires the final frontend rerun and rebuilt UI before acceptance.

## Latest upstream baseline

A second fetch at approximately 20:35 found two newer upstream commits. The
acceptance baseline is pinned to **`3bee4a884dd1a3150af457ce477fd4692be19822`**
(startup/vendor splitting and Review/context-window UI fixes).

- Merge commit: `d1d649bf`; this second merge had no conflicts.
- `pnpm install --frozen-lockfile` passed after upstream removed dependencies.
- Manual integration check confirms Review still uses this branch's exact-path
  matching helper and now also uses upstream's container-only scrolling.
- Native sources and Cargo manifests have no content differences from the
  already-tested native baseline. The 1829-test Windows result covered that
  merge's native source. Later transport-error changes were independently
  covered by the 1830-test run below.
- The intermediate parent-preview fix passed a whole frontend rerun of 7358
  tests and a successful desktop build. These intermediate results are not
  substituted for the new upstream frontend checks.

- Latest-baseline frontend regression: **622 files, 7362 tests passed**, zero
  failures; runner exit zero. Whole-frontend ESLint, final quality gate
  (78 files at or above 1000 lines), website self-tests and dependency audit
  passed. Desktop embedding completed successfully at 20:39:58; the executable
  was 77,770,752 bytes. This was the build used for the runtime checks below.

## Desktop runtime evidence and follow-up

The first launcher attempt opened the installed application. It was closed
before testing. The actual tested process was verified by its executable path
under this checkout's `src-tauri/target/debug`, launched at 20:44:17.

- Main workbench, Settings > Appearance and the separate theme editor opened
  successfully. The theme editor remained responsive; its earlier startup hang
  did not recur. Main window: 1143 x 793; theme editor: 842 x 602.
- The library initially contained 111 items (91 images, 20 videos). Filtering
  located a Pexels AVIF landscape. Its preview and image-to-video handoff worked.
- Selecting an image filled an editable localized video prompt immediately.
  Editing it, switching to image mode and returning retained the edited video
  draft. Selecting another source refreshed the prefill; remote captions were
  not inserted. Reusing a recorded prompt filled the image draft without
  starting generation or overwriting the video draft.
- One AVIF-to-video request completed in approximately 182 seconds. The audited
  session contained exactly one `image_to_video` call, with the edited prompt
  verbatim, duration 6 and resolution_name 480p. The resulting H.264 MP4 was
  3,129,445 bytes, 672 x 448, 24 fps and 6.041667 seconds (ffprobe). These are
  observed dimensions, separate from the provider's requested quality tier.
- The video visibly played in the lightbox. Escape closed only the lightbox.
  Details showed the saved prompt and requested options; parent navigation
  resolved the original AVIF, opened its preview and returned to the work.
  The transient PNG was removed after success.
- Favoriting the new video persisted across the Imagine/library roundtrip.
  The library then contained 112 items (91 images, 21 videos), with the new
  video first in the video filter and its favorite retained.
- Two subsequent UI-triggered requests failed at the tool's HTTP transport
  layer while sending to the video service. Their audited calls preserved the
  prompt and options. The input, source and earlier successful result survived;
  no failed result was added, and temporary output directories were removed.
  A planned cancellation click raced an already-finished failure and started
  the second request; this is not evidence of an automatic retry or a successful
  cancellation test. Fresh final-build cancellation acceptance remains open.

The transport failure was previously displayed as an unknown generation error.
The follow-up fix recognizes only the expected tool's request-failure wrapper
plus reqwest's transport prefix, after the existing complete session audit.
It returns `imagine_network_failed` without exposing URLs or nested diagnostics.
The same verified wrappers cover image generation, editing, video generation
and video polling. All 15 locales now give a network/proxy message and preserve
manual retry. No proxy, credential or generation routing settings were changed.

Post-fix verification: frontend **622 files / 7366 tests passed**; complete
ESLint passed; native **1830 passed / 1 ignored / 0 failed**, including strict
transport classification and invalid-audit cases; Clippy with warnings denied
and the final code-quality gate passed. The final desktop rebuild completed
with exit zero, including TypeScript and production frontend assets. Only the
existing Vite size advisory and MSVC import-library linker output remained.
The final executable was built at 21:37:43 (77,771,264 bytes). It was restarted
at 21:38:49 from the verified checkout path; the workbench rendered normally.

## Image-generation progress compatibility

An ordinary image-generation smoke test produced a valid JPEG in the current
CLI session but the Host rejected it as `imagine_result_invalid`. The initial
`image_gen` input contained prompt and aspect ratio; the progress update added
`variant: "ImageGen"`. Whole-JSON equality incorrectly rejected that known CLI
serialization marker. The fix accepts the exact original object or that one
additional known tag, while requiring every requested field unchanged. Wrong
tags, missing/changed fields and extra fields remain rejected.

The regression exercises a complete initial/progress/completed audit, including
the invalid variants above. All current Windows Rust harnesses passed:
**1831 passed, 1 ignored, 0 failed**. Clippy with warnings denied and Rust
formatting passed. The frontend is unchanged from the 7366-test result above.
The desktop rebuild passed (22:00:18, 77,710,336 bytes), including TypeScript
and production frontend assets. PID 16072 was launched at 22:00:50 from this
checkout. The final code-quality gate passed (78 files at or above 1000 lines).

The ordinary-image retest succeeded with the same previously rejected request.
Its audit contains exactly one `image_gen` call, the known tagged progress input,
and completion, with the exact prompt and 16:9 ratio. The Host saved a valid
1280 x 720 JPEG (224,875 bytes), approximately 23 seconds after submission.
The result appeared in Imagine and persisted near the top of the library.
Selecting its video action immediately populated the original scene prompt
plus the localized editable motion template, without submitting generation.

A cancellation test using the shared generate/cancel button again raced a
completed request and started another request. This produced one additional
valid image (225,877 bytes); it was not an automatic retry. Closing the modal
while the following request was busy removed its output directory, left no
catalog entry and showed no late result on reopening. The CLI session retains
its own completed image, so this proves Host cleanup/late-result rejection,
not prevention of upstream generation or a measured two-second process stop.
The generate/cancel action changing purpose at completion remains a UX edge
case for delayed input; these earlier attempts do not measure cancellation.

A separate final test submitted a new request at 22:30:00 and clicked Cancel
immediately after observing the busy state. The next snapshot, 733 ms after
the click, showed the editable prompt and Generate action restored, with no
result. The subsequent process/filesystem check found the output directory
removed and no matching generation process; the pre-existing unrelated CLI
process remained running. This accepts immediate cancellation for this sample,
without claiming that an upstream request can always be recalled after dispatch.

The resulting library showed 115 entries (94 images, 21 videos). Pexels search
for `mountain lake` returned 20 validated images in 15.2 seconds. Clicking
Load more expanded the prefetched next page immediately to 32 images; opening
an existing item resolved the original file and showed slide 20 of 32.
Escape returned to the search results. This run does not prove clicking while
a network pagination request is still pending; that path has automated coverage.

The preceding image-edit smoke test also completed successfully: one audited
`image_edit` call transformed the Pexels AVIF landscape into a 1280 x 720 sunset
JPEG (292,555 bytes). The tool completed approximately 38 seconds after the UI
submission; Host success was separately confirmed by the result card and media
details. Details and the persisted catalog retain the exact editable prompt,
`image_edit`, requested 16:9 and the source media ID. Only `result.jpg` remains
in the output directory; the temporary PNG was removed.

The new ordinary-image result opened at full size from the persistent library
and was explicitly applied as the wallpaper. Both the Appearance preview and
the main workbench displayed it correctly. Before this check, the current
aurora video and appearance values were exported to a local temporary
`.grokskin` recovery file. The archive contained the original 1102 x 768 MP4,
default centered focus, mist skin and 36% scrim. Importing and applying that
recovery file restored the aurora video without adding a preset. No network,
account or authentication settings were modified. The usual before-last-apply
snapshot now represents the generated-image test appearance.

## Remaining acceptance boundary

This report does not claim the entire wallpaper optimization plan is complete.
Remote-original failure/retry and the exhaustive seven-source/theme/size matrix
remain outside this runtime pass. Ordinary image generation, image editing,
image-to-video, immediate cancellation, successful remote-original loading,
late-result rejection and
application/restoration of a newly generated background were exercised as
described above. Automated full-regression coverage does not substitute for
that remaining exhaustive manual matrix.
Generated-video dimensions/duration are externally measured here but are not
yet populated automatically in media details. Existing large single-result
cards require scrolling to their bottom actions. Recoverable deletion, batch
management, broader filtering/deduplication and the 500-item performance work
remain in the separate optimization plan. No push or PR was performed.
