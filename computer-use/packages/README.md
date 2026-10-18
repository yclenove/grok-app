# Computer Use distribution

Release tags are `cu-v<semver>`. `release --dry-run` is offline and writes nothing.
`release --stage --tag cu-v1.2.3 --target <triple> --output <fresh-directory>`
stages a prebuilt `target/<triple>/release/gcu` and creates platform npm metadata.
CI signs the staged payload, archives it, and copies the same payload to the npm package.
`release --collect --tag cu-v1.2.3 --output <assets-directory>` requires all five
archives, SBOMs, Linux packages and npm packages, then renders manifests and SHA256SUMS.
The workspace version must match the tag; CI changes it only in its disposable checkout.
Source links below point to repository main because archives contain this README
without the source docs; inspect the matching `cu-v<version>` tag for older builds.

Targets: macOS 14+ arm64/x64, Windows 10 1903+ x64, Linux glibc x64/arm64.
Linux archives are built on Ubuntu 22.04; musl/Alpine is unsupported.
No browser, Node, Python or other third-party runtime is bundled. npm uses the
user's existing Node installation to select an optional platform package; it never downloads binaries.

## Driver default, upgrade and release qualification

> **Upgrade notice:** `gcu mcp` with no `--driver` now selects `auto`. Existing
> MCP registrations may first reach the real desktop after installing a build
> containing the matching native backend. Review OS permissions and App grants
> before reconnecting. Installers add neither `--driver fake` nor auto-approval;
> the App setting remains off by default.

`auto`/`native` use the same platform/entry/feature checks and fail closed. No
fake fallback, elevation, broader input route or borrowed terminal TCC is used.
`--scene` is valid only with explicit fake; release fake requires
`GCU_ALLOW_FAKE=1`, which grants no real permission. Fixture identities have a
synthetic image digest and cannot serve as real App grants. CLI grant records
share `approvals.json`, matched by identity; no fake grant migration is performed.
Switching/cleaning fake does not clear persistent Stop.

| Platform / entry | Compiled route and current package boundary |
|---|---|
| macOS route A | Stdio `gcu mcp` auto/native proxies `Grok Computer Use.app` with shared Host Stop/activity; helper TCC and complete product actions/resume **not_run**. Terminal native `call`, `doctor` and self-hosted HTTP MCP exit 2. |
| macOS route B | Authenticated App engine assembly admits macOS 27 arm64 only; full TCC/Dev-signature isolation is **not_run**. macOS 14/15/Intel are build targets, not verified desktop support. |
| Windows | A1 monitored Worker assembly requires default-off `native-windows`. Current release workflow does not enable it: desktop execution is **fake-only**, auto/native exit 2. |
| Linux X11 | A2 AT-SPI + X11 assembly requires default-off `native-x11`. Current workflow does not enable it: desktop execution is **fake-only**, auto/native exit 2. AT-SPI is required; XTEST is foreground and Stop/activity/policy-gated. |
| Linux Wayland | A3 probe/Host assembly requires default-off `native-wayland`; actual frames additionally need `gcu-driver-linux/pipewire`. The CLI does not initiate consent/attach proven portal sessions. Current workflow enables only PipeWire, leaving desktop execution **fake-only** and native selection refused. Real GNOME/KDE/wlroots **not_run**. |

Source builds can explicitly compile the matching gcu-cli feature; compiling a
feature is not release permission. Review [actual entry behavior](https://github.com/yclenove/grok-app/blob/main/computer-use/README.md#what-this-build-runs)
and [Wayland scope](https://github.com/yclenove/grok-app/blob/main/computer-use/docs/wayland.md) before doing so. No native features or
installer arguments are changed by this documentation update.

Windows foreground uses the [ADR 0013 action-gated Stop lease](https://github.com/yclenove/grok-app/blob/main/computer-use/docs/adr/0013-windows-stop-guard-liveness-lease.md):
no idle pre-renewal; prove both hooks before injection and renew during it.
The fourth physical-mouse probe is a same-position absolute move and **can
trigger hover**. The task handoff reports only one passing physical `hook_probe`
run; A1's other physical/product checks remain `not_run` in STATUS. UIA Invoke
can activate a window, and own High/System/UIAccess or a secure/non-interactive
desktop is refused. No InProcess fallback or elevation is attempted.

The extension's fixed ID is `jgpkhdjlddaoopklipcnpcjcpiancmff`. Browser L0 remains
available where the selected native driver's capabilities permit. **L2 DOM
fusion/actions are disabled everywhere**: A4a lacks a trustworthy shared top
document → OS window mapping, so A4b is blocked. ADR 0014 is accepted only for feasibility
validation, not a delivery commitment. Existing Chrome/Edge
registration/developer-load steps are in the [extension guide](https://github.com/yclenove/grok-app/blob/main/computer-use/docs/browser-extension.md#install);
there is no published store listing or verified browser integration claim.

Stop survives turns/reconnects/restarts; only explicit trusted user recovery
resumes. Route B abnormal control disconnect persists global Stop; `stop-reason`
contains a display-only enum and cannot grant recovery. Required limits remain
100 ms latch / 500 ms release / 5 s blocked native call or Worker termination
(4750 ms driver budget); missing physical measurements are `not_run`.
Always grants survive sessions until revoked. Normal MCP close deletes owned
temporary images/overflow; CLI exports are cleaned only by a later eligible
invocation after one hour. Crashes/IO errors and downstream copies can leave
data. App history, grants and Stop have separate storage. See the dated
[security review](https://github.com/yclenove/grok-app/blob/main/computer-use/docs/security.md) and [data inventory](https://github.com/yclenove/grok-app/blob/main/computer-use/docs/data-inventory.md)
for open signature, ACL and cleanup findings; neither is a release sign-off.

**No new release qualification is claimed.** Under
[07 §8](https://github.com/yclenove/grok-app/blob/main/docs/plans/computer-use-v2/07-quality-gates.md#8-发布门槛), A-α needs
macOS L1–L4, native L6 ≥70% and the first six safety items plus the specified
redaction/Stop regressions; A-β needs three-platform L1–L5, native+text L6 ≥70%
and zero red_team violations. Formal A-1.0/App needs L1–L7, macOS ≥85% and
Windows/Linux X11 ≥75% success, budgets, all safety items, ≥30 App compatibility
and signing/notarization. Experimental App visibility also requires A-α.
[STATUS](https://github.com/yclenove/grok-app/blob/main/docs/plans/computer-use-v2/STATUS.md) still lacks clean-machine
install→client→grant→fixture action→Stop/resume→revoke→uninstall, real-model,
performance/soak and signing evidence. Q5 requires absent platform evidence to
stay `not_run`, with unaccepted release routes closed. Keep native features off
in release artifacts until those gates are met; fake/virtual results cannot
qualify a native package.

## Install and remove (disposable desktop VM)

Download the matching archive and SHA256SUMS. Verify before extracting:
`sha256sum --check --ignore-missing SHA256SUMS` (macOS: `shasum -a 256 -c SHA256SUMS`).
On Windows compare `Get-FileHash <archive> -Algorithm SHA256` with SHA256SUMS.
A digest alone checks corruption, not publisher identity. Signed tag builds include
`SHA256SUMS.sigstore.json`; verify using `cosign verify-blob --bundle SHA256SUMS.sigstore.json
--certificate-identity https://github.com/OWNER/REPO/.github/workflows/release-computer-use.yml@refs/tags/cu-vVERSION
--certificate-oidc-issuer https://token.actions.githubusercontent.com SHA256SUMS`.
Manual builds skip cosign (no OIDC request); the artifact is not a published release.

Extract tar.gz/zip into a new directory, add it to PATH and run `gcu version`.
On Windows/Linux, `gcu doctor --json` requires a matching compiled native route;
otherwise it exits 2. A matching native doctor reports the route, issues and
capabilities (`ready=false` exits 3); it does not grant permission.
On macOS native terminal doctor exits 2: keep `Grok Computer Use.app` beside
`gcu`, register it
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
[install-smoke](https://github.com/yclenove/grok-app/blob/main/computer-use/scripts/install-smoke/README.md) target disposable destinations
and native doctor/MCP. Current default Windows/Linux packages fail the native
selection requirement; macOS terminal native doctor is unsupported. The existing
scripts are not evidence of a completed install and do not bypass those failures.
Offline fake tests are not native evidence; full clean-machine checks are `not_run`.

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
