import { EXTENSION_ID, PairingClient } from "./pairing-client.mjs";
import { SharedTabs } from "./shared-tabs.mjs";
import { ExtensionTransport } from "./extension-transport.mjs";
import { CompletionJournal } from "./completion-journal.mjs";
import { CompletionRecovery } from "./completion-recovery.mjs";
import { allocateWorkerEpoch, ExecutionClock } from "./execution-clock.mjs";
import { DocumentLifetime } from "./document-lifetime.mjs";
import { HostLifetimeClient } from "./host-lifetime.mjs";
import { BrowserSession } from "./browser-session.mjs";
import { decideBrowserExit } from "./browser-exit.mjs";
// Startup metadata alone cannot release an owner. Retirement also requires the
// original Host's exact browser/document/request match against durable cleanup.
const browserSession = new BrowserSession({ storage: chrome.storage.local, runtime: chrome.runtime });
void browserSession.ready.catch(() => {});
let sharing;
let transport;
// A hint only: popup re-reads local state. Never broadcast credentials or page metadata.
const notifyState = () => { void chrome.runtime.sendMessage({ type: "cu-state-changed" }).catch(() => {}); };
let journal;
let sessionChangeRevision = 0;
const client = new PairingClient({ storage: chrome.storage.session, onSessionChanged: () => {
  const revision = ++sessionChangeRevision;
  sharing?.reset(); transport?.reset();
  void (async () => {
    await browserSession.ready; await journal?.ready;
    if (revision !== sessionChangeRevision || client.connectionEpoch === null) { notifyState(); return; }
    const session = await browserSession.snapshot();
    const cleanups = session.previousId ? await journal.cleanupRecords(session.previousId) : [];
    const registration = await client.registerBrowser(session.id, session.previousId, cleanups);
    if (revision !== sessionChangeRevision || client.connectionEpoch === null) return;
    if (session.previousId && cleanups.length > 0) {
      const confirmed = registration.results.every((result) => {
        const record = cleanups.find((item) => item.requestId === result.requestId);
        if (!record) return false;
        const decision = decideBrowserExit({
          writeSucceeded: true,
          paired: true,
          currentId: session.id,
          previousId: session.previousId,
          physical: record.phase,
          record,
          pending: {
            browserId: record.browserId,
            requestId: record.requestId,
            documentId: record.documentId,
          },
        });
        return decision.release && decision.actions === 0 && decision.result === result.result;
      });
      if (confirmed && registration.results.length === cleanups.length) {
        await journal.forgetBrowserExit(registration.results.map((item) => item.requestId));
        await browserSession.acknowledgeRestart(session);
      }
    } else if (session.previousId && cleanups.length === 0) {
      await browserSession.acknowledgeRestart(session);
    }
    if (revision !== sessionChangeRevision || client.connectionEpoch === null) return;
    transport?.start(); notifyState();
  })().catch(() => { if (revision === sessionChangeRevision) notifyState(); });
} });
const lifetime = new DocumentLifetime({ navigation: chrome.webNavigation, tabs: chrome.tabs, extension: chrome.extension });
journal = new CompletionJournal({ storage: chrome.storage.local, lifetime, scripting: chrome.scripting,
  receipts: chrome.storage.session, hostLifetime: new HostLifetimeClient({ client }) });
const clock = new ExecutionClock(allocateWorkerEpoch());
sharing = new SharedTabs({ client, tabs: chrome.tabs, scripting: chrome.scripting, journal, clock });
transport = new ExtensionTransport({ client, journal, observe: request => sharing.observe(request),
  act: (request, signal) => sharing.act(request, signal), recovered: binding => sharing.releaseRecovered(binding) });
const recovery = new CompletionRecovery({ journal, scripting: chrome.scripting, notify: notifyState });
void recovery.start(client.ready).then(notifyState, notifyState);
void client.ready.then(notifyState, notifyState);
const popupUrl = chrome.runtime.getURL("popup.html");
const errors = new Set(["invalidAddress", "invalidCode", "challengeChanged", "pairingRejected", "busy", "cancelled", "shareUnavailable"]);
const changedTab = tabId => {
  void sharing.invalidate(tabId).catch(() => {}).finally(() => {
    // Only already-owned receipts match this tab. Navigation of unrelated tabs does not query their frames.
    void recovery.retry(tabId); void transport.retryCleanup(tabId); notifyState();
  });
};
chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (change.status === "loading" || typeof change.url === "string") {
    changedTab(tabId);
  }
});
chrome.tabs.onRemoved.addListener(changedTab);
chrome.tabs.onActivated.addListener(() => { sharing.activityChanged(); });
chrome.windows.onFocusChanged.addListener(() => { sharing.activityChanged(); });
chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local" && area !== "session") return;
  for (const [key, change] of Object.entries(changes)) {
    if (change.newValue === undefined) continue;
    void journal.storedReceipt(key).then(tabId => {
      if (tabId !== null) { void recovery.retry(tabId); void transport.retryCleanup(tabId); }
    }).catch(() => {});
  }
});

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (sender.id === EXTENSION_ID && message?.type === "cu-document-finished") {
    void journal.documentFinished(message, sender).then(accepted => {
      sendResponse({ ok: accepted });
      if (accepted) { void recovery.retry(sender.tab.id); void transport.retryCleanup(sender.tab.id); }
    }, () => sendResponse({ ok: false }));
    return true;
  }
  if (sender.id !== EXTENSION_ID || sender.url !== popupUrl || sender.frameId > 0) return;
  if (!message || typeof message !== "object") return;
  let operation;
  if (message.type === "cu-pair") operation = () => client.pair(message.endpoint, message.code);
  else if (message.type === "cu-status") operation = () => client.status();
  else if (message.type === "cu-forget") operation = () => client.forget();
  else if (message.type === "cu-tab-state") operation = async () => {
    await client.ready;
    if (client.connectionEpoch === null) return { paired: false };
    const tab = await sharing.current();
    return { paired: client.connectionEpoch !== null, ...tab };
  };
  else if (message.type === "cu-share-current") operation = () => sharing.share(message.tabId);
  else if (message.type === "cu-unshare-current") operation = () => sharing.unshare(message.tabId);
  else return;
  Promise.resolve().then(operation).then(
    result => sendResponse({ ok: true, ...result }),
    error => sendResponse({ ok: false, error: errors.has(error.message) ? error.message : "connectionFailed" }),
  );
  return true;
});
