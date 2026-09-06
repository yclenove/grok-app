# Wallpaper image-to-video hardening — September 5

Status: local implementation and automated regression complete. Initial Windows
video, search, and source-preparation acceptance passed. The resumed September 6
session found and fixed a missing media viewer in the standalone theme editor;
its focused regression and standalone desktop video playback now pass.
Saved now visibly shows media and additional pages after the user's manual
session, but its full preview/source-handoff acceptance remains open. No push or PR.

## Acceptance ledger

The implementation baseline is `ad53ae93`. The resumed session merged upstream
`ecd1d998` as `37e15a6a`; a later fetch found upstream `b7196790` (v0.2.32),
which is merged locally as `1cbbd58f`. The current branch also contains a
search-palette lifecycle fix in `9f1aa261`. The current full frontend suite and
UI build cover the merged frontend and lifecycle fix. The Windows Rust harness,
strict Clippy, and formatting checks cover the merged native implementation;
the later search fix changes only frontend files. Dependency audit and final
quality gates also pass.

| Requirement | Evidence | Status |
| --- | --- | --- |
| Fix unsupported image-to-video inputs | Real AVIF-to-MP4 run below; bounded browser conversion and independent native source decoding; EXIF pixel regressions | Passed, with later source fixes covered by native tests |
| Immediate editable prompt without an extra model/network request | Synchronous `buildWallpaperVideoPrompt`, controller/editing tests, both template keys in all 15 catalogs, real edited-prompt generation below | Passed |
| Isolate remote captions from agent instructions | Only Imagine's original prompt is used as context; tests reject inherited copy from X, Web, Openverse, Pexels, Saved, and library sources | Passed |
| Restrict video execution and validate its output | Dedicated CLI flags, fresh session UUID, exact source/prompt/options audit, Host-owned result copy, containment/signature/size checks and rejection tests | Passed as a result-acceptance boundary; CLI compliance limitation remains below |
| Cancel and release temporary resources | Real running CLI cancellation below; latest delayed encoder/reader tests; late results cannot start generation | Passed; latest browser cancellation change has automated coverage |
| Merge latest fetched upstream and preserve local changes | `1cbbd58f` merges upstream `b7196790` (v0.2.32); the OLE conflict keeps the validated `ReleaseStgMedium` implementation; search lifecycle fix is `9f1aa261` | Passed; upstream is an ancestor of HEAD |
| Complete automated regression and build gates | Current run: 609 frontend files / 7,236 tests, production build/typecheck, lint and quality gates; 1,808 native tests with 1 ignored; four drop tests retained | Passed; the single ignored native test regenerates a mock-stream fixture and is not an acceptance test |
| Real Windows generation, playback, background, source selection and search | Two MP4s and device observations below; 6s/480p and edited 10s/720p requests | Passed at the initial device-validation build; follow-up distinctions remain explicit below |
| Final Windows interaction after later code changes | Main and standalone editor libraries preserve portrait/landscape ratios; the fresh generated apple MP4 plays on first open in the editor at 0:03 / 0:06 and Escape retains the library | Partial; Explorer drag remains pending |
| Fresh authenticated Grok Saved gallery, paging and video handoff | September 6 desktop observations showed 40 displayed / 55 cached, then 80 displayed / 98 cached after user interaction | Gallery and page growth observed; preview and video handoff remain pending |
| Review report and local commits without push/PR | Report updated after local merge `1cbbd58f` and fix `9f1aa261`; no push or PR | Passed |

The complete objective remains open for the two pending device rows. Continue
the Explorer folder drop into the sidebar/file drop into the composer using
nonpersonal fixtures. Saved login and page growth
have been observed; finish preview and source handoff without exporting
authentication state.
The completed automated suites do not need another run unless code changes or a
new failure gives a reason. No further background Saved polling is planned.

## Review findings and changes

- September 6 follow-up: the fresh 1,400,713-byte apple MP4 played on first
  open after reloading the standalone editor. No Next/Previous workaround was
  used. Clicking the video exposed its pause/seek controls at 0:05 / 0:06;
  after removing all temporary diagnostics, a second first-open check showed
  changing frames and paused at 0:03 / 0:06. Escape preserved the selected
  card and video filter. The native accessibility tree misleadingly retained
  "unable to play media" while the video was playing; that stale string alone
  is not evidence of a decoding failure. Temporary event diagnostics recorded
  successful metadata/playing events and no media error and are not shipped.
- Two delayed-endpoint regressions cover library and Imagine result cards:
  when the media endpoint becomes ready after mount, the video source updates
  to loopback HTTP. The gallery, lightbox, and viewer suites passed 20 tests
  across three files; targeted ESLint and TypeScript passed. Product source is
  unchanged from the previously built implementation after diagnostic removal.
- Native video controls exposed 480p and 720p, and selecting 720p updated the
  field. The checked local Grok Build `video_gen` contract has only image,
  prompt, duration, and resolution fields for `image_to_video`; its model is
  fixed to `grok-imagine-video-1.5`. Independent ratio, 1080p, and model choices
  remain unsupported by this integration. No new generation was submitted.

- The standalone `ThemeEditorApp` did not mount `ImageViewerProvider`. Both
  wallpaper library and Imagine result cards consequently called the optional
  viewer's no-op fallback. The editor now provides the existing media lightbox
  with its current locale. Two entry-point regressions failed before the fix
  and passed afterward using the real image/video lightbox and close control.
  The three focused suites passed 19 tests; TypeScript, targeted ESLint,
  production UI build, and final code-quality gates passed. Existing bundle
  size and mixed-import advisories remain.
  Later native standalone playback is recorded below separately from these tests.
- Gallery cards previously forced paged media into a 16:10 crop. Cards now use
  each item's validated width/height metadata and `contain`, preserving the
  source ratio for library and remote results. The layout guard and gallery
  tests pass. The current Grok Build image-to-video contract still exposes only
  480p/720p, has no image-to-video aspect parameter, and fixes the model to
  `grok-imagine-video-1.5`; the UI therefore does not advertise unsupported
  1080p, ratio, or model selectors.
- An earlier full frontend regression at this point passed 608 test files
  and 7,222 tests. The current post-merge run below supersedes this count.
- A later visual follow-up changed the remaining video-source thumbnail from
  `cover` to `contain`; its eight layout/control tests passed. The current
  full suite includes this follow-up. At 902 x 928, the main-window
  library showed portrait and landscape cards without the old uniform crop.
  Opening the existing six-second mountain/lake MP4 displayed changing frames;
  clicking the video paused it at 0:03 / 0:06 with seek controls visible.
  Escape closed only the lightbox and retained the filtered library and selected
  card. No new generation or background application was performed.

- The Theme submenu opened on pointer entry but toggled closed on the following
  click. Clicking now keeps it open. The regression reproduces pointer entry
  followed by click and opening the editor. Four menu/editor tests and targeted
  ESLint passed. A native click through the menu opened the standalone editor.
- At 842 x 602, the standalone editor library showed 16 existing media items
  (11 images and 5 videos), with distinct portrait and landscape proportions.
  The six-second drawn mountain/lake fixture opened at 5 / 5 in the real
  lightbox. Playback advanced from 0:00 to 0:02 / 0:06 and the mountain/water
  frames changed. Escape closed the lightbox and retained the filtered library
  and selected item. This verifies native decoding in the repaired editor.
  The app's existing composer draft and user media were preserved.
- A fresh `pnpm build:ui` passed after the thumbnail and menu fixes, including
  TypeScript compilation. Existing mixed-import and large-chunk warnings remain.
  The current 7,236-test full run includes these two follow-ups.
- Reopening Saved from the standalone editor subsequently displayed the official
  Cloudflare human-verification page again. Manual completion was requested;
  no authentication UI was automated. This latest observation does not negate
  the earlier authenticated gallery/page growth, but fresh Saved preview and
  source handoff remain unverified. The local Grok Build source examined for
  video capabilities is evidence for this integration, not a guarantee about
  other or future CLI versions.
- A fresh post-fix six-second/480p run invoked `image_to_video` once and wrote a
  1,400,713-byte MP4 under the isolated wallpaper output. The Host accepted the
  result and rendered the result card. A follow-up fix now waits for the media
  endpoint before resolving local result URLs and refreshes the gallery when
  that endpoint becomes ready; its gallery and layout regressions pass.
  The first-open playback checks at the start of this section verify this
  generated MP4 after the fix, including a check without temporary diagnostics.

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

- The latest fetched upstream is `b7196790` (v0.2.32), merged as `1cbbd58f`.
  The Windows drop-target conflict retains RAII `DropMedium` ownership and
  `ReleaseStgMedium` from the Ole namespace, while incorporating upstream
  parent-window coordinate conversion and deduplicated HWND registration.
  The lockfile retains both local `bytes` and upstream `block2` dependencies.
  The new upstream macOS bridge was formatted with the repository formatter.

## Verification

- Current September 6 regression: 609 frontend files / 7,236 tests passed
  without unhandled errors (189.74 seconds). The initial run exposed a late
  search callback after teardown; `9f1aa261` invalidates dispatched search
  requests on close/unmount. Three delayed-result regressions cover close,
  cleared queries, and title search; the close case failed before the fix.
  After correcting a test fixture timestamp, all six focused search tests,
  TypeScript, targeted/full ESLint, and the production UI build passed again.
  The final UI build completed in 25.75 seconds with existing bundle advisories.
- The latest merged Windows harness passed 1,808 tests with 1 ignored and
  0 failed (51.86 seconds). Strict Clippy and formatting checks passed.
  Final quality gates, production dependency audit (no known vulnerabilities),
  dependency hygiene, and all three website download self-tests passed.
- The post-merge debug Host rebuilt against the existing isolated QA profile.
  A fresh desktop observation showed the responsive workbench, preserved
  composer draft, and the Theme submenu remaining open after a click.
  Saved preview/source handoff and physical Explorer drag remain pending;
  startup and menu observations do not close either acceptance item.
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
