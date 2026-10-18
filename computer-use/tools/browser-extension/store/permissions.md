# Extension permission justifications

This is the complete permission list for both Chrome Web Store and Edge Add-ons.
It must match `permissions` in the [source manifest](../manifest.json) and the
packaged `dist/manifest.json`. The tests read the rows below; adding or removing
a manifest permission without updating this disclosure fails validation.

| Manifest permission | Purpose and scope |
| --- | --- |
| `activeTab` | Temporarily access the tab selected by the user's explicit toolbar click. Only the current selected tab is shared by that click; selecting another tab does not share it. There is no persistent site access or all-sites grant. |
| `scripting` | Inject the bundled content script into that shared tab's top frame in the browser's isolated world after the user click. The script handles authorized DOM observations and semantic actions; it does not run arbitrary code or remote scripts. |
| `webNavigation` | Clear an existing share when any frame navigates, including history-state and fragment changes, so the previous document's scope cannot be reused. Browser navigation events for unshared tabs are ignored; the extension does not build or retain browsing history. This permission observes lifecycle events and does not block page redirects. |
| `nativeMessaging` | Connect the extension background worker to the installed local `ai.grok.computer_use` gcu host. Shared-tab DOM responses and authorized action messages use only this browser-managed local pipe, without a network upload, HTTP control server or telemetry endpoint. |

## Permissions and access not requested

`host_permissions`, `optional_host_permissions`, `optional_permissions` and
static `content_scripts` are absent. There is no `<all_urls>`, `tabs`, `cookies`,
`storage`, `history`, `clipboardRead`, `clipboardWrite`, `debugger` or
`externally_connectable` access. There is no optional request for wider access.
The extension does not read cookies, local/session storage, IndexedDB or browser
credential stores. It does not capture screenshots or clipboard data.

Browser tab lifecycle APIs work without the `tabs` permission. Tab IDs are used
to clear or refresh share indicators; this does not authorize reading those tabs'
DOM. Only a toolbar click can create a share. A new authorized observe request is
required to read DOM content, even after sharing.

## Store form reuse

Use each row's justification for the corresponding permission in both stores.
For host-permission and optional-permission questions, answer that none are
requested. For remote code questions, answer that all scripts are bundled and
the MV3 extension-page CSP is `script-src 'self'; object-src 'none'; connect-src
'none'`. No arbitrary evaluation or downloaded executable code is used.

See [data use](data-use.md), [privacy policy](privacy-policy.md) and the
[user guide](../../../docs/browser-extension.md).
