/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { IDLE_SNAPSHOT } from "@/lib/session";
import { useSendQueue } from "./useSendQueue";
const mock = vi.hoisted(() => ({ snapshot: vi.fn(), listen: vi.fn(), desktop: true, callbacks: new Map<string, (value: unknown) => void>(), cleanups: [] as Array<ReturnType<typeof vi.fn>> }));
vi.mock("@/lib/api", () => ({ hasHost: () => true, isDesktopHost: () => mock.desktop, isMirrorClient: () => !mock.desktop, listen: mock.listen, sessionQueueSnapshot: mock.snapshot }));
function entry(id: string, state = "queued", sessionId = "s") { return { sessionId, itemId: id, prompt: `prompt ${id}`, createdAt: 1, state }; }
function options(acceptExternal = true) {
  return { sessionId: "s", sessionState: "streaming" as const, connecting: false,
    liveHostRef: { current: { ...IDLE_SNAPSHOT, sessionId: "s", state: "streaming" as const } },
    viewingSessionIdRef: { current: "s" as string | null }, sendInFlightRef: { current: false },
    executeSendRef: { current: vi.fn(async () => true) }, showToast: vi.fn(), acceptExternal,
    labels: { sendFailed: "failed", droppedOldest: () => "dropped" } };
}
async function settle(ms = 0) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }
function emit(event: string, value: unknown) { act(() => mock.callbacks.get(event)?.(value)); }
beforeEach(() => {
  vi.useFakeTimers(); mock.desktop = true; mock.callbacks.clear(); mock.cleanups = []; mock.snapshot.mockReset().mockResolvedValue({ version: 1, items: [] });
  mock.listen.mockReset().mockImplementation(async (event: string, fn: (v: unknown) => void) => {
    mock.callbacks.set(event, fn); const un = vi.fn(() => { if (mock.callbacks.get(event) === fn) mock.callbacks.delete(event); }); mock.cleanups.push(un); return un;
  });
});
afterEach(() => { cleanup(); vi.useRealTimers(); });
it("restores durable Host queue on mount without relying on historical push events", async () => {
  mock.snapshot.mockResolvedValue({ version: 1, items: [entry("restored", "held")] });
  const { result } = renderHook(() => useSendQueue(options())); await settle(100);
  expect(result.current.activeQueue.map(x => x.id)).toEqual(["restored"]);
  expect(result.current.activeQueue[0]).toMatchObject({ source: "external", externalState: "held" });
});
it("repairs missed take and handoff order from authoritative snapshots on focus", async () => {
  mock.snapshot.mockResolvedValueOnce({ version: 1, items: [entry("old"), entry("tail")] }).mockResolvedValue({ version: 1, items: [entry("replacement"), entry("tail")] });
  const p = options(); const { result } = renderHook(() => useSendQueue(p)); await settle(100);
  act(() => window.dispatchEvent(new Event("focus"))); await settle(100);
  expect(result.current.activeQueue.map(x => x.id)).toEqual(["replacement", "tail"]);
});
it("cleans up an acquired push listener when take listener registration fails", async () => {
  const original = mock.listen.getMockImplementation()!; let failed = false;
  mock.listen.mockImplementation(async (event: string, fn: (v: unknown) => void) => {
    if (event === "session://send_queue_take" && !failed) { failed = true; throw new Error("synthetic register failure"); }
    return original(event, fn);
  });
  const p = options(); renderHook(() => useSendQueue(p)); await settle();
  expect(mock.cleanups[0]).toHaveBeenCalledTimes(1);
});
it("preserves mirror observation and does not read queues in a secondary window", async () => {
  mock.desktop = false; const a = renderHook(() => useSendQueue(options())); await settle();
  expect(mock.listen).toHaveBeenCalledTimes(3); expect(mock.snapshot).toHaveBeenCalledTimes(1); a.unmount(); mock.snapshot.mockClear();
  mock.desktop = true; renderHook(() => useSendQueue(options(false))); await settle(); expect(mock.snapshot).not.toHaveBeenCalled();
});
it("ignores a stale snapshot response overtaken by a queue event", async () => {
  let resolve!: (value: unknown) => void; mock.snapshot.mockReturnValueOnce(new Promise(r => { resolve = r; })).mockResolvedValue({ version: 1, items: [entry("new")] });
  const p = options(); const { result } = renderHook(() => useSendQueue(p)); await settle(100);
  expect(mock.snapshot).toHaveBeenCalledTimes(1);
  emit("session://send_queue", { sessionId: "s", itemId: "new", prompt: "prompt new" });
  await act(async () => resolve({ version: 1, items: [entry("stale")] })); await settle(100);
  expect(result.current.activeQueue.map(x => x.id)).toEqual(["new"]);
  expect(mock.snapshot).toHaveBeenCalledTimes(2);
});

it("keeps the last verified queue on read failure and recovers only from a valid snapshot", async () => {
  mock.snapshot.mockResolvedValueOnce({ version: 1, items: [entry("kept")] }).mockRejectedValueOnce(new Error("private detail")).mockResolvedValue({ version: 1, items: [] });
  const p = options(); const { result } = renderHook(() => useSendQueue(p)); await settle();
  act(() => result.current.externalSync.refresh()); await settle();
  expect(result.current.activeQueue.map(x => x.id)).toEqual(["kept"]);
  expect(result.current.externalSync.error).toBe(true); expect(result.current.externalSync.ready).toBe(false);
  expect(p.showToast).not.toHaveBeenCalled(); expect(p.executeSendRef.current).not.toHaveBeenCalled();
  act(() => result.current.externalSync.refresh()); await settle();
  expect(result.current.activeQueue).toEqual([]); expect(result.current.externalSync.ready).toBe(true);
});
it("coalesces an event burst, cleans all timers/listeners, and uses latest callbacks without resubscription", async () => {
  const p = options(); const view = renderHook(({ p }) => useSendQueue(p), { initialProps: { p } }); await settle();
  for (let i=0;i<100;i++) emit("session://send_queue", { sessionId:"s", itemId:`q-${i}`, prompt:"p" });
  expect(mock.snapshot).toHaveBeenCalledTimes(1); await settle(100); expect(mock.snapshot).toHaveBeenCalledTimes(2);
  view.rerender({ p: { ...p, labels: { ...p.labels } } }); await settle(); expect(mock.listen).toHaveBeenCalledTimes(2);
  view.unmount(); expect(mock.cleanups.every(fn => fn.mock.calls.length === 1)).toBe(true);
  expect(vi.getTimerCount()).toBe(0); expect(mock.callbacks.size).toBe(0);
});
it("releases a push listener that arrives after unmount without registering a take listener", async () => {
  let resolve!: (value: () => void) => void; const un = vi.fn();
  mock.listen.mockReturnValueOnce(new Promise(r => { resolve=r; }));
  const p = options(); const view = renderHook(() => useSendQueue(p)); view.unmount();
  await act(async () => resolve(un)); await settle();
  expect(un).toHaveBeenCalledTimes(1); expect(mock.listen).toHaveBeenCalledTimes(1);
  expect(mock.snapshot).not.toHaveBeenCalled(); expect(vi.getTimerCount()).toBe(0);
});
it("releases both listeners when unmounted during second registration", async () => {
  let resolve!: (value: () => void) => void; const un = vi.fn(); const original=mock.listen.getMockImplementation()!;
  mock.listen.mockImplementation((event:string, fn:(v:unknown)=>void) => event === "session://send_queue_take"
    ? new Promise(r => { resolve=r; }) : original(event,fn));
  const p=options(); const view=renderHook(()=>useSendQueue(p)); await settle(); view.unmount();
  await act(async()=>resolve(un)); await settle();
  expect(un).toHaveBeenCalledTimes(1); expect(mock.cleanups[0]).toHaveBeenCalledTimes(1); expect(vi.getTimerCount()).toBe(0);
});
it("does not accept or reschedule a response delivered after unmount", async () => {
  let resolve!: (value:unknown)=>void; mock.snapshot.mockReturnValue(new Promise(r=>{resolve=r;}));
  const p=options(); const view=renderHook(()=>useSendQueue(p)); await settle(); view.unmount();
  await act(async()=>resolve({version:1,items:[entry("late")]})); await settle(15000);
  expect(mock.snapshot).toHaveBeenCalledTimes(1); expect(vi.getTimerCount()).toBe(0); expect(p.executeSendRef.current).not.toHaveBeenCalled();
});
it("marks a hung read stale without overlapping requests, then refreshes after its observed generation changes", async () => {
  let resolve!: (value:unknown)=>void; mock.snapshot.mockReturnValueOnce(new Promise(r=>{resolve=r;})).mockResolvedValue({version:1,items:[entry("fresh")]});
  const p=options(); const {result}=renderHook(()=>useSendQueue(p)); await settle(5001);
  expect(result.current.externalSync.error).toBe(true); expect(result.current.externalSync.loading).toBe(false);
  for(let i=0;i<50;i++) act(()=>result.current.externalSync.refresh());
  await settle(15000); expect(mock.snapshot).toHaveBeenCalledTimes(1);
  await act(async()=>resolve({version:1,items:[entry("stale")]})); await settle(100);
  expect(mock.snapshot).toHaveBeenCalledTimes(2); expect(result.current.activeQueue.map(x=>x.id)).toEqual(["fresh"]);
});
it("polls to recover an unobserved state transition and refreshes on session navigation", async () => {
  mock.snapshot.mockResolvedValueOnce({version:1,items:[entry("q")]}).mockResolvedValue({version:1,items:[entry("q","held"),entry("other","pending","other")]});
  const p=options(); const view=renderHook(({id})=>useSendQueue({...p,sessionId:id}),{initialProps:{id:"s"}});
  await settle(5001); expect(view.result.current.activeQueue[0].externalState).toBe("held");
  view.rerender({id:"other"}); await settle(); expect(mock.listen).toHaveBeenCalledTimes(2);
  expect(mock.snapshot).toHaveBeenCalledTimes(3); expect(view.result.current.activeQueue[0].id).toBe("other");
});
it.each(["pending","held"])("does not auto-send or manually flush a local head across an unresolved Host %s row", async state => {
  const p=options(); const view=renderHook(({busy})=>useSendQueue({...p,sessionState:busy ? "streaming":"ready"}),{initialProps:{busy:true}});
  await settle();
  act(()=>view.result.current.enqueue({storedDisplay:"local",attachments:[],goalMode:false}));
  mock.snapshot.mockResolvedValue({version:1,items:[entry("host",state)]});
  act(()=>view.result.current.externalSync.refresh()); await settle();
  expect(view.result.current.activeQueue.map(x=>x.source??"composer")).toEqual(["composer","external"]);
  p.liveHostRef.current.state="ready" as "streaming"; view.rerender({busy:false}); await settle(200);
  act(()=>view.result.current.resumeFlush()); await settle(200);
  expect(p.executeSendRef.current).not.toHaveBeenCalled();
});
it("does not auto-send or bypass an unavailable Host snapshot, then resumes local-only work after verification", async () => {
  const p=options(); const view=renderHook(({busy})=>useSendQueue({...p,sessionState:busy ? "streaming":"ready"}),{initialProps:{busy:true}});
  await settle(); act(()=>view.result.current.enqueue({storedDisplay:"local",attachments:[],goalMode:false}));
  mock.snapshot.mockRejectedValueOnce(new Error("offline"));
  act(()=>view.result.current.externalSync.refresh()); await settle();
  p.liveHostRef.current.state="ready" as "streaming"; view.rerender({busy:false}); await settle(200);
  act(()=>view.result.current.resumeFlush()); await settle(); expect(p.executeSendRef.current).not.toHaveBeenCalled();
  act(()=>view.result.current.externalSync.refresh()); await settle(); await settle(100);
  expect(p.executeSendRef.current).toHaveBeenCalledTimes(1);
  expect(p.executeSendRef.current).toHaveBeenCalledWith(expect.objectContaining({storedDisplay:"local",targetSessionId:"s",fromQueue:true}));
});

it("reconciles immediately after mirror reconnect without re-registering queue listeners", async () => {
  mock.desktop=false; const p=options(); const {result}=renderHook(()=>useSendQueue(p)); await settle();
  mock.snapshot.mockResolvedValue({version:1,items:[entry("missed","held")]});
  emit("mirror://reconnected",{resumed:true}); await settle();
  expect(result.current.activeQueue[0].id).toBe("missed"); expect(mock.listen).toHaveBeenCalledTimes(3);
  expect(mock.snapshot).toHaveBeenCalledTimes(2); expect(p.executeSendRef.current).not.toHaveBeenCalled();
});
