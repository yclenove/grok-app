# Computer Use third-party notes

This feature is default-off. It is not enabled in a release build unless the user turns it on.

Tracked source of truth for Windows x64 pins: `src-tauri/resources/computer-use/windows-x64.lock.json`.
Generated seed is gitignored and rebuilt by `node scripts/prepare-computer-use-runtime.mjs --prepare --target x86_64-windows`.

| Component | License | Pin / location | Status |
| --- | --- | --- | --- |
| Host Broker | MIT (this repo) | `src-tauri/src/computer_use/` | in tree |
| Native Wayland EI | MIT (upstream libei; retained COPYING) | static libei 1.5.0, source archive SHA-256 `da1fba92daccd0667bc46c3ee952d4ae8cfc6bdb4c0bb4d34df26528fb240618`; `scripts/build-computer-use-libei.sh` | private SDK/native tests; not yet enabled in App. Distribute SDK `share/licenses/libei/COPYING` with any binary containing it; no dynamic older-system fallback |
| App-owned JS runtime | Node.js license (extracted `seed/bin/LICENSE`) | Node `20.18.0` win-x64; archive SHA-256 `f5cea43414cc33024bbe5867f208d1c9c915d6a38e92abeee07ed9e563662297` (29602979 bytes); `node.exe` SHA-256 `35b7c95a379beb606f5798ed83081690df13190077630b234163c6607aa4cc94` (69804184 bytes) | official zip extracted into generated seed `bin/node.exe`. **禁止** `which node` / PATH Node |
| Production browser worker | MIT (this repo) | source `tools/computer-use-browser/`; generated copy `seed/playwright/worker.mjs` | packed from source on prepare |
| Playwright | Apache-2.0 (`LICENSE` / `NOTICE` / `ThirdPartyNotices.txt` inside playwright-core) | `playwright-core@1.48.0` tgz SHA-256 `60cbf41da4e72847064ad7c720088dd133cf8601f7af9d5b1b73751d6e649562` (1957232 bytes); tree SHA-256 `f5e938f6c30f0ca5f74be2ac14b45a23840f047a82032d1aa4195e7f24d8edce` (335 files / 7700524 bytes) | materialized `seed/playwright/node_modules/playwright-core` |
| Chromium | BSD-3-Clause plus `chrome-win` third-party notices (`CREDITS.html`) | Playwright 1.48.0 `browsers.json` revision `1140` / `130.0.6723.31`; URL `https://playwright.azureedge.net/builds/chromium/1140/chromium-win64.zip` SHA-256 `da752c83305f84d2e4ce2c93842532402289f2ebf5ca96178ed2e59c58a4aedb` (147201336 bytes); tree SHA-256 `63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3` (84 files / 361230888 bytes); `chrome.exe` SHA-256 `de88b8d38bf8807685bf0293e79f7a49a449309b05d36ecc0b22eed92cf445e8` | App-owned `seed/chromium/chrome-win/`; zip stays in download cache, not in the published seed; product does not scan system Chrome |
| Synthetic MCP probe | MIT (this repo) | `tools/computer-use-probe/` | in tree; not shipped in the install runtime pack |
| Cua Driver | MIT (upstream) | research clone only | gitignored; **不进发行包** |
| BrowserSkill | see Tencent repo | research cache only | not a product dependency |

Do not copy Codex private runtimes. Do not install a DSH plugin marketplace.
macOS/Linux runtime packs remain `not_run`.
