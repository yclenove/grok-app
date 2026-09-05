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
- Production build review exposed a circular re-export warning in the existing
  Reliability Center import. It now imports the view assembler directly; the
  rebuilt bundle no longer reports that circular chunk warning.

## Upstream synchronization

- Refreshed `upstream/main` on September 5: `fb863234`.
- Current branch already contains that commit; `git merge --no-edit upstream/main`
  returned `Already up to date`. There were no new conflicts.
- Work remains local. No push or PR is part of this task.

## Verification

- Focused frontend: 6 files, 57 tests passed.
- TypeScript, ESLint, strict Clippy, and final code-quality gates passed.
- Production dependency audit: no known vulnerabilities found.
- Full frontend: 604 files, 7,185 tests passed. The subsequent import-only fix
  passed the 44 goal-orchestration tests and a fresh production build/typecheck.
- Production UI build passed. Existing large-chunk advisories remain; the circular
  re-export warning is resolved.
- After the EXIF fix, the Windows test harness with the repository manifest
  embedded passed 1,804 tests, with 1 ignored and 0 failed. The video-specific
  subset passed 13 tests, including pixel-level checks for all eight orientations,
  bounded rotated output, metadata removal, and no repeated rotation when a
  normalized PNG is supplied through the browser path. Strict Clippy and final
  code-quality gates passed again.
- Windows device checks below used the debug Host and Vite UI before the EXIF
  follow-up, with a separate temporary app data root and locally drawn,
  nonpersonal fixtures. The EXIF fix was verified by native pixel-level regression;
  it did not trigger another billable video generation.
- The follow-up debug Host was rebuilt with `tauri.dev.conf.json` and restarted
  against the same isolated profile. Its process remained responsive alongside
  the installed app, and the Vite endpoint returned HTTP 200. This is a startup
  check, not a fresh authenticated Saved or video-generation acceptance pass.

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
