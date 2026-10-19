/** Local jsdom only. No resource loading, native host, network or production imports. */
export type Carrier = "C1" | "C2";
export type Transport = "postMessage" | "same_origin_reference";
type Page = Window & typeof globalThis;
type PageId = "A" | "B";
const fixtureHtml = '<!doctype html><title>Continue</title><body><button>Continue</button></body>';

export function createPages(parent: Document) {
  const frames = [parent.createElement("iframe"), parent.createElement("iframe")];
  for (const frame of frames) parent.body.append(frame); // about:blank, no src/resources
  const [A, B] = frames.map(frame => frame.contentWindow as Page);
  const identity = new Map<Document, PageId>([[A.document, "A"], [B.document, "B"]]);
  const actions = { A: 0, B: 0 };
  for (const win of [A, B]) {
    win.document.open();
    win.document.write(fixtureHtml);
    win.document.close();
    const id = identity.get(win.document)!;
    win.document.querySelector("button")!.addEventListener("click", () => { actions[id]++; });
  }
  return { A, B, identity, actions, sharedDocument: A.document,
    close: () => { for (const frame of frames) frame.remove(); } };
}
type Pages = ReturnType<typeof createPages>;

export const marker = (nonce: string) => ` [gcu:${nonce}]`;
export function readCarrier(doc: Document, carrier: Carrier): string {
  const button = doc.querySelector("button")!;
  return carrier === "C1" ? doc.title : button.getAttribute("aria-label") ?? button.textContent!;
}
export function writeCarrier(doc: Document, carrier: Carrier, suffix: string): void {
  if (carrier === "C1") doc.title = `Continue${suffix}`;
  else if (suffix) doc.querySelector("button")!.setAttribute("aria-label", `Continue${suffix}`);
  else doc.querySelector("button")!.removeAttribute("aria-label");
}
/** Candidate scan only; it deliberately has no binding or action capability. */
export function matches(f: Pages, carrier: Carrier, nonce: string): PageId[] {
  return [f.A, f.B].filter(win => readCarrier(win.document, carrier).endsWith(marker(nonce)))
    .map(win => f.identity.get(win.document)!);
}

export function installRelay(f: Pages, carrier: Carrier, transport: Transport) {
  const reads: string[] = [];
  let delivered!: () => void;
  const delivery = new Promise<void>(resolve => { delivered = resolve; });
  const receive = (event: MessageEvent) => {
    if (event.data?.kind !== "fixture-relay" || event.data.carrier !== carrier) return;
    writeCarrier(f.B.document, carrier, event.data.suffix);
    delivered();
  };
  f.B.addEventListener("message", receive);
  const observer = new f.A.MutationObserver(() => {
    // Attacker code reads shared DOM, never a test variable containing the nonce.
    const suffix = readCarrier(f.A.document, carrier).match(/ \[gcu:[a-f0-9]{64}\]$/)?.[0];
    if (!suffix) return;
    reads.push(suffix);
    if (transport === "postMessage") {
      // Real jsdom asynchronous postMessage. Wildcard is local adversary code only;
      // this fixture does not claim jsdom authenticates native origin/source.
      f.B.postMessage({ kind: "fixture-relay", carrier, suffix }, "*");
    } else {
      // Same-origin Window reference: ordinary page DOM access is sufficient.
      writeCarrier(f.B.document, carrier, suffix);
    }
    writeCarrier(f.A.document, carrier, "");
    if (transport === "same_origin_reference") delivered();
  });
  observer.observe(f.A.document, { subtree: true, childList: true, characterData: true, attributes: true });
  return { reads, delivered: delivery,
    close: () => { observer.disconnect(); f.B.removeEventListener("message", receive); } };
}

/** Model of a private browser-owned slot, NOT an assertion about Chrome lifecycle. */
export function privateActionShim() {
  const titles = new Map<number, string>();
  return {
    setTitle: async ({ tabId, title }: { tabId: number; title: string }) => { titles.set(tabId, title); },
    getTitle: async ({ tabId }: { tabId: number }) => titles.get(tabId) ?? "Share this tab",
  };
}

/** Explicit page-visible API allowlist, plus a recorder for the content-world port.
 * No action API, native pipe, worker closure or test runner global is exposed.
 * This checks the modeled information flow, not arbitrary Chrome internals.
 */
export function pageVisibleShim(win: Page) {
  const pageMessages: unknown[] = [];
  const contentMessages: unknown[] = [];
  const captured: string[] = [];
  const capture = () => { captured.push(win.document.documentElement.outerHTML); };
  const observer = new win.MutationObserver(records => {
    capture();
    for (const record of records) {
      if (record.oldValue) captured.push(record.oldValue);
      for (const node of record.addedNodes) captured.push(node.textContent ?? "");
    }
  });
  observer.observe(win.document, { subtree: true, attributes: true, attributeOldValue: true,
    childList: true, characterData: true, characterDataOldValue: true });
  const receive = (event: MessageEvent) => { pageMessages.push(event.data); };
  win.addEventListener("message", receive);
  const api = Object.freeze({
    document: win.document,
    MutationObserver: win.MutationObserver,
    postMessage: win.postMessage.bind(win),
    addEventListener: win.addEventListener.bind(win),
    removeEventListener: win.removeEventListener.bind(win),
  });
  return {
    api,
    contentIngress: (message: unknown) => { contentMessages.push(message); },
    postAndWait: (message: unknown) => new Promise<void>(resolve => {
      win.addEventListener("message", () => resolve(), { once: true });
      api.postMessage(message, "*");
    }),
    leaks: (nonce: string) => {
      capture();
      const surfaces = { title: api.document.title, dom: api.document.documentElement.outerHTML,
        pageMessages: JSON.stringify(pageMessages), contentMessages: JSON.stringify(contentMessages),
        captured: JSON.stringify(captured) };
      return Object.entries(surfaces).filter(([, text]) => text.includes(nonce)).map(([name]) => name);
    },
    close: () => { observer.disconnect(); win.removeEventListener("message", receive); },
  };
}
