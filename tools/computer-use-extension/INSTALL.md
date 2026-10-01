# Install Grok Computer Use pairing (development)

Version: 1.1.0

This source extension pairs with Grok App and explicitly shares the current tab
as an App picker candidate. Model observation/actions on existing tabs are still
unfinished. It is not yet distributed with the installer.

1. From the repo root run `node scripts/prepare-computer-use-extension.mjs`
   to generate the 15 extension locale catalogs from the App catalogs.
2. In a development Chrome/Edge profile open `chrome://extensions` or
   `edge://extensions`, enable Developer mode, and Load unpacked this folder.
3. Verify the source extension ID is `bgegbabkegkanjbmjbeaockdijnbkjgi`.
   The manifest public key stabilizes this ID; it does not attest source integrity.
4. In Grok App explicitly enable Computer Use. Open Computer, click Pair browser
   extension, review the extension ID, then Confirm pairing in App.
5. Open the extension popup. Copy the displayed `http://127.0.0.1:PORT` App
   address and one-time code into its two fields, then click Pair. Codes expire
   in five minutes. Never include a code/token in the address.
6. On an HTTP(S) page, open Grok Computer Use from the browser's Extensions menu
   or pinned toolbar button, then click Share current tab. Opening `popup.html`
   as an ordinary tab does not grant site access. Only the chosen tab becomes a
   candidate; pairing does not share all open tabs. Restricted browser pages
   cannot be shared. App authorization and model control are separate steps;
   the production control adapter is not available yet.
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
terminal receipt is briefly stored in extension local storage so a worker can
read it even if navigation drops the runtime message. The corresponding Host
proof stays in trusted session storage. Receipts contain no action text, page
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
