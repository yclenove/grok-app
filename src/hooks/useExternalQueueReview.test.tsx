/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useExternalQueueReview } from "./useExternalQueueReview";
import type { QueuedSend } from "@/lib/sendQueue";
const api = vi.hoisted(() => ({ cancel: vi.fn(), recover: vi.fn() }));
vi.mock("@/lib/api/sessionQueue", () => ({ sessionQueueCancel: api.cancel, sessionQueueRecover: api.recover }));
const item: QueuedSend = { id: "q", storedDisplay: " exact prompt ", createdAt: 1, source: "external", externalState: "queued", attachments: [], goalMode: false };
function options() { return { sessionId: "s" as string | null, queue: [item], sync: { enabled: true, ready: true, loading: false, error: false, refresh: vi.fn() } }; }
function deferred() { let resolve!: () => void; let reject!: (error: unknown) => void; const promise = new Promise<void>((a,b) => { resolve=a; reject=b; }); return {promise,resolve,reject}; }
beforeEach(() => { api.cancel.mockReset().mockResolvedValue(undefined); });
afterEach(cleanup);
it("requires explicit confirmation and keeps local queue until a Host snapshot replaces it", async () => {
  const p=options(); const {result}=renderHook(() => useExternalQueueReview(p));
  act(() => result.current.open(item));
  await act(async () => result.current.cancel()); expect(api.cancel).not.toHaveBeenCalled();
  act(() => result.current.setConfirmed(true));
  await act(async () => result.current.cancel());
  expect(api.cancel).toHaveBeenCalledExactlyOnceWith({sessionId:"s",itemId:"q",prompt:" exact prompt ",createdAt:1});
  expect(result.current.done).toBe(true); expect(p.queue).toEqual([item]); expect(p.sync.refresh).toHaveBeenCalledTimes(1);
});
it.each(["pending","held",undefined] as const)("never cancels an unverified dispatch state %s", async state => {
  const p=options();p.queue=[{...item,externalState:state}];const {result}=renderHook(() => useExternalQueueReview(p));
  act(() => {result.current.open(p.queue[0]);result.current.setConfirmed(true);});
  await act(async () => result.current.cancel());expect(api.cancel).not.toHaveBeenCalled();
});
it.each(["unready","error","changed","missing","disabled"])("rejects stale snapshot or identity: %s", async condition => {
  const p=options();const {result,rerender}=renderHook(x => useExternalQueueReview(x),{initialProps:p});
  act(() => {result.current.open(item);result.current.setConfirmed(true);});
  const next={...p,queue:[...p.queue],sync:{...p.sync}};
  if(condition==="unready")next.sync.ready=false;
  if(condition==="error")next.sync.error=true;
  if(condition==="disabled")next.sync.enabled=false;
  if(condition==="missing")next.queue=[];
  if(condition==="changed")next.queue=[{...item,storedDisplay:"different"}];
  rerender(next);await act(async () => result.current.cancel());expect(api.cancel).not.toHaveBeenCalled();
});
it("serializes duplicate clicks and allows dismissal without misreporting an unresolved mutation", async () => {
  const d=deferred();api.cancel.mockReturnValue(d.promise);
  const p=options();const {result}=renderHook(() => useExternalQueueReview(p));
  act(() => {result.current.open(item);result.current.setConfirmed(true);});
  act(() => {void result.current.cancel();void result.current.cancel();});
  expect(api.cancel).toHaveBeenCalledTimes(1);expect(result.current.busy).toBe(true);
  act(() => result.current.close());expect(result.current.target).toBeNull();
  act(() => result.current.open(item));expect(result.current.canCancel).toBe(false);
  await act(async () => d.resolve());expect(result.current.done).toBe(false);
  expect(p.sync.refresh).toHaveBeenCalledTimes(1);expect(result.current.busy).toBe(false);
});
it("late completion cannot alter a different session or a newly opened review", async () => {
  const d=deferred();api.cancel.mockReturnValue(d.promise);
  const p=options();const {result,rerender}=renderHook(x => useExternalQueueReview(x),{initialProps:p});
  act(() => {result.current.open(item);result.current.setConfirmed(true);});
  act(() => {void result.current.cancel();});
  rerender({...p,sessionId:"other"});expect(result.current.target).toBeNull();
  act(() => result.current.open(item));
  await act(async () => d.resolve());expect(result.current.target?.sessionId).toBe("other");expect(result.current.done).toBe(false);
});
it.each([["SESSION_QUEUE_CLAIMED","claimed"],["SESSION_QUEUE_CHANGED","changed"],["private/path/token","unavailable"]])("shows safe failure %s without removing data",async(message,expected)=>{
  api.cancel.mockRejectedValue(new Error(message));
  const p=options();const {result}=renderHook(()=>useExternalQueueReview(p));
  act(()=>{result.current.open(item);result.current.setConfirmed(true);});
  await act(async()=>result.current.cancel());
  expect(result.current.error).toBe(expected);expect(result.current.done).toBe(false);expect(p.queue).toEqual([item]);
});
it("unmount does not turn late success into a new UI mutation",async()=>{
  const d=deferred();api.cancel.mockReturnValue(d.promise);const p=options();
  const {result,unmount}=renderHook(()=>useExternalQueueReview(p));
  act(()=>{result.current.open(item);result.current.setConfirmed(true);});
  act(()=>{void result.current.cancel();});unmount();await act(async()=>d.resolve());
  expect(p.sync.refresh).not.toHaveBeenCalled();
});
