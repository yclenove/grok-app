# Wallpaper image-to-video QA - 2026-09-04

Branch: `fix/wallpaper-responses-network-observability`

## Scope

- Appearance wallpaper preview at narrow card widths.
- Static gallery card to Imagine video handoff.
- Source preparation, supported duration/resolution controls, cancellation,
  generation, library discovery, and Lightbox playback.
- Windows path compatibility for generated local media.

## Windows Tauri device pass

Tested in the debug Tauri app at approximately `902 x 948` with the repository
dev server using the configured `127.0.0.1:10809` proxy.

- A follow-up screenshot at the same narrow size exposed that the initial
  grid/container-query fix had not taken effect in the live WebView: the
  preview collapsed into a vertical strip beside the fixed-width actions.
  That screenshot supersedes the earlier narrow-layout pass claim.
- The library rendered 78 entries after the pass (`61` images, `17` videos).
  Static cards exposed the bottom-right video action; video cards did not.
- A Pexels landscape prepared as a local source and opened Imagine in video
  mode. The source used a compact `56 x 36` preview, and the optional prompt,
  `6` / `10` second, `480p` / `720p`, and Generate controls were all visible.
- A generation was started and cancelled. The action changed to
  `正在取消...` and returned to Generate in about two seconds without a late
  result.
- A real `6` second, `480p` generation completed in about three minutes. The
  result appeared in the Imagine gallery, opened in the real Lightbox, and was
  visibly playing at `0:02 / 0:06` with pause and seek controls.
- A second visible library image then replaced the source successfully; its
  thumbnail and filename updated without closing the dialog.
- `Set as background` was intentionally not invoked during QA.

## Final narrow-width regression pass

The corrected layout uses intrinsic flex wrapping with minimum item widths
instead of depending on the failed container-query breakpoint. The minimums
force a new flex line before the controls' text can squeeze the preview into a
vertical strip, while `min(..., 100%)` still prevents overflow in very narrow
cards. It was measured in system Edge against the live repository Vite page at
a `900 x 946` viewport:

- At the reported card width of `293 px`, the preview measured
  `293 x 183.13 px` (`1.600`, exactly `16:10`) and the action area wrapped below
  it at the full `293 px` width.
- At a `700 px` card width, the preview and action area returned to one row;
  the preview measured `404 x 252.5 px` (`1.600`) and the action area measured
  `280 px`.
- The rendered narrow card had no overlap, clipped labels, or unused action-row
  gap.
- The final debug Tauri window was then rechecked at approximately `902 x 948`.
  The wallpaper preview rendered at approximately `271 x 169 px` (`1.60`) as a
  complete landscape frame, its actions wrapped below at full card width, and
  both sliders remained intact.

## Search source acceptance

Live source checks used fresh, non-sensitive queries. Query text, media URLs,
account details, and stored credentials are intentionally omitted.

- Pexels returned 20 validated images in `19.2` seconds with source, author,
  and license attribution. Loading more reused the prefetched continuation and
  expanded `20 -> 40`; the first and first appended cards opened as `1 / 20`
  and `21 / 40`. The appended card's video action prepared the original in
  about `15.5` seconds and opened Imagine in video mode with the source preview
  and description intact. Generation itself was not started in this rerun.
- Web search returned 2 validated images in `55.3` seconds with visible source
  provenance. The first continuation appended 1 result in about `28` seconds;
  the next continuation ended honestly with no more results in `20.5` seconds.
  Existing results and selection remained available throughout.
- Openverse returned 16 validated images in `5.9` seconds with author and
  Creative Commons attribution. Its warmed next page expanded the gallery to
  36 items in the first post-click frame, and the first appended item opened as
  `17 / 36`.
- Grok Saved opened in its isolated persistent WebView through the configured
  proxy, but Cloudflare required a human verification. No challenge was solved
  automatically; pagination and media playback remain pending that manual
  step.

## Automated verification

Post-QA verification completed against the final working tree:

- Corrected appearance and wallpaper layout guards: `6/6` passed.
- Focused wallpaper frontend suites: `68/68` tests passed.
- Frontend suite: `592` files, `7087/7087` tests passed.
- TypeScript typecheck and ESLint: passed.
- Production UI build (`pnpm build:ui`): passed.
- Rust formatting and `cargo clippy --all-targets -- -D warnings`: passed.
- Windows Rust harness, with the repository test manifest embedded according
  to CI: `1732` passed, `1` ignored, `0` failed.
- Final code-quality gate covered `77/77` thousand-line files; `git diff
  --check` passed.

## Notes

- Generated-video containment and validation continue to use the canonical
  Windows path internally. The DTO strips the Windows extended-path prefix so
  the frontend loopback media resolver can display the file.
- The real generation timing is an observed smoke result, not a latency SLA.
