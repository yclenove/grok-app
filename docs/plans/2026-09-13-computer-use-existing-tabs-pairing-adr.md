# ADR: Existing Tabs pairing transport

Date: 2026-09-13; revised 2026-09-20
Status: pairing and share-candidate slice implemented; full Existing Tabs transport is unfinished
Context: R2D secure pairing, replacing the old public-nonce/automatic-confirm fixture

## Decision

Use an **MV3 service worker** and **bounded loopback HTTP requests**. HTTP binds
only to `127.0.0.1` on an ephemeral port. Pairing is not tab authorization.

The App-only verification code has 80 CSPRNG bits and a five-minute lifetime.
It is intentionally displayed in trusted App UI and entered in the extension
popup; it is never exposed by the public HTTP challenge, ordinary page DOM,
URL, GET body, history, logs, crash reports, or test screenshots. The session
key is never returned to the Tauri frontend or popup messaging API.

## Flow

1. User clicks Pair in App. Creating a challenge revokes any previous pairing
   and borrowed-tab grants. App shows the installed extension ID.
2. User explicitly confirms that exact challenge nonce in App. Only then does
   the UI display the App address and verification code. Old confirmations
   cannot confirm a replacement challenge.
3. User opens the extension popup, enters the address and code, then clicks
   Pair. The popup clears the input immediately; the service worker obtains
   public metadata from `POST /cu/pairing-challenge`.
4. The worker sends `POST /cu/pairing-confirm` with an HMAC-SHA256 proof. Key:
   the code with ASCII spaces/hyphens removed and hexadecimal letters uppercased.
   Signed UTF-8 bytes are the compact JSON array:
   `["grok-cu-pairing-v1", nonce, instance, ext, expiresAt, connectionNonce]`.
5. Under one Host lock, validate App confirmation, expiry, protocol, every
   identity field, and the MAC, then consume the challenge once and issue a
   session key bound to App instance, connection nonce, and generation.
6. The worker keeps the key in memory and `chrome.storage.session` restricted
   to `TRUSTED_CONTEXTS`. No local/sync storage and no key-export message.
   Status/disconnect require Bearer plus the complete connection binding.
   A coalesced status request renews the monotonic 30-second Host lease every
   10 seconds, with a five-second request deadline; popup presence is not required.
7. User opens the browser action on the desired tab, then clicks Share current
   tab. The action supplies Chrome's temporary `activeTab` grant; simply opening
   `popup.html` as a normal browser tab does not. The worker queries only the
   current active tab and checks the exact tab shown in the popup before reading
   fixed main-frame metadata in the isolated world. Sharing creates a candidate;
   it does not authorize a session/run or enable model control.

There is no ordinary pairing web page, no automatic confirmation fallback,
and no `/cu/pairing-session` endpoint. Challenge GET remains public-metadata
only with the same exact-Origin checks; the extension itself uses POST.

## Identity and threat boundaries

- Source installation ID is `bgegbabkegkanjbmjbeaockdijnbkjgi`, derived from the
  public RSA key embedded in `manifest.json`. The public key stabilizes the
  unpacked installation ID; it is **not** an authentication secret, binary hash,
  store signature, or proof that unpacked source was not modified.
- Exact extension Origin and loopback Host checks, forbidden forwarded headers,
  strict bodies, size limits, expiry and rate limiting are defense in depth.
  Native local callers can forge Origin; knowing public metadata is insufficient
  because the caller must also prove possession of the App-only code.
- This does not defend against an already-compromised OS, modified trusted App
  or extension, debugger/memory access, or an active local proxy impersonating
  the endpoint the user entered. Users must load trusted extension source and
  copy the address from the trusted App. No TLS/local-process attestation is claimed.
- Host error responses and probe output never include the code, proof or key.
  A proof is a one-use credential too; it is not safe to log it.
- Ordinary web pages/content scripts cannot invoke the popup-only message API.
  The extension requests `storage`, `activeTab`, `scripting` and the loopback
  host permission. No all-tabs enumeration, broad host access, Cookie or debugger
  permission. Fixed scripts are injected only after an explicit Share request.

## Lifecycle and remaining work

- Explicit Unpair, replacement challenge, rotation, feature-off and App exit
  revoke Host authority and invalidate borrowed-tab observations/previews.
- Worker startup removes prior session data and attempts authenticated Host
  disconnect; it never silently restores a previous grant.
- Local storage failure must not skip the Host disconnect attempt. Late pairing
  replies are fenced by the worker revision and their issued key is revoked.
- Browser exit clears browser session storage. Host dispatch rejects at the
  monotonic 30-second lease boundary; a listener-owned one-second sweep also
  clears raw credentials, candidates and borrowed grants. Valid full-binding
  status/heartbeat requests renew; invalid or expired identities cannot renew.
  Abrupt browser death is bounded expiry, not immediate crash detection.
- Share/Unshare HTTP mutations authenticate and validate a connection-wide
  increasing sequence under the same lock as candidate/grant mutation. The
  Broker feature gate wraps that lock in the same order as feature-off. There
  are at most 64 candidates. Unshare binds the document generation and retires
  its grant. The worker binds each share to Chrome's main-frame `documentId`,
  revalidates it before/after offering, and invalidates immediately on loading,
  URL changes, close or unshare. Same-URL reloads cannot reuse document authority.
- A new offer carries a fresh Host picker token. App candidate IDs use
  `existing-tab:{tabId}@{pickerToken}`. Authorization compares the token under the
  same lock as reading the candidate and creating the grant. A stale displayed
  candidate cannot authorize a newer document. Actual tab IDs remain separate
  from picker selectors; a production adapter must preserve the ExistingTab surface.
- Share/unshare intents capture the initiating connection before asynchronous
  work and pass the expected worker epoch. They cannot acquire a replacement
  pairing. Unshare locally fences the tab until retirement completes; late share
  callbacks cannot resurrect it. Worker notifications prompt an open popup to
  reread local state; they contain neither credentials nor page metadata and
  do not introduce network polling.
- Tab transport must additionally bind session/run/tab/document, handle bounded
  deadlines/cancellation/heartbeat/navigation, and reject late results. Pairing
  alone does not satisfy those requirements or prove a model can control a tab.

## Why not native messaging as the first transport

Native messaging needs a stable host install identity, OS-specific manifests,
and a separate trust story for unpacked vs store-signed extensions. Loopback
reuses the Host IPC, but needs the possession proof described above. Native
messaging remains an option if store policy forbids loopback.

## Invariants

- Challenge GET exposes nonce, expiry, public metadata only.
- Session key dies on revoke/rotate and App restart; worker restart discards it
  and attempts Host revocation. Abrupt browser death expires at the Host lease
  deadline even with no disconnect packet. Old renewals cannot revive the key.
- Nonce, App instance, extension id, expiry, connection nonce and connection
  generation bind pairing against replay. Document generation belongs to the
  following tab transport, not the pairing challenge.
- CORS, CSRF, Origin, Host, forwarded headers, rate-limit, expiry, and
  replay have negative tests.
- Arbitrary `chrome-extension://` origins are rejected; installed id must
  match.

## Evidence boundary

`pairing-client.test.mjs` tests worker state/races/faults. Core pairing/IPC tests
exercise possession, expiry, field tampering, replay, concurrent completion,
revocation, feature-off and origins. `pairing-live.mjs` exercises the real MV3
popup/service worker with a new disposable Chrome profile and real Rust IPC.
The `cu_probe existing-tab-extension` wrapper uses the process-wide App Broker
and requires an owner-marked isolated App home. Its name is historical:
its evidence now includes candidate sharing and document retirement, not full
tab operations, installed acceptance, Edge acceptance, cross-platform acceptance,
or real-model completion. The automatic share success fixture uses loopback
permission; a separate non-loopback fixture proves denial without an action
grant. `existing-tab-extension-toolbar` additionally requires an operator to
click the real browser action and Share/Unshare on an isolated non-loopback
fixture. It cannot pass by opening a popup tab or injecting an activeTab grant.
