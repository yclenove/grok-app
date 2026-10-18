# Chrome Web Store listing draft

Maintainer submission only. Resolve the [submission checklist](README.md#maintainer-inputs-and-release-checklist)
before copying this draft into a live listing. No listing ID or approval exists
in this kit.

## Name

Grok Computer Use

## Short description

Share a tab with local gcu for authorized Computer Use DOM requests.

## Single purpose

Provide a user-shared tab's DOM to local Grok Computer Use (gcu), so an authorized
Computer Use session can observe and interact with that exact document.

## Detailed description

Grok Computer Use is an optional DOM companion for the locally installed gcu
application. Select a web tab and click the extension's toolbar action to share
that tab. Sharing alone does not read the DOM or approve an action. With a
compatible, connected gcu session and the required approvals, the extension can
return structured page content and perform semantic clicks, field-value changes,
scrolling and option selection in the shared document.

Click the toolbar action again to stop sharing. Navigation (including frame,
history and fragment changes), tab closure, extension worker restart or connection
loss clears the share. A new share requires another user click. Sharing never
resumes a stopped Computer Use session.

The extension does not read cookies or storage. Page data goes only through native
messaging to gcu on your computer. The extension has no data upload, advertising,
analytics or telemetry service. Your selected gcu client or model service may
process returned tool results under its own settings and policies; the extension
does not make the whole Computer Use workflow offline.

DOM support includes readable same-origin frames and open shadow roots. Opaque
frames, closed shadow roots and execution interfaces can limit observation or
require you to take over. The extension provides no arbitrary script execution,
keyboard/navigation API or redirect blocker. gcu's safety filters apply to its
own requests; L0 accessibility cannot prevent a page's own redirects. A shared
badge indicates browser consent, not that a task succeeded or dispatch is ready.

## Reviewer references and form fields

- [Every requested permission and its justification](permissions.md).
- [Data use and privacy questionnaire guidance](data-use.md).
- [Privacy policy text](privacy-policy.md); hosted policy URL: `__MAINTAINER_PRIVACY_POLICY_URL__`.
- [Install](../../../docs/browser-extension.md#install), [share](../../../docs/browser-extension.md#share), [revoke](../../../docs/browser-extension.md#revoke), [uninstall](../../../docs/browser-extension.md#uninstall).

Developer account: `__MAINTAINER_CHROME_WEB_STORE_DEVELOPER_ACCOUNT__`.
Fixed native identity: `__MAINTAINER_EXTENSION_ID_FROM_MANIFEST_KEY__`.
Listing URL: `__MAINTAINER_CHROME_WEB_STORE_LISTING_URL__`.
Support URL: `__MAINTAINER_SUPPORT_URL__`.
Screenshots: `__MAINTAINER_CHROME_WEB_STORE_SCREENSHOTS__`.

For review, the maintainer must supply the matching gcu release and demonstrate
the guide on a disposable fixture page. The checked-in native host entry currently
acknowledges consent only, and its manifest lacks the fixed public key. These
release prerequisites are listed in the checklist; no working production DOM
dispatch, store submission or reviewer acceptance is asserted by this draft.
