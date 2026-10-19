# Browser DOM extension

Browser L0 accessibility works without an extension where the selected native
driver exposes it, subject to the same App/action approvals and Stop. **The
optional MV3 extension does not currently provide production L2 DOM observations
or actions on any platform.** It uses an existing Chrome or Microsoft Edge;
no browser runtime is bundled. DOM representation/action code and tests exist,
but no production DomChannel is connected.

This is a pre-publication guide. The checked-in manifest carries the maintainer
public key. Its derived extension ID is `jgpkhdjlddaoopklipcnpcjcpiancmff`.
Registration and startup refuse a missing key, a placeholder key, or any other
origin. The native-host executable acknowledges sharing only. L2 stays closed:
A4a found no reproducible shared top-document → OS-window proof on Windows,
X11, Wayland or macOS; A4b is therefore blocked and DOM dispatch is not connected.
ADR 0014 explored a challenge-marker binding and was **exited on 2026-10-19**: the race between
the last native check and the DOM effect could not be proven away. No binding mechanism is planned. A share badge alone does
not demonstrate that DOM observation or control is operational. See [proof](#proof) and the
[maintainer release checklist](../tools/browser-extension/store/README.md#maintainer-inputs-and-release-checklist).

<a id="install"></a>
## Install

1. These are existing registration/developer-load steps for review, not an
   operational L2 installation. Obtain a matching gcu and extension build.
   The extension requires Chromium's document-ID APIs (Chrome minimum 106); Edge support must be
   verified for the actual release. Only per-user native host registration is
   supported on macOS, Windows and Linux. Other Chromium variants are not
   registered by this installer.
2. Install from the verified store listing when published: Chrome
   `__MAINTAINER_CHROME_WEB_STORE_LISTING_URL__` or Edge
   `__MAINTAINER_EDGE_ADD_ONS_LISTING_URL__`. These explicit placeholders are not
   working store links. Until then, maintainers can build with the
   [extension build instructions](../tools/browser-extension/README.md#build-and-checks),
   open `chrome://extensions` or `edge://extensions` in a disposable profile,
   enable Developer mode and choose **Load unpacked** with the generated `dist/`
   directory. Use a build with the maintainer-approved public identity; an
   arbitrary unpacked extension ID cannot replace it. Do not generate a key or
   override the native allowlist as a workaround.
3. Verify the browser's extension ID is `jgpkhdjlddaoopklipcnpcjcpiancmff`. Both the extension and gcu embed
   the matching public key. No developer account or extension-private signing
   key is needed by an end user.
4. Preview and install the local native messaging host from a terminal:

   ```sh
   gcu install chrome-extension --dry-run
   gcu install chrome-extension
   ```

   The command registers `ai.grok.computer_use` for both Chrome and Edge under your
   user account; it does not install the browser extension or edit MCP client
   settings. Keep the gcu executable at the registered absolute path. If it moves,
   preview/register the new release again. Repeating installation is idempotent.
   See [CLI registration paths and flags](cli.md#native-host--chrome-extension).
5. Pin the extension's toolbar action using the browser's extensions menu.
   This allows consent/revocation UI checks; it does not connect a DOM session.
   Use the already declared L0 route for browser Computer Use and authorize the
   browser separately when prompted. Extension installation grants no App access.

For fixture testing, `--home /absolute/disposable/home --gcu /absolute/path/gcu`
selects a disposable registration root and executable. Do not use real accounts
or personal browser data for release/reviewer tests.

<a id="share"></a>
## Share a tab

1. Select the HTTP(S) tab you intend your Computer Use session to process. Browser
   internal pages, extension pages and local `file:` pages cannot be shared.
2. Click the extension toolbar action, whose tooltip is **Share this tab** (or its
   localized equivalent). The tooltip changes to **Stop sharing this tab**, and
   a `●` badge marks browser consent. There is no popup, pairing code or automatic
   grant for other tabs. Each tab requires its own click; a share remains bound to
   its exact document rather than following whichever tab has focus later.
3. The current native entry acknowledges consent only; there is no production
   DOM observation/action request to try. Sharing by itself reads no tree and
   executes no action. Normal App/action approvals still apply to L0. If gcu
   is stopped, sharing cannot resume it; use trusted user recovery controls.

If the action shows `!` / **This tab cannot be shared**, injection failed or the
page is unsupported. Connection/identity failures can instead clear the dot and
restore the share tooltip. Check the installed matching release, native host
registration and browser page restrictions. With the current consent-only native
entry, the dot can appear without any DOM request being available; do not loosen
identity or approval checks to enable it.

<a id="revoke"></a>
## Revoke sharing

Select the shared tab and click the toolbar action again (**Stop sharing this
tab**). The dot clears and the tooltip returns to **Share this tab**. This clears
the share's content connection, pending requests and cached DOM handles; no new
extension read or action can use that share.

Navigation in any frame, reload, history-state or fragment change, tab
close/replacement, content/native disconnection and extension worker restart also
clear sharing. To share the resulting page again, explicitly click the toolbar
action again. There is no persistent site grant or automatic sharing recovery.
Browser suspension/host idle shutdown can end sharing even if the page stays open.

To stop all Computer Use delivery, use its trusted **Stop** control as well.
Revoking the DOM share does not revoke an existing browser App approval: where
declared available, gcu can still use the browser's L0 accessibility route. To
remove persistent browser approval, use the host's authorization settings, or
`gcu approvals list` followed by `gcu approvals revoke KEY` for the CLI host
([CLI authorization controls](cli.md)). Sharing cannot override a Stop latch.

Revocation does not undo an already delivered action or delete results already
passed to gcu, a client or a model provider. Use their own session/deletion
controls for those copies.

<a id="uninstall"></a>
## Disable or uninstall

1. Stop the Computer Use session and revoke each shared tab as described above.
2. Open `chrome://extensions` or `edge://extensions`. Turn off **Grok Computer Use**
   to disable it, or choose **Remove** to uninstall it. Repeat in every browser
   profile where you installed it. Re-enabling starts without a saved share.
3. To remove the local host registration for both browsers, preview and run:

   ```sh
   gcu uninstall chrome-extension --dry-run
   gcu uninstall chrome-extension
   ```

   Use the same `--home` override if you installed into a disposable root. Removal
   is idempotent and remains available when the extension key is missing. It
   removes owned host manifests/Windows registry values; it preserves foreign
   registrations and makes owner-only backups of changed owned manifests. These
   registration backups contain configuration, not page content. Removing the
   extension does not itself unregister the native host, and unregistering the
   host does not uninstall extensions from browser profiles.

Neither operation deletes gcu sessions, other client configuration, downstream
results or persistent App approvals. Manage those through their own controls.

<a id="proof"></a>
## Proof of identity and window mapping

A4a records whether a native-host connection may use L2. It does not open a
production DOM channel. The rules below describe the private proof validator
when supplied with process/window evidence, not a connected production broker.
The current consent-only `native-host` entry checks the fixed origin and framed
protocol; it does not invoke that full OS launch-chain validator.
Refusal leaves the browser on its already declared L0 accessibility route. A
refusal is not approval of that L0 action, and a DOM mutation that already
started is `unknown` rather than a second attempt.

Identity:

- The only extension ID is the one derived from the public `key` in the
  extension manifest that is compiled into gcu: `jgpkhdjlddaoopklipcnpcjcpiancmff`.
  The origin is `chrome-extension://jgpkhdjlddaoopklipcnpcjcpiancmff/`.
- Host manifest templates keep the placeholder origin
  `chrome-extension://__MAINTAINER_EXTENSION_ID_FROM_MANIFEST_KEY__/`.
  `gcu install chrome-extension` replaces `allowed_origins` with the embedded
  origin and nothing else. A missing key, the placeholder key, a different key,
  or a different origin fails closed.
- The proof validator requires the parent to be that platform's Chrome or Edge executable, and the
  argv must be the native-messaging launch (`native-host` plus the origin, or
  the origin alone). On Windows only, a single `--parent-window=<hwnd>` suffix
  is accepted as the foreground window at launch. Any other parent, including a
  local relay, fails closed. The gcu image path and build id must be the
  expected build. Same-user control of the browser itself is outside this check.

Session binding:

- L2 would require exactly one listening engine and exactly one authorized local
  session for that engine and epoch. Several engines, several authorized
  sessions, or a session for a different window are refused. Title similarity
  and geometry similarity are ignored. There is no UI here to pick among
  sessions, so those cases stay off L2.

Top document to OS window, reviewed without a live browser attached:

| Platform | Browser | What native messaging actually provides | Verdict |
|---|---|---|---|
| Windows | Chrome, Edge | Parent image, plus `--parent-window` as the foreground HWND at the moment the host starts. Extension `tabId` / `windowId` are not HWNDs. One host connection is not one tab. | L2 unavailable. Fall back to declared L0. |
| Linux X11 | Chrome, Edge | Parent image only. No XID is passed. `_NET_WM_PID` names the browser process, which owns every window. | L2 unavailable. Fall back to declared L0. |
| Linux Wayland | Chrome, Edge | Parent image only. No authenticated toplevel handle is passed. | L2 unavailable. Fall back to declared L0. |
| macOS | Chrome, Edge | Parent image only. No CGWindowID is passed. Extension window ids are not OS window ids. | L2 unavailable. Fall back to declared L0. |

A single visible window, a matching HWND, or a high title/geometry score does
not identify the document. No supported binding was established by A4a.
ADR 0014 was exited and is not a mechanism;
this guide neither adopts a mechanism nor promises delivery. A change of
Host/ACP trust boundaries requires an accepted ADR and fresh evidence before
any platform can admit the production DOM channel. The existing [ADR 0012](adr/0012-real-driver-assembly.md)
records that requirement; it does not enable L2.

These checks were not run on a real browser. Xvfb, fake drivers, and in-process
fixtures do not fill the gap:

| Evidence | Status |
|---|---|
| Linux X11 Chrome native-messaging launch chain | not_run |
| Linux X11 Edge native-messaging launch chain | not_run |
| Linux Wayland Chrome native-messaging launch chain | not_run |
| Linux Wayland Edge native-messaging launch chain | not_run |
| Windows Chrome native-messaging launch chain | not_run |
| Windows Edge native-messaging launch chain | not_run |
| macOS Chrome native-messaging launch chain | not_run |
| macOS Edge native-messaging launch chain | not_run |
| Linux X11 top document to X11 window | not_run |
| Linux Wayland top document to Wayland window | not_run |
| Windows top document to HWND | not_run |
| macOS top document to OS window | not_run |

## Privacy, permissions and capability limits

The extension requests only the four permissions in the
[complete permission inventory](../tools/browser-extension/store/permissions.md).
It does not read cookies, browser/page storage or the clipboard, and it does not
capture screenshots. Password-field values are masked without being read;
hidden-input values are omitted. The tested DOM representation can contain
private page text,
ordinary form values and offscreen content; no production gcu session currently
receives it through L2. Share only what you intend the session to process.

The existing extension transport is native messaging to local gcu, currently
consent-only at the native entry. The extension does not upload data or send
telemetry. Your selected MCP client/model may handle
tool results remotely under its own settings and policies. The extension keeps
shares and DOM handles in memory, without persistent browser storage. Read the
[policy text](../tools/browser-extension/store/privacy-policy.md) and
[data use disclosure](../tools/browser-extension/store/data-use.md). The published
policy location is pending: `__MAINTAINER_PRIVACY_POLICY_URL__`.

The core fusion code is tested with fake DomChannel implementations: it merges
DOM only when extension, shared document and web-area frame/fingerprint align.
These checks cannot establish the missing production document/window binding,
so they do not enable L2. When absent or mismatched, the browser uses its declared L0 accessibility capabilities.
Readable same-origin frames and open shadow roots are supported; cross-origin or
transformed frames, opaque custom elements, closed shadow roots, canvas and
execution interfaces can limit observation or require user takeover. Execution
interface detection is best effort, not complete. DOM does not authorize arbitrary
scripts, keyboard input, navigation or a bypass of the ActionRouter.

gcu filters its own address-bar input, dangerous schemes and metadata-host
requests. L0 cannot prevent a page from redirecting itself; the extension's
navigation permission clears shares and is not a redirect blocker. See the
[browser fusion safety contract](../crates/gcu-core/src/browser/mod.rs) and
[extension representation and limits](../tools/browser-extension/README.md#dom-representation-and-limits).
