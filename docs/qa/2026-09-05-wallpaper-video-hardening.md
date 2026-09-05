# Wallpaper image-to-video hardening — September 5

Status: local implementation and automated regression complete. Initial Windows
video, search, and source-preparation acceptance passed. The resumed September 6
session found and fixed a missing media viewer in the standalone theme editor;
its focused regression passes, with desktop playback verification pending.
Saved now visibly shows media and additional pages after the user's manual
session, but its full preview/source-handoff acceptance remains open. No push or PR.

## Acceptance ledger

The implementation baseline is `ad53ae93`. The resumed session merged upstream
`ecd1d998` as `37e15a6a` and reran the full frontend suite and build gates. Rust
sources and dependencies are identical to the previously validated baseline;
the four Windows drop tests were also rerun successfully. The later September 6
script-scoping follow-up changes two bundled JavaScript files and has separate
focused coverage below; the earlier full-suite counts describe `37e15a6a`.

| Requirement | Evidence | Status |
| --- | --- | --- |
| Fix unsupported image-to-video inputs | Real AVIF-to-MP4 run below; bounded browser conversion and independent native source decoding; EXIF pixel regressions | Passed, with later source fixes covered by native tests |
| Immediate editable prompt without an extra model/network request | Synchronous `buildWallpaperVideoPrompt`, controller/editing tests, both template keys in all 15 catalogs, real edited-prompt generation below | Passed |
| Isolate remote captions from agent instructions | Only Imagine's original prompt is used as context; tests reject inherited copy from X, Web, Openverse, Pexels, Saved, and library sources | Passed |
| Restrict video execution and validate its output | Dedicated CLI flags, fresh session UUID, exact source/prompt/options audit, Host-owned result copy, containment/signature/size checks and rejection tests | Passed as a result-acceptance boundary; CLI compliance limitation remains below |
| Cancel and release temporary resources | Real running CLI cancellation below; latest delayed encoder/reader tests; late results cannot start generation | Passed; latest browser cancellation change has automated coverage |
| Merge latest fetched upstream and preserve local changes | `37e15a6a` merges `ecd1d998`; the OLE conflict keeps the validated `ReleaseStgMedium` implementation; no Rust changes relative to `2288f52f` | Passed for the recorded upstream snapshot |
| Complete automated regression and build gates | Resumed run: 607 frontend files / 7,210 tests, production build/typecheck, lint and quality gates; unchanged Rust baseline: 1,808 native tests, Clippy and fmt; four drop tests rerun | Passed; the single ignored native test regenerates a mock-stream fixture and is not an acceptance test |
| Real Windows generation, playback, background, source selection and search | Two MP4s and device observations below; 6s/480p and edited 10s/720p requests | Passed at the initial device-validation build; follow-up distinctions remain explicit below |
| Final Windows interaction after later code changes | Rebuilt isolated Host renders alongside the installed app without a blocking permission prompt; Appearance and source workspace open | In progress; Explorer drag gesture and final wallpaper/video smoke are not yet claimed |
| Fresh authenticated Grok Saved gallery, paging and video handoff | September 6 desktop observations showed 40 displayed / 55 cached, then 80 displayed / 98 cached after user interaction | Gallery and page growth observed; preview and video handoff remain pending |
| Review report and local commits without push/PR | This report and local merge `37e15a6a`; implementation worktree was clean after merge | Passed |

The complete objective remains open for the two pending device rows. Continue
the final wallpaper/video smoke and Explorer folder drop into the sidebar/file
drop into the composer using nonpersonal fixtures after the manual login step.
After the user completes the official Saved login/challenge, check gallery sync,
load more, preview and source handoff without exporting authentication state.
The completed automated suites do not need another run unless code changes or a
new failure gives a reason. No further background Saved polling is planned.

## Review findings and changes

- The standalone `ThemeEditorApp` did not mount `ImageViewerProvider`. Both
  wallpaper library and Imagine result cards consequently called the optional
  viewer's no-op fallback. The editor now provides the existing media lightbox
  with its current locale. Two entry-point regressions failed before the fix
  and passed afterward using the real image/video lightbox and close control.
  The three focused suites passed 19 tests; TypeScript, targeted ESLint,
  production UI build, and final code-quality gates passed. Existing bundle
  size and mixed-import advisories remain.
  Desktop inputs were paused when the UI tool detected concurrent user activity;
  these automated checks are not proof of native video decoding/playback.
- Gallery cards previously forced paged media into a 16:10 crop. Cards now use
  each item's validated width/height metadata and `contain`, preserving the
  source ratio for library and remote results. The layout guard and gallery
  tests pass. The current Grok Build image-to-video contract still exposes only
  480p/720p, has no image-to-video aspect parameter, and fixes the model to
  `grok-imagine-video-1.5`; the UI therefore does not advertise unsupported
  1080p, ratio, or model selectors.

- An AVIF original reached the CLI video tool unchanged and failed format detection.
  The browser now converts AVIF/WebP/GIF to bounded PNG pixels; Host independently
  decodes and snapshots all source images before invoking the CLI. No external
  image converter is required.
- JPEG EXIF orientation was lost when Host re-encoded the source as PNG. New
  regression tests first reproduced ignored mirroring and a rotated image keeping
  the wrong dimensions. Host now applies the decoder's orientation after bounded
  resizing and before stripping metadata, preserving the existing allocation
  limits and the original file. All eight EXIF orientations are covered.
- Source captions were being carried into a background agent prompt. Only an
  Imagine image's original prompt is retained; all other sources use a localized
  deterministic motion template. Prompt editing and separate image/video drafts
  remain available. ZWJ/ZWNJ survive text normalization.
- The shared runner allowed unrelated tools and asked the agent to copy files.
  A video-only CLI policy disables Web, subagents, and MCP meta-tools. Host audits
  exactly one completed tool call with the selected source and options, then copies
  the unique valid video from the known fresh session. Auditing rejects violating
  results; it does not undo upstream calls that a nonconforming CLI already made.
- Source PNGs are transient and excluded from the wallpaper library. Cancellation
  during browser conversion cannot start a late Host generation. Existing request
  IDs and sticky Host cancellation remain authoritative.
- Cancellation originally waited for pending PNG encoding or Base64 reads to
  return, leaving the video controls in the cancelling state. Delayed-callback
  tests reproduced both cases. These steps now settle on cancellation, release
  the object URL/canvas, and abort any active FileReader after detaching handlers.
  A late encoder callback is ignored; normal bounded PNG conversion still passes.
  The browser may finish its internal canvas encoding because `toBlob` has no
  abort API, but this no longer delays UI cancellation or starts a Host request.
- Production build review exposed a circular re-export warning in the existing
  Reliability Center import. It now imports the view assembler directly; the
  rebuilt bundle no longer reports that circular chunk warning.
- The later upstream Windows drop-target fix retained OLE media on drag-enter
  and used `DragFinish` on drop, ignoring delegated COM ownership. The local
  follow-up copies paths before releasing every acquired `STGMEDIUM` through
  `ReleaseStgMedium`, including invalid data. A missing final data object now
  clears hover and rejects the copy instead of reusing the previous drag paths.
  The upstream formatting and strict Clippy failures were also corrected.
- Manual login reached the xAI Accounts "completing sign in / verifying your
  device" page. Code inspection found that the album marker script also wrapped
  `history.pushState`/`replaceState` and installed observers on identity-provider
  pages. Nine regression cases reproduced unwanted instrumentation on foreign
  origins/subframes or signed-out recovery inside a subframe. The marker now
  runs only in the HTTPS `grok.com` top-level document; recovery also requires a
  top-level Saved page. All 18 script tests pass, including normal Grok route
  tracking. This removes unnecessary interference with login pages; the observed
  device-verification stall is not claimed fixed until manual login succeeds.

## Upstream synchronization

- The earlier September 5 upstream baseline was `fb863234`.
- A later refresh found `d794849f` (Windows Explorer drag-drop, #1017). It was
  merged locally as `13b594a4`. The only conflict was the Windows feature list
  in `Cargo.toml`; both the existing pipe support and upstream COM/OLE features
  were retained. This was the upstream snapshot used for the earlier checks below.
- The resumed session fetched `ecd1d998`, including the composer/selection hot-path,
  Mac Control+Return, and duplicate image-prompt bubble fixes. Local merge
  `37e15a6a` includes 19 changed frontend/documentation files. The only merge
  conflict was `DragEnter` in `win_file_drop.rs`: the current `read_paths` and
  COM-owned medium cleanup already preserve the upstream nonempty-path guard,
  so that implementation was retained. `git diff 2288f52f 37e15a6a -- src-tauri`
  is empty. No wallpaper modules were deleted.
- Work remains local. No push or PR is part of this task.

## Verification

- The later script-scoping follow-up passed 37 focused tests across the fixed
  scripts, album controller and status panel, plus TypeScript and targeted
  ESLint. The native debug Host rebuilt successfully with the updated bundled
  scripts and the same isolated app home. Fresh manual login remains required
  to determine whether the xAI device-verification wait is resolved.
- Resumed September 5-6 run on `37e15a6a`: 607 frontend files and 7,210 tests
  passed, with 0 failures (158.97 seconds). TypeScript, ESLint, production UI
  build and final code-quality gates passed. Four existing Windows drop-target
  tests also passed against the unchanged native implementation. The full Rust
  suite was not repeated because no Rust source/dependency changed in the merge.
- `pnpm dev` rebuilt the debug Host with `tauri.dev.conf.json` and the existing
  isolated QA app home. Its native window was selected by the exact local
  executable path, distinct from the running installed app. No security prompt
  blocked it. Appearance rendered the existing video preview at 902 x 928;
  the wallpaper source workspace and the independent Saved window opened.
  Saved is currently at the official human-verification boundary, not accepted
  as an authenticated gallery pass.
- Focused frontend: 6 files, 57 tests passed.
- The cancellation follow-up passed 31 tests across the browser converter, video
  API, controller, controls, and prompt helper. New regression checks cover
  cancellation before slow encoder/reader completion, late callbacks, temporary
  resource cleanup, and normal completion.
- TypeScript, ESLint, strict Clippy, and final code-quality gates passed.
- Production dependency audit: no known vulnerabilities found.
- After the cancellation follow-up, the full frontend suite passed 605 files and
  7,188 tests, with 0 failures. A fresh production build/typecheck, ESLint, and
  final code-quality gates also passed. The earlier import-only and upstream
  release-note/file-drop checks are included in this final full run.
- Production UI build passed. Existing large-chunk advisories remain; the circular
  re-export warning is resolved.
- After the upstream merge and OLE fix, the Windows test harness with the
  repository manifest embedded passed 1,808 tests, with 1 ignored and 0 failed.
  The 13 video tests include pixel-level checks for all eight EXIF orientations,
  bounded rotated output, metadata removal, and no repeated rotation through the
  browser PNG path. Four Windows drop-target tests cover path normalization,
  delegated COM release on success/error, and lost final drop data. Strict Clippy,
  formatting, and final code-quality gates passed again.
- Windows device checks below used the debug Host and Vite UI before the EXIF
  follow-up, with a separate temporary app data root and locally drawn,
  nonpersonal fixtures. The EXIF fix was verified by native pixel-level regression;
  it did not trigger another billable video generation.
- The follow-up debug Host was rebuilt with `tauri.dev.conf.json` and restarted
  against the same isolated profile. Its process remained responsive alongside
  the installed app, and the Vite endpoint returned HTTP 200. This is a startup
  check, not a fresh authenticated Saved or video-generation acceptance pass.
- After the upstream merge, the debug Host was rebuilt and its main workbench
  visibly rendered. A Windows Firewall permission prompt for the newly linked
  test harness obscured the window, so interactive follow-up was stopped without
  changing system permissions. The OLE checks above are native automated tests;
  no fresh Explorer-to-workbench drag gesture is claimed.

## Windows device acceptance

- AVIF opened in video mode with an immediate localized prompt. Browser decoding
  and the Host PNG snapshot succeeded. A real 6-second/480p request produced one
  590,201-byte H.264 MP4 (6.041667 seconds, 688 x 432) and passed the exact-call
  audit. Lightbox visibly played it with pause/seek controls.
- Editing the prompt and choosing 10 seconds/720p produced one 2,214,620-byte
  H.264 MP4 (10.041667 seconds, 1200 x 752). Session inputs matched the edited
  prompt verbatim and the selected options. These are the actual upstream output
  dimensions; the resolution option is the provider's named quality tier.
- The second video was applied as the test app's dynamic background. At a
  902 x 928 window, the preview retained its landscape frame, showed changing
  video content, and wrapped both actions below the preview without clipping.
- Library discovery showed exactly three source images and two generated videos.
  Transient source snapshots were absent; video cards had no image-to-video action.
- Image and video drafts remained independent across mode changes. Selecting a
  different source updated its preview and prefill. Removing the source cleared
  the video prompt and disabled generation.
- Cancellation was exercised against a confirmed running CLI process using a
  WebP source. The UI returned to Generate, the process exited, the owned output
  directory disappeared, no transient PNG remained, and the video count stayed
  at two. No automatic retry was started.
- Openverse returned 20 validated images in 2.9 seconds. Load more appended 20
  images, retaining existing cards and selection. Lightbox opened as 1 / 20;
  Escape closed only the preview. An appended image opened the video workspace
  with an immediate generic prompt while its original downloaded.
- Default X search returned 14 images in 55.2 seconds, with the real Grok Build
  CLI route and X post provenance displayed. Switching to Web retained source
  separation; Web returned eight validated images in 55.0 seconds.
- Pexels returned 20 validated images in 10.4 seconds with author and license
  attribution. Load more appended 20 images without replacing the existing
  cards. The first appended image downloaded successfully and opened video mode
  with the matching thumbnail, a localized generic prompt, and an enabled
  Generate action. Its external caption was not inserted into the prompt.
- A restart of the isolated Host was needed after provisioning the test Pexels
  credential outside the app: the presence UI reads disk, while actual secret
  values are cached per Host process. Search passed after restart. This is a
  test-setup observation, not a failure of the normal in-app save flow.
- Grok Saved opened its isolated WebView and encountered Cloudflare's human
  verification page. Authentication and challenge interaction were not
  automated. The September 4 authenticated gallery/paging/handoff evidence
  remains historical and is not presented as a fresh September 5 pass. The user
  subsequently deferred this check to the evening manual session.
- Final file inspection retained two MP4 results and found zero transient source
  PNGs or late cancellation outputs. The temporary Pexels credential file was
  removed and its absence verified; the original profile was not modified.

## Observed limitations

- One separate CLI attempt returned a video path without invoking any tool.
  The Host rejected it, displayed the localized generation error, and removed
  its transient directory. It did not add a fabricated result to the gallery.
  This is a model compliance failure, not the original AVIF format error; the
  application deliberately does not retry a potentially billable operation.
- Exact-call auditing is a result acceptance check after CLI execution. It
  cannot undo upstream calls that a nonconforming CLI may already have made.
- The existing production build still reports large bundle size advisories.
- The resumed September 6 session visibly reached the Saved gallery and showed
  additional pages after user interaction. This does not prove the earlier
  device-verification stall was fixed by script scoping. Saved preview and
  video-source handoff still need a fresh acceptance pass.
- Web discovery can still include weak topical matches and visually similar
  crops. This sample contained one unrelated image and two near-duplicate views;
  URL/media identity deduplication does not establish semantic relevance or
  perceptual uniqueness.

No credentials, raw CLI traces, source URLs, or personal account details belong
in this report.
