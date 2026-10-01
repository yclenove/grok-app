const t = key => chrome.i18n.getMessage(key);
for (const element of document.querySelectorAll("[data-i18n]")) {
  element.textContent = t(element.dataset.i18n);
}
document.documentElement.lang = chrome.i18n.getUILanguage();
document.title = t("extensionName");
document.getElementById("identity").textContent = chrome.runtime.id;
const form = document.getElementById("pair-form");
const code = document.getElementById("code");
const endpoint = document.getElementById("endpoint");
const status = document.getElementById("status");
const pair = document.getElementById("pair");
const forget = document.getElementById("forget");
const shareCurrent = document.getElementById("share-current");
const unshareCurrent = document.getElementById("unshare-current");
const tabTitle = document.getElementById("tab-title");
const tabStatus = document.getElementById("tab-status");
let requestRevision = 0;
let paired = false;
let selectedTab = null;
let shared = false;
let sharingBusy = false;
let tabRevision = 0;

function shareButtons() {
  shareCurrent.disabled = !paired || sharingBusy || selectedTab === null || shared;
  unshareCurrent.disabled = !paired || sharingBusy || selectedTab === null || !shared;
}

function clearRetiredSharingStatus() {
  if (tabStatus.dataset.state === "shared" || tabStatus.dataset.state === "unshared") {
    tabStatus.dataset.state = "";
    tabStatus.textContent = "";
  }
}

async function refreshTab() {
  const revision = ++tabRevision;
  if (!paired) {
    selectedTab = null; shared = false; tabTitle.textContent = "";
    clearRetiredSharingStatus(); shareButtons(); return;
  }
  try {
    const result = await chrome.runtime.sendMessage({ type: "cu-tab-state" });
    if (revision !== tabRevision || !paired) return;
    if (!result?.ok) throw new Error("shareUnavailable");
    if (!result.paired) {
      paired = false; selectedTab = null; shared = false; tabTitle.textContent = "";
      status.dataset.state = "unpaired"; status.textContent = t("unpaired");
      clearRetiredSharingStatus(); shareButtons(); return;
    }
    if (!Number.isSafeInteger(result.tabId)) throw new Error("shareUnavailable");
    if (tabStatus.dataset.state === "shared" && (!result.shared || result.tabId !== selectedTab)) {
      tabStatus.dataset.state = ""; tabStatus.textContent = "";
    }
    selectedTab = result.tabId; shared = result.shared === true;
    tabTitle.textContent = result.title || "";
  } catch {
    if (revision !== tabRevision) return;
    selectedTab = null; shared = false; tabStatus.textContent = t("shareUnavailable");
  }
  shareButtons();
}

async function send(message) {
  const revision = ++requestRevision;
  try {
    const result = await chrome.runtime.sendMessage(message);
    if (revision !== requestRevision) return;
    if (!result?.ok) {
      paired = false; selectedTab = null; ++tabRevision; shareButtons();
      status.dataset.state = result?.error || "connectionFailed";
      status.textContent = t(status.dataset.state); return;
    }
    paired = result.paired === true;
    status.dataset.state = result.paired ? "paired" : "unpaired";
    status.textContent = t(result.paired ? "paired" : "unpaired");
    if (result.endpoint) endpoint.value = result.endpoint;
    tabStatus.dataset.state = ""; tabStatus.textContent = "";
    await refreshTab();
  } catch {
    if (revision !== requestRevision) return;
    status.dataset.state = "connectionFailed";
    status.textContent = t("connectionFailed");
    paired = false; shareButtons();
  }
}
form.addEventListener("submit", event => {
  event.preventDefault();
  if (pair.disabled) return;
  const value = code.value;
  code.value = "";
  paired = false; selectedTab = null; ++tabRevision; shareButtons();
  pair.disabled = true;
  void send({ type: "cu-pair", endpoint: endpoint.value.trim(), code: value })
    .finally(() => { pair.disabled = false; });
});
forget.addEventListener("click", () => {
  if (forget.disabled) return;
  forget.disabled = true;
  code.value = "";
  paired = false; selectedTab = null; ++tabRevision; shareButtons();
  void send({ type: "cu-forget" }).finally(() => { forget.disabled = false; });
});
async function changeSharing(type) {
  if (!paired || sharingBusy || selectedTab === null) return;
  const revision = ++requestRevision;
  const tabId = selectedTab;
  sharingBusy = true; shareButtons();
  try {
    const result = await chrome.runtime.sendMessage({ type, tabId });
    if (revision !== requestRevision) return;
    if (!result?.ok) throw new Error(result?.error || "shareUnavailable");
    tabStatus.dataset.state = result.shared ? "shared" : "unshared";
    tabStatus.textContent = t(result.shared ? "sharedCurrentTab" : "unsharedCurrentTab");
  } catch {
    if (revision === requestRevision) {
      tabStatus.dataset.state = "shareUnavailable"; tabStatus.textContent = t("shareUnavailable");
    }
  } finally {
    sharingBusy = false;
    await refreshTab();
  }
}
shareCurrent.addEventListener("click", () => { if (!shareCurrent.disabled) void changeSharing("cu-share-current"); });
unshareCurrent.addEventListener("click", () => { if (!unshareCurrent.disabled) void changeSharing("cu-unshare-current"); });
chrome.tabs.onActivated.addListener(() => { void refreshTab(); });
chrome.tabs.onUpdated.addListener((tabId, change) => {
  if (tabId === selectedTab && (change.status === "complete" || typeof change.url === "string")) void refreshTab();
});
chrome.runtime.onMessage.addListener((message, sender) => {
  if (sender.id === chrome.runtime.id && message?.type === "cu-state-changed") void refreshTab();
});
void send({ type: "cu-status" });
