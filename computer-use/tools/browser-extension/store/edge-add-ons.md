# Microsoft Edge Add-ons listing draft

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

Grok Computer Use adds optional DOM access for a tab you explicitly share with
the locally installed gcu application. Select an HTTP or HTTPS page and click the
extension's toolbar action. A compatible gcu session must still obtain the normal
Computer Use approvals before observing page content or requesting semantic
clicks, field-value changes, scrolling or option selection. Sharing by itself
does not read the DOM, approve actions or resume a stopped session.

Use the toolbar action again to stop sharing. Navigation in any frame, history
or fragment changes, tab closure, worker restart and native connection loss clear
the share. Sharing a different tab requires another explicit user click.

The extension does not read cookies or storage and does not upload page content.
It uses native messaging only to exchange shared-tab data and authorized requests
with gcu on your computer. It has no advertising, analytics or telemetry service.
Tool results can subsequently be processed by the gcu client/model you choose,
under that service's settings and policies.

Readable same-origin frames and open shadow roots can improve DOM observations.
Unreadable frames, closed shadow roots and execution interfaces have limits and
may require user takeover. This is not a general script runner or redirect
blocker. gcu filters its own input; L0 accessibility does not stop a page's own
redirects. The share badge indicates consent, not operational readiness or task
success.

## Reviewer references and form fields

- [Every requested permission and its justification](permissions.md).
- [Data use and privacy questionnaire guidance](data-use.md).
- [Privacy policy text](privacy-policy.md); hosted policy URL: `__MAINTAINER_PRIVACY_POLICY_URL__`.
- [Install](../../../docs/browser-extension.md#install), [share](../../../docs/browser-extension.md#share), [revoke](../../../docs/browser-extension.md#revoke), [uninstall](../../../docs/browser-extension.md#uninstall).

Developer account: `__MAINTAINER_EDGE_ADD_ONS_DEVELOPER_ACCOUNT__`.
Fixed native identity: `jgpkhdjlddaoopklipcnpcjcpiancmff`.
Listing URL: `__MAINTAINER_EDGE_ADD_ONS_LISTING_URL__`.
Support URL: `__MAINTAINER_SUPPORT_URL__`.
Screenshots: `__MAINTAINER_EDGE_ADD_ONS_SCREENSHOTS__`.

The maintainer must provide the matching gcu release and a disposable fixture
profile for review. The checked-in native host entry acknowledges consent only,
and the manifest has no fixed public key. Follow the checklist's identity and
dispatch prerequisites before publishing. This draft does not assert working
production DOM dispatch or a successful store review.
