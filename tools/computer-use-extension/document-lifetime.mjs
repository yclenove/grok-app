import { cancelDocumentOperation } from "./act-document.mjs";

// Queries only the original admitted document. A cached/prerendered document is still alive.
export class DocumentLifetime {
  constructor({ navigation, tabs, extension }) { this.navigation = navigation; this.tabs = tabs; this.extension = extension; }

  async #allowed(context) {
    if (context?.incognito === true && await this.extension.isAllowedIncognitoAccess() !== true) {
      throw new Error("documentLifetimeUnavailable");
    }
  }

  #frame(frame, binding) {
    if (!frame || typeof frame !== "object" || frame.documentId !== binding.documentId
      || frame.parentFrameId !== -1 || frame.frameType !== "outermost_frame"
      || !["active", "cached", "prerender", "pending_deletion"].includes(frame.documentLifecycle)) {
      throw new Error("documentLifetimeUnavailable");
    }
    return frame.documentLifecycle;
  }

  async attest(binding) {
    const tabId = Number(binding.tabId);
    const tab = await this.tabs.get(tabId);
    if (tab?.id !== tabId || typeof tab.incognito !== "boolean") throw new Error("documentLifetimeUnavailable");
    const context = { incognito: tab.incognito };
    await this.#allowed(context);
    const frame = await this.navigation.getFrame({ documentId: binding.documentId, tabId, frameId: 0 });
    if (this.#frame(frame, binding) !== "active" || frame.errorOccurred !== false) throw new Error("documentLifetimeUnavailable");
    const url = new URL(frame.url);
    if (!/^https?:$/.test(url.protocol) || url.username || url.password) throw new Error("documentLifetimeUnavailable");
    await this.#allowed(context);
    return Object.freeze(context);
  }

  async state(binding, context) {
    if (!context || typeof context.incognito !== "boolean") throw new Error("documentLifetimeUnavailable");
    await this.#allowed(context);
    // Supplying current tab/frame constraints here would confuse mismatches with document destruction.
    const frame = await this.navigation.getFrame({ documentId: binding.documentId });
    await this.#allowed(context);
    // Chrome removes FrameNavigationState on RenderFrameHostChanged, including BFCache.
    // A null response can later become the same active document again. It is not destruction.
    if (frame === null) return "unavailable";
    return this.#frame(frame, binding) === "active" ? "active" : "retained";
  }
}

export async function proveDocumentCompletion({ lifetime, scripting, binding, context, retireExecution }) {
  const state = async () => context === null ? "unattested" : lifetime.state(binding, context);
  const before = await state();
  if (before === "retained" || before === "unavailable") return false;
  try {
    // Never race away an original injection. Only its matching completed return proves completion.
    const rows = await scripting.executeScript({
      target: { tabId: Number(binding.tabId), documentIds: [binding.documentId] }, world: "ISOLATED",
      func: cancelDocumentOperation, args: [binding.snapshotId, binding.requestId, retireExecution],
    });
    if (rows?.length === 1 && rows[0].frameId === 0 && rows[0].documentId === binding.documentId
      && rows[0].result?.status === "settled") return true;
  } catch { /* A native exception is not evidence, even when getFrame also returns null. */ }
  return false;
}
