# Install the Grok Computer Use extension (experimental)

Version: 1.1.0

Grok App embeds a matching, fixed production file set in its executable. Open
Extension setup in App, choose Get bundled extension, and copy/open the exported
folder. This only writes an App-owned folder; it does not install into any browser,
start a task, pair or grant access. Repeated exports reuse only an exact unchanged
bundle. Modified/unowned destinations are rejected, never overwritten. An
interrupted private staging directory is retained; explicit retry uses a fresh
staging identity. Export is not proof of installed-browser or full task acceptance.

For source development only, regenerate the catalogs as in step 1. Packaged App
users do not need Node, the repository, a download URL or a store listing. Existing
tab task control remains gated pending the complete App/ACP/MV3 acceptance chain.

1. From the repo root run `node scripts/prepare-computer-use-extension.mjs`
   to generate the 15 extension locale catalogs from the App catalogs.
2. In the Chrome/Edge profile you choose, open `chrome://extensions` or
   `edge://extensions`, enable Developer mode, and Load unpacked the exported
   folder (or this source folder when developing). Chrome/Edge 140+ is required.
   The persistent browser marker and cleanup journal require
   `storage.local.setAccessLevel({accessLevel: "TRUSTED_CONTEXTS"})`, which the
   older bundled Chromium 130 does not expose. Older browsers are rejected at
   installation; do not skip this protection or expose Host proofs to page code.
3. Verify the source extension ID is `bgegbabkegkanjbmjbeaockdijnbkjgi`.
   The manifest public key stabilizes this ID; it does not attest source integrity.
4. For an offered task, choose its shared-tab scope and explicitly Confirm pairing
   in App. The older Computer panel also offers explicit Pair browser extension.
   Neither step grants model access to unselected tabs.
5. Open the extension popup. Copy the displayed `http://127.0.0.1:PORT` App
   address and one-time code into its two fields, then click Pair. Codes expire
   in five minutes. Never include a code/token in the address.
6. On an HTTP(S) page, open Grok Computer Use from the browser's Extensions menu
   or pinned toolbar button, then click Share current tab. Opening `popup.html`
   as an ordinary tab does not grant site access. Only the chosen tab becomes a
   candidate; pairing does not share all open tabs. Restricted browser pages
   cannot be shared. App authorization and model control are separate steps;
   task control is still gated pending joint acceptance.
7. Stop sharing this tab removes its candidate and grants without closing it.
   Reload/navigation also removes the candidate, including reloads to the same
   URL. Share again explicitly after navigation; an old picker row cannot
   authorize the replacement document.
8. To stop pairing, use Unpair in the popup or Revoke pairing in App.
   Reloading the extension/worker requires a new pairing. Browser restart does
   not restore its key. The worker renews a 30-second Host lease without keeping
   the popup open. A browser crash or suspended worker stops renewal; expired
   access is rejected and cleared automatically. Use App Revoke for immediate removal.

Use trusted source only. No ordinary web page receives the code; it is used in
the App and trusted extension popup. No Cookie/Token export is performed.
Do not share code screenshots or credential-bearing debug output.

The development extension also requests `webNavigation` to check the original
admitted document before cleanup. It queries only document IDs involved in
sharing and cleanup; it does not enumerate browsing history or all tabs.
A missing result can mean back/forward cache, so neither a missing result nor
a permission error proves that a document was destroyed.

Before an experimental action is claimed, a fixed isolated-world guardian is
installed in that document. On a trusted non-cached page departure it retires
execution and waits for the original operation to finish. A one-use random
terminal receipt is briefly stored in extension session storage so a worker can
read it even if navigation drops the runtime message. The corresponding Host
proof stays in trusted local storage. Receipts contain no action text, page
content, cookies or pairing credentials; consumed and orphan receipts are
removed. A cached document keeps its occupancy until its original completion
can be confirmed. This does not provide full browser/App crash recovery.

For automated verification use a fresh disposable Chromium profile via
`tools/computer-use-probe/pairing-live.mjs`; never use a daily browser profile.
The App probe's `existing-tab-extension-toolbar` gate launches a separate owned
browser and waits for real toolbar Share/Unshare clicks on local fixture pages.
No account, Internet page, daily profile or permission override is needed.
Chrome-for-Testing local evidence does not establish source Edge, installed
Chrome/Edge or macOS/Linux acceptance.
