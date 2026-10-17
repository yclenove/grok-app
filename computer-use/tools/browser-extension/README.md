# Computer Use v2 DOM extension (P8-01)

This MV3 extension shares only the tab whose browser toolbar icon the user
clicks. A second click revokes sharing. The localized tooltip changes between
share/revoke; a dot marks a shared tab and an exclamation mark marks injection
failure. There is no popup, pairing, lease, HTTP server/client, storage access,
cookie access, clipboard access, page message bridge, or arbitrary evaluation.

**DOM delivery remains disabled in this card.** `installBackground` receives no
`TrustedRouterChannel` in the production entry point. Sharing does not read the
tree or perform an action. P8-02 must implement/authenticate the native messaging
adapter; P8-03 must connect it to the complete ActionRouter and merge the tree.
This is not yet an enabled end-to-end Computer Use feature.

## Permissions (fixed allowlist)

| Manifest permission | Reason |
| --- | --- |
| `activeTab` | Temporary access to the tab selected by the user's explicit toolbar click; no persistent site access. |
| `scripting` | Inject the bundled `content.js` into that tab's top frame in the isolated world after the click. |
| `webNavigation` | Revoke sharing on navigation in any frame, including history/fragment changes, before another request can use the old scope. Events for unshared tabs are ignored; no browsing history is retained. |

No `host_permissions`, `<all_urls>`, static `content_scripts`, `tabs`, `storage`,
`cookies`, `nativeMessaging`, or `externally_connectable` permissions are present.
Tab close/update/replacement events need no `tabs` permission. P8-02 is responsible
for the separately reviewed native messaging permission, fixed extension identity,
host installation, and narrowly scoped authenticated routing. MV3 CSP prohibits
remote scripts, evaluation and network connections.

## Trusted message boundary

`TrustedRouterChannel` is a background-module dependency, not a callable page or
runtime endpoint. `listen` supplies requests only from the authenticated gcu
connection. `authorize(request, scope)` must verify the live connection, bound
session, one-shot request, complete Router approval/preflight, and persistent Stop.
The scope's tab/document IDs come from Chrome injection results, never the page or
model. `shared` publishes only those browser-owned IDs and a random share ID;
sharing never grants App/action approval or resumes Stop.

The adapter must call `onInvalidate` immediately on Stop, session end, native
disconnect or loss of Stop health. This clears scopes and disconnects content
ports without waiting for ordinary requests. There is no resume operation in the
DOM protocol. A newly connected adapter still checks the Host's persistent Stop;
re-sharing cannot clear it. Requests must not be buffered/replayed across a
disconnect. P8-02/P8-03 must verify actual transport cancellation/Stop timing before
enabling delivery; the synchronous fixture channel is not evidence of those limits.

The background uses a private `tabs.connect` port pinned to Chrome's document ID.
Content accepts only ports from this extension without a tab sender. A page,
another extension, or a content script in another tab cannot issue DOM requests.
Only `{observe, click, set_value, scroll, select}` are accepted; no selectors, script
strings, keyboard input, navigation or approvals are accepted. Requests are bound
to the share and session, bounded, serialized, timed out and deduplicated. Actions
consume the observation before dispatch. Failed transport after dispatch returns
`unknown`; it is never automatically retried. Page exception text and action values
are never logged or included in errors.

Revocation, any-frame navigation, tab close/replacement, port disconnect, and worker
restart invalidate the share. Content also clears immediately on `pagehide` (including
same-origin child documents) and iframe load. Late injection/replies cannot recreate
a cleared share. Worker memory and content handles are never persisted.

## DOM representation and limits

Nodes use core role names, tagged `NodeValue`, state names and `Rect {x,y,w,h}`.
Opaque node IDs only identify an exact element in one observation. Before delivery,
the adapter verifies the same element, ancestry, role/name/value/states/frame,
document chain and viewport geometry, and scans again for execution interfaces.
Mutation records also invalidate observations when attributes are changed and
then restored, including inside open shadow roots and same-origin frames.
It never substitutes a similarly named element. `done` means semantic delivery,
not business success; interrupted/uncertain delivery is `unknown`.

Frames are explicitly **tab viewport physical pixels**, with width/height/scale
metadata. P8-03 must bind the real browser window, map the viewport origin to its
window/screenshot coordinate space and validate frame/fingerprint alignment before
creating unified `RawNode` frames. These frames must not be treated as already
window-relative. This card changes no core/Driver/Host protocol interfaces.

Same-origin frames are traversed with border offsets; open shadow trees preserve
their document scope and exact element handles. Cross-origin/unreadable frames,
transformed frames and opaque custom elements report incomplete observation and
force action handoff. Native closed shadow roots cannot be inspected; no claim of
complete closed-shadow support is made. Canvas/object/embed and terminal/console/
remote execution/editor signals force handoff for every mutation, including
ordinary-looking controls elsewhere in a mixed interface. Recognition is best
effort, not a complete detector. DOM observations never authorize execution.

Traversal is bounded to 5000 elements, depth 64 and two seconds. Incompleteness is
explicit and prevents mutations. Password values are `•••` without reading the
value getter; hidden input values are omitted. No screenshots or image derivatives
are produced. P8-03 must preserve incompleteness and run core's redaction pipeline
before delivering screenshots, including those from the native browser driver.

All extension UI copy lives in `_locales`, with 15 catalogs locked to English by
the DOM test. Chrome locale directory aliases are `tl` (fil), `pt_BR` (pt-BR),
`zh_CN` (zh), and `zh_TW` (zh-TW).

## Build and checks

From the repository root, using existing dependencies (no new runtime/dependencies):

```sh
node computer-use/tools/browser-extension/build.mjs
pnpm exec tsc --noEmit -p computer-use/tools/browser-extension/tsconfig.json
pnpm exec vitest run computer-use/tools/browser-extension/tests/dom.test.ts
```

The build emits a self-contained ES module worker, an IIFE content script,
manifest and locales into the ignored `dist/` directory. Load that directory as
an unpacked extension only in a disposable browser profile/fixture. No real
accounts are needed. Protocol compatibility is checked separately by
`rcargo test -p gcu-protocol` on the authorized build machine.

The DOM extractor/executor was reviewed against
`archive/cu-v1-2026-10-13:tools/computer-use-extension/{observe,act}-document.mjs`.
No v1 transport, pairing, lease, storage, or recovery subsystem is copied.
