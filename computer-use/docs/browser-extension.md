# Browser DOM extension

The optional Grok Computer Use MV3 extension adds DOM observations and semantic
actions for a tab you explicitly share with local gcu. It uses your existing
Chrome or Microsoft Edge browser; it does not install a browser runtime. DOM
actions support clicks, field-value changes, scrolling and option selection,
subject to the same App/action approvals and Stop as other Computer Use routes.

This is a pre-publication guide. The checked-in manifest has no fixed public
`key`, so native registration/startup refuses to proceed. The current native-host
executable acknowledges sharing only; its trusted session/window binding and DOM
dispatch must be assembled and verified in the release. A share badge alone does
not demonstrate that DOM observation or control is operational. See the
[maintainer release checklist](../tools/browser-extension/store/README.md#maintainer-inputs-and-release-checklist).

<a id="install"></a>
## Install

1. Obtain the maintainer's matching gcu and extension release. The extension
   requires Chromium's document-ID APIs (Chrome minimum 106); Edge support must be
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
5. Pin the extension's toolbar action using the browser's extensions menu. Start
   your compatible local Computer Use session and authorize the browser when
   prompted. Extension installation does not grant gcu access to browser content.

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
3. Request an observation in the compatible gcu session. DOM content is read only
   after sharing and an authorized request; sharing by itself does not read the
   tree or execute an action. Normal App/action approvals still apply. If gcu is
   stopped, sharing cannot resume it; use the trusted user Stop/recovery controls.

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

## Privacy, permissions and capability limits

The extension requests only the four permissions in the
[complete permission inventory](../tools/browser-extension/store/permissions.md).
It does not read cookies, browser/page storage or the clipboard, and it does not
capture screenshots. Password-field values are masked without being read;
hidden-input values are omitted. Shared DOM can still contain private page text,
ordinary form values and offscreen content. Share only what you intend the
session to process.

Shared-tab data travels only through native messaging to local gcu. The extension
does not upload data or send telemetry. Your selected MCP client/model may handle
tool results remotely under its own settings and policies. The extension keeps
shares and DOM handles in memory, without persistent browser storage. Read the
[policy text](../tools/browser-extension/store/privacy-policy.md) and
[data use disclosure](../tools/browser-extension/store/data-use.md). The published
policy location is pending: `__MAINTAINER_PRIVACY_POLICY_URL__`.

L2 merges DOM into a browser window only when the authenticated extension,
shared document and accessibility web-area frame/fingerprint align. When absent
or mismatched, the browser uses its declared L0 accessibility capabilities.
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
