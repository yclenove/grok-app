# Computer Use installer seed

Managed-browser policy: macOS 14+ only; the App's deployment floor stays macOS 11+. Native support is checked before product browser path resolution. Other surfaces and Node/MCP bootstrap do not inherit this browser-only floor. Native ARM/Intel acceptance of the new boundary and maintained full packs is still not_run; the source-pair audit is not seed publication.

Copied into the App install directory by Tauri/NSIS resources. Repair copies these files into `{app_data}/computer-use/runtime/packs/`. Never uses system Node or PATH.

Windows `js-runtime` is official Node 20.18.0 win-x64 (`node.exe` SHA-256 `35b7c95a379beb606f5798ed83081690df13190077630b234163c6607aa4cc94`). The `playwright` component is official `playwright-core-1.48.0.tgz` (SHA-256 `60cbf41da4e72847064ad7c720088dd133cf8601f7af9d5b1b73751d6e649562`). `diagnose`/`repair`/`resolve` never consult PATH. Text stubs remain unhealthy. `playwright/worker.mjs` is the Host-owned worker contract, not the npm package. macOS/Linux packs are `not_run`.
