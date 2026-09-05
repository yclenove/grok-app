# Wallpaper image-to-video hardening — September 5

Status: local implementation and automated regression complete. Windows video,
search, and source-preparation acceptance passed. At the user's request, the fresh
authenticated Grok Saved check is deferred to the evening manual session; it is
not being polled or treated as passed. No push or PR.

## Review findings and changes

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

## Upstream synchronization

- The earlier September 5 upstream baseline was `fb863234`.
- A later refresh found `d794849f` (Windows Explorer drag-drop, #1017). It was
  merged locally as `13b594a4`. The only conflict was the Windows feature list
  in `Cargo.toml`; both the existing pipe support and upstream COM/OLE features
  were retained. This is the upstream snapshot used for the final checks below.
- Work remains local. No push or PR is part of this task.

## Verification

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
- The user deferred fresh authenticated Grok Saved regression until they can
  assist with the manual Cloudflare verification. Its window-open boundary was
  checked; the live saved-media gallery remains unverified in this isolated run.
- Web discovery can still include weak topical matches and visually similar
  crops. This sample contained one unrelated image and two near-duplicate views;
  URL/media identity deduplication does not establish semantic relevance or
  perceptual uniqueness.

No credentials, raw CLI traces, source URLs, or personal account details belong
in this report.
