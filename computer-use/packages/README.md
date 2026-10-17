# Computer Use distribution

Release tags are `cu-v<semver>`. `release --dry-run` is offline and writes nothing.
`release --stage --tag cu-v1.2.3 --target <triple> --output <fresh-directory>`
stages a prebuilt `target/<triple>/release/gcu` and creates platform npm metadata.
CI signs the staged payload, archives it, and copies the same payload to the npm package.
`release --collect --tag cu-v1.2.3 --output <assets-directory>` requires all five
archives, SBOMs, Linux packages and npm packages, then renders manifests and SHA256SUMS.
The workspace version must match the tag; CI changes it only in its disposable checkout.

Targets: macOS 14+ arm64/x64, Windows 10 1903+ x64, Linux glibc x64/arm64.
Linux archives are built on Ubuntu 24.04 (glibc 2.39); musl/Alpine is unsupported.
No browser, Node, Python or other third-party runtime is bundled. npm uses the
user's existing Node installation to select an optional platform package; it never downloads binaries.

## Install and remove (disposable desktop VM)

Download the matching archive and SHA256SUMS. Verify before extracting:
`sha256sum --check --ignore-missing SHA256SUMS` (macOS: `shasum -a 256 -c SHA256SUMS`).
On Windows compare `Get-FileHash <archive> -Algorithm SHA256` with SHA256SUMS.
A digest alone checks corruption, not publisher identity. Signed tag builds include
`SHA256SUMS.sigstore.json`; verify using `cosign verify-blob --bundle SHA256SUMS.sigstore.json
--certificate-identity https://github.com/OWNER/REPO/.github/workflows/release-computer-use.yml@refs/tags/cu-vVERSION
--certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS`.
Manual builds skip cosign (no OIDC request); the artifact is not a published release.

Extract tar.gz/zip into a new directory, add it to PATH, run `gcu version`, then
`gcu doctor --json`. On macOS keep `Grok Computer Use.app` beside `gcu`, register it
with `open -Ra '/absolute/path/Grok Computer Use.app'`, and authorize that helper.
A missing Apple certificate preserves the identifier-only ad-hoc signature, **not**
Developer ID/notarization: recheck authorization on every update; it cannot provide
production publisher identity. Never disable Gatekeeper or system security in automation.
Missing Windows certificates produce unsigned executables (SmartScreen may warn).
Configured signing failures abort the build; they never fall back to unsigned output.

npm: `npm install -g gcu@VERSION`; on macOS register the helper inside the installed
`@grok-computer-use/darwin-ARCH/bin` directory with `open -Ra` before use.
Remove with `npm uninstall -g gcu`. Linux: `sudo apt install ./<package>.deb` or
`sudo dnf install ./<package>.rpm`; remove with `sudo apt remove grok-computer-use`
or `sudo dnf remove grok-computer-use`. System shared-library dependencies come
from the native package tools; desktop services/permissions are still required.
Homebrew: review `gcu.rb`, `brew install --formula ./gcu.rb`; `brew uninstall gcu`.
Windows: review the three generated winget manifests, validate with
`winget validate --manifest <directory>`; maintainer submission to winget-pkgs is manual.
After listing in winget: `winget install RongleCat.GrokComputerUse`, remove with
`winget uninstall RongleCat.GrokComputerUse`.

Use `gcu install grok --scope project` in an empty VM project to connect Grok Build;
remove with `gcu uninstall grok --scope project`. Never point smoke tests at a real
user home/account. Archive removal deletes only the extracted directory; registrations
and OS grants are user data and require explicit uninstall/revocation. Scripts in
`scripts/install-smoke/` test disposable destinations and include native doctor/MCP.
They fail if the native path is unavailable; offline fake tests are not native evidence.

## Publication controls

The workflow never runs on PR events. Manual dispatch always produces artifacts,
even with dry_run=false. Only a `cu-v*` push may publish a GitHub Release; a matching
CHANGELOG section is required. npm also requires NPM_TOKEN (a granular token with
publish rights for `gcu` and all five scoped packages), publishes platform packages
first and uses `--provenance`. Prereleases use the npm `next` tag. Missing NPM_TOKEN
leaves reviewable npm packages in the artifact. Homebrew/winget submissions remain
manual: configure ownership of a tap and the winget publisher account first; this
workflow does not create external repository changes or PRs.

Apple secrets (all together): APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD,
APPLE_SIGNING_IDENTITY, APPLE_TEAM_ID, APPLE_API_ISSUER, APPLE_API_KEY,
APPLE_API_KEY_P8. These use Developer ID Application and App Store Connect notarytool.
Windows: WINDOWS_CERTIFICATE and WINDOWS_CERTIFICATE_PASSWORD. Missing certificate
means skip; do not create empty secrets. Certificates exist only during signing steps
and are removed by traps/finally. No get-task-allow or extra entitlements are requested.
GitHub Release uses the job-scoped GITHUB_TOKEN; cosign uses GitHub OIDC/Fulcio/Rekor
only in tag publication (requires network and repository Actions write/OIDC permission).
SBOMs include a deterministic, explicitly all-target Cargo.lock CycloneDX inventory
plus cargo-cyclonedx's per-platform resolved dependency graph. SHA256SUMS covers final
archives, native packages, SBOMs, rendered manifests and packed npm tarballs.
