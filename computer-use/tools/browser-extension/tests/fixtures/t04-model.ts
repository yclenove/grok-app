/** Test-only optimistic reference protocol. No production imports or Chrome claims.
 * In particular, native snapshots, getFrame, and API completion ordering below
 * are granted model assumptions, not evidence that current APIs supply them.
 */
export const adversaries = ["switch_tab", "move_window", "move_geometry", "navigate", "reload",
  "same_document_navigation", "close_tab", "close_window", "replace_tab", "minimize",
  "focus_window", "worker_restart", "late_event", "late_response"] as const;
export type Adversary = typeof adversaries[number];
export interface Injection { point: string; adversary: Adversary; notification?: "immediate" | "delayed" }

export class Schedule {
  trace: string[] = [];
  injected = false;
  now = 0;
  constructor(private readonly injection?: Injection) {}
  apply: (adversary: Adversary, immediate: boolean) => void = () => {};
  hit(point: string): void {
    this.trace.push(point);
    this.now++;
    if (!this.injected && this.injection?.point === point) {
      this.injected = true;
      this.apply(this.injection.adversary, this.injection.notification === "immediate");
    }
  }
  /** :after changes reality AFTER the returned snapshot/write, BEFORE resumption.
   * This prevents a returned object from silently becoming a live atomic view.
   */
  async call<T>(name: string, operation: () => T): Promise<T> {
    this.hit(`${name}:before`);
    const result = operation();
    this.hit(`${name}:after`);
    return result;
  }
}

interface ModelDocument { readonly id: string; url: string; alive: boolean }
interface ModelTab { id: number; windowId: number; active: boolean; document: ModelDocument }
interface ModelWindow { id: number; nativeId: string; geometry: number; minimized: boolean; focused: boolean }
interface Policy {
  window: string; geometry: number; document: ModelDocument; epoch: number;
}
interface Effect {
  policyWindow: string; actualWindow?: string; policyDocument: ModelDocument;
  actualDocument: ModelDocument; selectedDocument?: ModelDocument; minimized?: boolean;
  policyGeometry: number; actualGeometry?: number; policyEpoch: number; actualEpoch: number;
}

class Desktop {
  readonly original: ModelDocument = { id: "doc-A", url: "https://fixture.invalid/", alive: true };
  readonly tabs = new Map<number, ModelTab>([
    [1, { id: 1, windowId: 10, active: true, document: this.original }],
    [2, { id: 2, windowId: 10, active: false, document: { id: "doc-B", url: this.original.url, alive: true } }],
    [3, { id: 3, windowId: 20, active: true, document: { id: "doc-C", url: this.original.url, alive: true } }],
  ]);
  readonly windows = new Map<number, ModelWindow>([
    [10, { id: 10, nativeId: "HW1#instance1", geometry: 0, minimized: false, focused: true }],
    [20, { id: 20, nativeId: "HW2#instance1", geometry: 0, minimized: false, focused: false }],
  ]);
  // Browser-owned UI may survive worker death. It is not a capability.
  readonly titles = new Map<number, string>();
  epoch = 1;
  queuedInvalidations = 0;
  contentRevoked = false;
  onInvalidate: () => void = () => {};
  onOldResponse: () => void = () => {};

  apply(event: Adversary, immediate: boolean): void {
    const tab = this.tabs.get(1);
    const win = this.windows.get(10);
    if (event === "late_response") { this.onOldResponse(); return; }
    if (event === "focus_window") {
      for (const window of this.windows.values()) window.focused = window.id === 20;
      return; // Global foreground is not the per-window selected tab.
    }
    if (event === "late_event") {
      // A queued invalidation for this share finally arrives. Physical changes
      // with delayed notification are exercised by the other adversaries.
      this.queuedInvalidations++;
      this.flushEvents();
      return;
    }
    if (event === "switch_tab") {
      if (tab) tab.active = false;
      this.tabs.get(2)!.active = true;
    }
    if (event === "move_window" && tab) {
      this.tabs.get(3)!.active = false;
      tab.windowId = 20;
    }
    if (event === "move_geometry" && win) win.geometry++;
    if ((event === "navigate" || event === "reload") && tab) {
      tab.document.alive = false;
      tab.document = { id: "doc-new", url: event === "reload" ? this.original.url : "https://fixture.invalid/new", alive: true };
    }
    if (event === "same_document_navigation" && tab) tab.document.url += "#new";
    if (event === "close_tab" || event === "replace_tab" || event === "close_window") {
      if (tab) tab.document.alive = false;
      this.tabs.delete(1);
      if (event === "replace_tab") this.tabs.set(4, {
        id: 4, windowId: 10, active: true,
        document: { id: "doc-replacement", url: this.original.url, alive: true },
      });
      if (event === "close_window") this.windows.delete(10);
    }
    if (event === "minimize" && win) win.minimized = true;
    if (event === "worker_restart") this.epoch++;
    // User/browser state changes independently of delivery of its notifications.
    this.queuedInvalidations++;
    if (immediate) this.flushEvents();
  }
  flushEvents(): void {
    if (!this.queuedInvalidations) return;
    this.queuedInvalidations = 0;
    this.contentRevoked = true;
    this.onInvalidate();
  }
}

/** Independent side-effect sink: inspects live desktop ownership and the actual
 * document object receiving the operation. Never uses protocol comparison code,
 * API snapshots, event counters, response status, nonce, or a claimed document ID.
 */
export class EffectOracle {
  readonly effects: Effect[] = [];
  constructor(private readonly desktop: Desktop) {}
  commit(policy: Policy, actualDocument: ModelDocument): void {
    const tab = [...this.desktop.tabs.values()].find(tab => tab.document === actualDocument);
    const win = tab && this.desktop.windows.get(tab.windowId);
    const selected = win && [...this.desktop.tabs.values()].find(tab => tab.windowId === win.id && tab.active);
    this.effects.push({
      policyWindow: policy.window, actualWindow: win?.nativeId,
      policyDocument: policy.document, actualDocument, selectedDocument: selected?.document,
      minimized: win?.minimized, policyGeometry: policy.geometry, actualGeometry: win?.geometry,
      policyEpoch: policy.epoch, actualEpoch: this.desktop.epoch,
    });
  }
  violations(): Effect[] {
    return this.effects.filter(effect =>
      effect.policyWindow !== effect.actualWindow || effect.policyDocument !== effect.actualDocument ||
      effect.selectedDocument !== effect.actualDocument || effect.minimized !== false ||
      effect.policyGeometry !== effect.actualGeometry || effect.policyEpoch !== effect.actualEpoch);
  }
}

/** Programmable fake chrome.tabs/windows/action plus an optimistic document probe.
 * tabs.get does NOT invent a Tab.documentId field. getFrame is separate and its
 * availability/freshness under the four real permissions remains unconfirmed.
 */
class FakeChrome {
  constructor(private readonly desktop: Desktop, private readonly schedule: Schedule) {}
  tabs = {
    get: (phase: string) => this.schedule.call(`${phase}.tabs.get`, () => {
      const tab = this.desktop.tabs.get(1);
      return tab && { id: tab.id, windowId: tab.windowId, active: tab.active };
    }),
    connect: (documentId: string) => {
      const document = this.desktop.original;
      if (document.id !== documentId) throw new Error("wrong-document-port");
      return { document }; // A persistent port to the EXACT original document.
    },
  };
  windows = {
    get: (phase: string) => this.schedule.call(`${phase}.windows.get`, () => {
      const win = this.desktop.windows.get(10);
      return win && { id: win.id, minimized: win.minimized };
    }),
  };
  webNavigation = {
    getFrame: (phase: string) => this.schedule.call(`${phase}.document.getFrame`, () => {
      const tab = this.desktop.tabs.get(1);
      return tab && { documentId: tab.document.id, url: tab.document.url };
    }),
  };
  action = {
    setTitle: (phase: string, title: string, live: () => boolean) => this.schedule.call(`${phase}.action.setTitle`, () => {
      if (!live()) throw new Error("retired-write");
      this.desktop.titles.set(1, title);
    }),
    getTitle: (phase: string) => this.schedule.call(`${phase}.action.getTitle`, () => this.desktop.titles.get(1) ?? "baseline"),
  };
}

class FakeNative {
  connectionBound = false;
  hubBound = false;
  admission = false;
  live = true;
  pending?: string;
  acceptedResponses = 0;
  readonly ignoredResponses: string[] = [];
  constructor(private readonly desktop: Desktop, private readonly schedule: Schedule) {}
  revoke(): void {
    this.live = false;
    this.admission = false;
    this.connectionBound = false;
    this.hubBound = false;
    this.pending = undefined;
  }
  receive(epoch: number, requestId: string): void {
    if (!this.live || epoch !== this.desktop.epoch || requestId !== this.pending) {
      this.ignoredResponses.push(requestId);
      return;
    }
    this.pending = undefined;
    this.acceptedResponses++;
  }
  sample(phase: string) {
    return this.schedule.call(`${phase}.native`, () => {
      this.schedule.now += 100; // Three samples span >=200 virtual ms, never sleep.
      // Ideal complete/fresh native chrome and window-instance source; stronger
      // than today's drivers. This assumption makes the counterexample stronger.
      return [...this.desktop.windows.values()].map(win => {
        const selected = [...this.desktop.tabs.values()].find(tab => tab.windowId === win.id && tab.active);
        return { nativeId: win.nativeId, geometry: win.geometry, minimized: win.minimized,
          selectedDocument: selected?.document.id, url: selected?.document.url,
          title: selected ? this.desktop.titles.get(selected.id) ?? "baseline" : "baseline" };
      });
    });
  }
}

/** Exploratory algorithm, intentionally NOT a safe production implementation.
 * Local publication/permit consumption is serialized. The browser is not locked.
 */
class ReferenceBinding {
  readonly chrome: FakeChrome;
  readonly native: FakeNative;
  readonly oracle: EffectOracle;
  private readonly epoch: number;
  private readonly policy: Policy;
  private readonly port: { document: ModelDocument };
  private deadline = 0;
  private nonceSequence = 0;
  private permit = false;
  constructor(readonly desktop: Desktop, readonly schedule: Schedule) {
    this.chrome = new FakeChrome(desktop, schedule);
    this.native = new FakeNative(desktop, schedule);
    this.oracle = new EffectOracle(desktop);
    this.epoch = desktop.epoch;
    this.policy = { window: "HW1#instance1", geometry: 0, document: desktop.original, epoch: this.epoch };
    this.port = this.chrome.tabs.connect(desktop.original.id);
    desktop.onInvalidate = () => { this.permit = false; this.native.revoke(); };
    desktop.onOldResponse = () => this.native.receive(this.epoch - 1, "retired-request");
  }
  private require(condition: unknown): asserts condition {
    if (!condition) throw new Error("refused");
  }
  private local(): boolean {
    return this.native.live && this.desktop.epoch === this.epoch && this.schedule.now < this.deadline;
  }
  private async metadata(phase: string): Promise<void> {
    const tab = await this.chrome.tabs.get(phase);
    this.require(this.local() && tab?.active && tab.windowId === 10);
    const document = await this.chrome.webNavigation.getFrame(phase);
    this.require(this.local() && document?.documentId === "doc-A" && document.url === "https://fixture.invalid/");
    const window = await this.chrome.windows.get(phase);
    this.require(this.local() && window && !window.minimized);
  }
  private async nativeCheck(phase: string, title: string): Promise<void> {
    const snapshots = await this.native.sample(phase);
    this.require(this.local());
    const own = snapshots.find(win => win.nativeId === this.policy.window);
    this.require(own && own.geometry === this.policy.geometry && !own.minimized &&
      own.selectedDocument === "doc-A" && own.url === "https://fixture.invalid/" && own.title === title);
    if (title !== "baseline") this.require(snapshots.filter(win => win.title === title).length === 1);
  }
  private async verify(phase: string): Promise<void> {
    await this.schedule.call(`${phase}.router.authorize`, () => this.require(this.local()));
    this.require(this.local());
    await this.nativeCheck(`${phase}.baseline`, "baseline");
    this.schedule.hit(`${phase}.baseline.checked`);
    await this.metadata(`${phase}.initial`);
    this.schedule.hit(`${phase}.initial.checked`);
    const nonce = `t04-model-nonce-${++this.nonceSequence}`; // Public fixture, not CSPRNG.
    await this.chrome.action.setTitle(`${phase}.install`, nonce, () => this.local());
    const installed = await this.chrome.action.getTitle(`${phase}.installed`);
    this.require(this.local() && installed === nonce);
    for (let sample = 1; sample <= 3; sample++) {
      await this.nativeCheck(`${phase}.sample${sample}`, nonce);
      this.schedule.hit(`${phase}.sample${sample}.checked`);
    }
    await this.chrome.action.setTitle(`${phase}.clear`, "baseline", () => this.local());
    const cleared = await this.chrome.action.getTitle(`${phase}.cleared`);
    this.require(this.local() && cleared === "baseline");
    await this.nativeCheck(`${phase}.final`, "baseline");
    this.schedule.hit(`${phase}.final.checked`);
    await this.metadata(`${phase}.final`);
    this.schedule.hit(`${phase}.final.metadataChecked`);
  }
  async run(admission: "explore" | "d2-closed"): Promise<"done" | "refused"> {
    if (admission === "d2-closed") return "refused";
    try {
      this.deadline = this.schedule.now + 4750;
      await this.verify("bind");
      this.schedule.hit("bind.verified");
      await this.schedule.call("bind.connection.stage", () => { this.require(this.local()); this.native.connectionBound = true; });
      await this.schedule.call("bind.hub.stage", () => { this.require(this.local()); this.native.hubBound = true; });
      this.require(this.local() && this.native.connectionBound && this.native.hubBound);
      this.native.admission = true; // No await; both stores hidden behind one gate.
      this.deadline = this.schedule.now + 4750;
      await this.verify("revalidate"); // A new challenge for every action.
      this.schedule.hit("revalidate.verified");
      await this.schedule.call("dispatch.permit", () => {
        this.require(this.local() && this.native.admission);
        this.permit = true;
        this.native.pending = "one-shot";
      });
      await this.metadata("dispatch");
      this.schedule.hit("dispatch.metadataChecked");
      this.schedule.hit("dispatch.beforePost"); // External browser may change even without a JS await.
      this.require(this.local() && this.permit);
      this.permit = false;
      await this.schedule.call("dispatch.message", () => this.require(this.local()));
      this.schedule.hit("content.beforeEffect"); // Private port delivery crosses processes/queues.
      const document = this.port.document;
      // Exact document port + existing content-side URL/liveness guard. There is
      // deliberately NO magical active/window/OS read in the content handler.
      this.require(document.alive && !this.desktop.contentRevoked && document.url === "https://fixture.invalid/");
      this.oracle.commit(this.policy, document);
      this.native.receive(this.epoch, "one-shot");
      return "done";
    } catch {
      this.native.revoke();
      this.permit = false;
      // No claim of physical cleanup after worker death; harmless residual UI
      // is observable in titles and confers no permission. No automatic replay.
      return "refused";
    }
  }
}

export async function explore(injection?: Injection, admission: "explore" | "d2-closed" = "explore") {
  const desktop = new Desktop();
  const schedule = new Schedule(injection);
  const protocol = new ReferenceBinding(desktop, schedule);
  schedule.apply = (event, immediate) => desktop.apply(event, immediate);
  const outcome = await protocol.run(admission);
  return { outcome, desktop, schedule, native: protocol.native, oracle: protocol.oracle };
}
