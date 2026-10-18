# Grok Computer Use browser extension privacy policy

Effective date: `__MAINTAINER_PRIVACY_POLICY_EFFECTIVE_DATE__`

Publisher / data controller: `__MAINTAINER_PUBLISHER_LEGAL_NAME__`

Privacy contact: `__MAINTAINER_PRIVACY_CONTACT__`

Publication URL: `__MAINTAINER_PRIVACY_POLICY_URL__`

This policy covers the Grok Computer Use browser extension for Chrome and Edge.
Its purpose is to connect a tab you explicitly share to the locally installed
Grok Computer Use application (gcu) for authorized observation and interaction.

## What the extension accesses

The extension reads a tab's DOM only after you click its toolbar action to share
that selected tab and gcu sends an authorized observation request. Sharing alone
does not read the DOM or perform actions. Other tabs are not automatically shared,
and changing the selected tab does not grant new access.

Shared observations can contain page text, element names/roles, non-password
form values, states and element positions. Readable same-origin frames, open
shadow roots and offscreen DOM may be included. The extension checks document
URLs locally to validate shares and identify execution interfaces, and uses
browser tab/navigation events to invalidate shares. It does not retain a browsing
history or record your keystrokes. Authorized action messages can contain values
you or your session supply for field changes or option selection.

The extension does not read cookies, local storage, session storage, IndexedDB,
browser credentials or clipboard contents. Password fields are masked without
reading their values, and hidden-input values are omitted. It does not take
screenshots. Other page text and ordinary fields can contain personal, health,
financial, communication, location or authentication information; masking
password fields does not guarantee that all sensitive content is removed.

## How information is used and transmitted

Information is used only to provide the authorized Computer Use interaction in
the document you shared. The extension sends shared-tab data only through the
browser's native messaging pipe to the installed gcu host on your computer. The
extension does not upload content to a server and does not operate advertising,
analytics or telemetry. Information is not sold or used for unrelated purposes,
advertising or creditworthiness decisions.

The local gcu application may pass tool results to the MCP client and model
provider you selected. Those applications/services have their own policies and
may process data remotely. This policy describes the extension's local transport
and does not promise that your entire Computer Use workflow stays on-device.
Browser vendors' extension distribution/update services operate under their own
policies as well.

## Retention and deletion

The extension stores share scopes, DOM snapshots/handles and pending requests
only in volatile memory; it does not save page content in browser or extension
storage. Revoking a share, navigating (including frame/history/fragment changes),
closing/replacing the tab, losing the content/native connection or restarting the
extension worker clears the affected sharing state and cached handles. Errors
use codes and do not log page text or action values.

Revocation stops future extension reads/actions for that share. It cannot undo an
action already delivered or erase data already received by gcu, your client or a
model service. Use those applications' session/deletion controls and policies
for their copies. Removing the extension ends its access.

## Your choices

Click the toolbar action again to stop sharing the tab. You can also disable or
remove the extension in your browser's extensions manager and remove the local
native host registration. A later share requires a new user click; it does not
resume a stopped Computer Use session or substitute for App/action approval.

The full installation, sharing, revocation and removal guide is available at
`__MAINTAINER_USER_GUIDE_URL__` (sections `#install`, `#share`, `#revoke` and
`#uninstall`). Share only content you intend to provide to your selected Computer
Use session.

## Policy updates and contact

The publisher will update this policy and its effective date when the extension's
data handling changes. For questions about this extension's handling of your
information, contact `__MAINTAINER_PRIVACY_CONTACT__`.
