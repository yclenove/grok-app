# Extension data use disclosure

Use with both stores' current privacy forms and the [privacy policy](privacy-policy.md).
The scope here is the browser extension and its local native messaging boundary;
the separate gcu application, MCP client and model provider have their own data
handling. Maintainers must review the actual release and current form wording
before submitting these answers.

## Single purpose and data flow

The single purpose is to provide a user-shared document's DOM to local gcu for
authorized Computer Use observation and interaction.

1. The user clicks the browser toolbar action on the selected HTTP(S) tab.
2. The extension injects only its bundled isolated content script. It keeps the
   tab/document/share identifiers and document URL in memory to validate the scope;
   sharing alone does not traverse the DOM or perform an action.
3. Only an authenticated native request bound to an approved gcu session can read
   that shared document or request a supported action. Snapshot data includes
   element roles, accessible names/text, non-password field values, states,
   viewport frames and observation-completeness metadata. Action traffic includes
   supplied field/option values, target identifiers and results.
4. Responses travel only through browser native messaging to the local
   `ai.grok.computer_use` gcu host. The extension makes no network upload or
   telemetry request, runs no HTTP control server and has no page-callable bridge.
   It does not collect screenshots, cookies, browser/page storage or clipboard data.
5. Local gcu can return observations to the user's selected MCP client/model.
   That downstream handling can include a remote model service; do not claim the
   entire workflow keeps content on-device merely because this extension's
   transport is local.

## Data categories for privacy forms

Declare website-content processing and the local native application recipient.
Do not choose a blanket “no data use/collection” answer based solely on the absence
of an extension-operated server. Where a form lists these categories, use the
following conservative scope; potentially sensitive page text is still content
even when it was shared voluntarily.

| Category | Scope of processing |
| --- | --- |
| Website content | Yes: the explicitly shared document's structured DOM, labels, text and non-password control values, including readable same-origin child frames and open shadow roots. Offscreen DOM can be included; this is not limited to visible pixels. |
| User activity | Only explicit share/revoke actions and authorized DOM action requests/results. No passive keystroke recording, continuous activity analytics or browsing profile. |
| Web history | The selected document URL is checked locally for share validity and execution-surface detection; browser navigation events clear shares. No history API access or stored browsing-history list. Page text may itself contain URLs. |
| Personally identifiable information | May appear in shared page text or ordinary form values. No separate identity/profile collection or sale. |
| Health information | May appear in shared page content; no separate health-data integration or collection API. |
| Financial and payment information | May appear in shared page content or action values; no separate payment integration or account access. |
| Personal communications | May appear as shared page content; no separate messaging integration or mailbox collection. |
| Location | May appear as shared page content; no geolocation permission or location tracking. |
| Authentication information | No cookie, credential-store or storage reads. Password fields are represented as `•••` without reading their values; hidden-input values are omitted. Secrets exposed as ordinary page text or supplied action values can still be sensitive content, so do not promise complete secret detection. |

Only share content you intend your Computer Use session to process. The extension
does not provide an anonymization guarantee. Any choices required for incidental
sensitive website content in the live privacy form must reflect this disclosed
scope, rather than incorrectly asserting that such content cannot be present.

## Purpose, recipients and retention

Processing is solely for the user-requested Computer Use session. Data is not
sold, used for ads, used to determine creditworthiness or lent to unrelated
features. There is no extension-operated analytics, telemetry or remote data
storage. All executable code is bundled; no remote code or arbitrary evaluation
is used. Reuse the [permission reasons](permissions.md) for both stores.

The extension keeps consent, DOM handles, snapshots and pending requests in
volatile memory. It writes no content to extension/browser storage and retains no
browsing history. Revocation/navigation, tab close/replacement, content/native
disconnect or worker restart clears the affected share and cached handles.
Previously delivered data is not recalled from gcu or a downstream client/model;
their retention/deletion policies apply separately. Extension errors contain
codes, not page text or action values, and no page payloads are logged by the
extension.

## User control

The toolbar tooltip switches from “Share this tab” to “Stop sharing this tab”
(localized in the extension). A dot marks browser consent. The user can revoke
with another click, close/navigate the tab, disable/remove the extension and
uninstall the native host registration. No automatic reconnect restores consent;
a user click is required again. Sharing grants neither App/action approval nor
permission to resume Stop. Follow [share](../../../docs/browser-extension.md#share),
[revoke](../../../docs/browser-extension.md#revoke) and
[uninstall](../../../docs/browser-extension.md#uninstall).
