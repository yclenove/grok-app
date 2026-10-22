/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useImportContinuation } from "./useImportContinuation";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import { defaultSelection } from "@/lib/sessionImport/selection";
const api = vi.hoisted(() => ({ sessionImportContinueSelection: vi.fn(), sessionImportContinueCreate: vi.fn(),
  sessionImportContinueRestore: vi.fn(), sessionImportContinueWorkspace: vi.fn(), sessionImportContinueDiscard: vi.fn() }));
vi.mock("@/lib/api", () => api);
const original: HandoffDraft = { draftId:"old", draftHash:"old-hash", importId:"imp:1", revisionId:"r1", leaf:"leaf",
  text:"includes removed text", coverage:"full", estimatedTokens:4, runtimeCapability:"local-full",
  selection:{ ...defaultSelection(), eventIds:["a","b"], pinnedEventIds:["b"], assetIds:["image"] } };
const selection = { ...original.selection!, eventIds:["a"], pinnedEventIds:[], assetIds:[] };
const revised = { ...original, draftId:"new", draftHash:"new-hash", text:"regenerated", selection };
const activity = vi.fn();
function deferred<T>() { let resolve!: (value:T)=>void;const promise=new Promise<T>((ok)=>{resolve=ok;});return {promise,resolve}; }
beforeEach(()=>{vi.resetAllMocks();api.sessionImportContinueSelection.mockResolvedValue(revised);api.sessionImportContinueCreate.mockResolvedValue({appSessionId:"native"});api.sessionImportContinueRestore.mockRejectedValue(new Error("offline"));});
afterEach(cleanup);
function setup(initial = original) { const hook=renderHook(()=>useImportContinuation(null,activity));act(()=>hook.result.current.setDraft(initial));return hook; }
test("scope review synchronously blocks creation, workspace and text edits before rerender",async()=>{
 const {result}=setup();
 await act(async()=>{result.current.beginContinuationScopeReview();await result.current.createContinue();await result.current.selectContinuationWorkspace("w");result.current.setDraft({...original,text:"unsafe edit"});});
 expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();expect(api.sessionImportContinueWorkspace).not.toHaveBeenCalled();
 expect(result.current.draft?.text).toBe(original.text);
 act(()=>result.current.cancelContinuationScopeReview());await act(async()=>{await result.current.createContinue();});expect(api.sessionImportContinueCreate).toHaveBeenCalledWith("old","old-hash","continuation:old",original.text);
});
test("apply replaces old identity and edited text; duplicate applies create only one request",async()=>{
 const {result}=setup();const reply=deferred<HandoffDraft>();api.sessionImportContinueSelection.mockReturnValueOnce(reply.promise);
 let task!:Promise<boolean>;act(()=>{result.current.beginContinuationScopeReview();task=result.current.selectContinuationScope(selection);});
 await act(async()=>{expect(await result.current.selectContinuationScope(selection)).toBe(false);expect(await result.current.createContinue()).toBeNull();});
 await act(async()=>{reply.resolve(revised);expect(await task).toBe(true);});
 expect(api.sessionImportContinueSelection).toHaveBeenCalledTimes(1);expect(result.current.draft).toEqual(revised);
 expect(result.current.continuationReviewingScope).toBe(false);await act(async()=>{await result.current.createContinue();});
 expect(api.sessionImportContinueCreate).toHaveBeenLastCalledWith("new","new-hash","continuation:new","regenerated");
});
test("lost scope replies freeze the original deep-copied selection for retry",async()=>{
 const {result}=setup();api.sessionImportContinueSelection.mockRejectedValueOnce(new Error("lost reply"));
 const input=structuredClone(selection);await act(async()=>{await result.current.selectContinuationScope(input);});input.eventIds.push("b");
 expect(result.current.continuationScopePending).toBe(true);act(()=>result.current.cancelContinuationScopeReview());
 expect(result.current.continuationReviewingScope).toBe(true);await act(async()=>{expect(await result.current.discardContinuation()).toBe(false);await result.current.selectContinuationScope(original.selection!);});
 expect(api.sessionImportContinueDiscard).not.toHaveBeenCalled();expect(api.sessionImportContinueSelection.mock.calls[0]).toEqual(api.sessionImportContinueSelection.mock.calls[1]);expect(api.sessionImportContinueSelection).toHaveBeenLastCalledWith("old","old-hash",selection);
});
test("Host-proven unchanged drafts unlock scope editing but do not allow create until review ends",async()=>{
 const {result}=setup();api.sessionImportContinueSelection.mockRejectedValueOnce(new Error("SOURCE_BUSY"));api.sessionImportContinueRestore.mockResolvedValueOnce(original);
 await act(async()=>{await result.current.selectContinuationScope(selection);});expect(result.current.continuationScopePending).toBe(false);expect(result.current.continuationReviewingScope).toBe(true);
 await act(async()=>{await result.current.createContinue();});expect(api.sessionImportContinueCreate).not.toHaveBeenCalled();
});
test("late selection replies cannot reopen a closed or replacement reader",async()=>{
 const {result}=setup();const reply=deferred<HandoffDraft>();api.sessionImportContinueSelection.mockReturnValueOnce(reply.promise);
 let task!:Promise<boolean>;act(()=>{task=result.current.selectContinuationScope(selection);});act(()=>result.current.invalidateContinuation());
 await act(async()=>{reply.resolve(revised);expect(await task).toBe(false);});expect(result.current.draft).toBeNull();expect(result.current.continuationScopePending).toBe(false);
});
test("claimed drafts cannot begin or apply scope review",async()=>{
 const {result}=setup({...original,creationRequestId:"claimed"});act(()=>{result.current.beginContinuationScopeReview();});
 await act(async()=>{expect(await result.current.selectContinuationScope(selection)).toBe(false);});expect(api.sessionImportContinueSelection).not.toHaveBeenCalled();expect(result.current.continuationReviewingScope).toBe(false);
});
