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

- The Appearance wallpaper card became a single-column layout at its own
  narrow container width. The preview retained a `16:10` shape instead of
  collapsing into a vertical strip; actions and sliders remained reachable.
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

## Automated verification

Post-QA verification completed against the final working tree:

- Focused layout and Imagine control coverage: `9/9` passed.
- Frontend suite: `592` files, `7086/7086` tests passed.
- TypeScript typecheck and ESLint: passed.
- Production UI build (`pnpm build:ui`): passed.
- Rust formatting and `cargo clippy --all-targets -- -D warnings`: passed.
- Windows Rust harness, with the repository test manifest embedded according
  to CI: `1732` passed, `1` ignored, `0` failed.
- Final code-quality gate and `git diff --check`: passed.

## Notes

- Generated-video containment and validation continue to use the canonical
  Windows path internally. The DTO strips the Windows extended-path prefix so
  the frontend loopback media resolver can display the file.
- The real generation timing is an observed smoke result, not a latency SLA.
