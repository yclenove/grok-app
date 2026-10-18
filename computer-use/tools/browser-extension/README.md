# Computer Use v2 DOM extension

See the [user guide](../../docs/browser-extension.md) for installation, sharing,
revocation and removal, and the [store submission kit](store/README.md) for
maintainer publication prerequisites, permissions and privacy disclosures.

This MV3 extension shares only the tab whose browser toolbar icon the user
clicks. A second click revokes sharing. The localized tooltip changes between
share/revoke; a dot marks a shared tab and an exclamation mark marks injection
failure. There is no popup, pairing, lease, HTTP server/client, storage access,
cookie access, clipboard access, page message bridge, or arbitrary evaluation.

The production background uses `NativeChannel` to announce browser consent to
`gcu native-host`. The native host entry point has no session binding or DOM
dispatch assembly: P8-03 must connect the transport to the complete ActionRouter
and merge the tree. Sharing alone never reads the tree or performs an action.

## Permissions (fixed allowlist)

| Manifest permission | Reason |
| --- | --- |
| `activeTab` | Temporary access to the tab selected by the user's explicit toolbar click; no persistent site access. |
| `scripting` | Inject the bundled `content.js` into that tab's top frame in the isolated world after the click. |
| `webNavigation` | Revoke sharing on navigation in any frame, including history/fragment changes, before another request can use the old scope. Events for unshared tabs are ignored; no browsing history is retained. |
| `nativeMessaging` | Connect only to the installed `ai.grok.computer_use` host; transport has no page-callable bridge, HTTP listener or pairing server. |

No `host_permissions`, `<all_urls>`, static `content_scripts`, `tabs`, `storage`,
`cookies`, or `externally_connectable` permissions are present.
Tab close/update/replacement events need no `tabs` permission. MV3 CSP prohibits
remote scripts, evaluation and network connections.

## Fixed identity and installation

**Fixed identity.** The manifest carries the maintainer's base64 DER public `key`
(2026-10-17); its derived ID is `jgpkhdjlddaoopklipcnpcjcpiancmff`. The matching private key is held by the
maintainer outside this repository and must never be committed. Changing the key
changes the ID, the native allowlist and every installed registration, so a new key
needs a deliberate release. The host manifest templates keep the placeholder
`__MAINTAINER_EXTENSION_ID_FROM_MANIFEST_KEY__`; `gcu install chrome-extension`
replaces it with exactly the embedded identity. Rebuild both gcu and the extension
after any key change.

Both sides derive Chrome's ID from the first 16 SHA-256 bytes of the decoded key,
encoding each nibble as a–p. The adapter checks that ID against `chrome.runtime.id`.
`gcu install chrome-extension` replaces the templates' placeholder with exactly
that origin; neither argv, environment nor incoming JSON can add an allowed ID.
Absent/invalid key refuses installation and host startup before any user writes
or frame processing. Do not load an arbitrary unpacked ID as a substitute.

`gcu install chrome-extension --dry-run` previews the two Chrome/Edge registrations;
`gcu uninstall chrome-extension` removes only owned manifests/default registry
values. Both operations are idempotent and support `--home` for disposable tests.
User scope is the only supported scope. This registers the native host, not the
extension in either browser; browser loading/store distribution stays explicit.
See [CLI paths and launch behavior](../../docs/cli.md#native-host--chrome-extension).

The native manifest points directly at the absolute gcu executable. Chrome/Edge
launch it with the origin as argv[1]; gcu recognizes that launch and enters the
same command as `gcu native-host <origin>`. No shell wrapper or fictitious `args`
manifest field is used. Windows `--parent-window` metadata is ignored for identity.

## Native wire protocol (version 1)

Each stdin/stdout frame is a four-byte **little-endian** unsigned length followed
by a UTF-8 JSON object, maximum 1 MiB in both directions. Stdout contains frames
only; diagnostics use stderr without payloads. A partial/invalid/oversized frame,
unexpected message or wrong connection challenge closes the connection. Handshake
and started frames have a five-second deadline; an idle host exits after 60 seconds.

| Direction | Message |
| --- | --- |
| Host → background | `{kind:"hello", protocol:1, connectionId}` with a fresh 256-bit challenge. |
| Background → host | `{kind:"ready", protocol:1, connectionId}` echoes on the same pipe. |
| Background → host | `{kind:"shared", connectionId, scope:{tabId,frameId:0,documentId,shareId}}` from the toolbar click and Chrome injection result. |
| Host → background | `{kind:"sharedAck", connectionId, shareId}` acknowledges consent only. |
| Background → host | `{kind:"revoked", connectionId, shareId}` on browser-owned lifecycle invalidation. |
| Host → background | `{kind:"request", connectionId, request:DomRequest}` only after trusted session binding and full `RouterGate` preflight (P8-03). |
| Background → host | `{kind:"response", connectionId, requestId, result:DomResult}` matched to a single pending host request. |

Envelope and scope unknown fields are rejected. Extension input cannot name a native window or a
session; the host binds a session through an in-process API after P8-03 verifies
the real window/document alignment. Replies inherit that pending request's session.
Only one request per share is in flight; IDs are single-use, with 256 requests per
connection and 64 live shared tabs maximum. Only top frames are injected; readable
child frames remain within that exact top document's DOM snapshot.

Authentication relies on Chrome/Edge enforcing the installed `allowed_origins`
and supplying the launch origin, checked again by gcu. The challenge pins messages
to this actual connection, not to a page's identity claim. It does not authenticate
against a compromised browser or arbitrary programs running as the same OS user.
No page/runtime message endpoint can open or select a native session. Disconnect,
Stop/Stop-health loss or session close must close the native pipe, clear consent
and retire pending actions as `unknown`; observations fail with `app_denied`.
No request is buffered/replayed across disconnect. Re-sharing cannot clear Stop.

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
pnpm exec vitest run computer-use/tools/browser-extension/tests/dom.test.ts computer-use/tools/browser-extension/tests/native.test.ts computer-use/tools/browser-extension/tests/store.test.ts
```

The build emits a self-contained ES module worker, an IIFE content script,
manifest and locales into the ignored `dist/` directory. Load that directory as
an unpacked extension only in a disposable browser profile/fixture. No real
accounts are needed. Protocol compatibility is checked separately by
`rcargo test -p gcu-protocol` on the authorized build machine.

The DOM extractor/executor was reviewed against
`archive/cu-v1-2026-10-13:tools/computer-use-extension/{observe,act}-document.mjs`.
No v1 transport, pairing, lease, storage, or recovery subsystem is copied.
