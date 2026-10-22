/** @vitest-environment jsdom */
import {act,cleanup,renderHook} from "@testing-library/react";
import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {useExternalQueueReview} from "./useExternalQueueReview";
import type {QueuedSend} from "@/lib/sendQueue";
const api=vi.hoisted(()=>({recover:vi.fn(),cancel:vi.fn()}));
vi.mock("@/lib/api/sessionQueue",()=>({sessionQueueCancel:api.cancel,sessionQueueRecover:api.recover}));
const item:QueuedSend={id:"old",storedDisplay:" exact prompt ",createdAt:1,source:"external",externalState:"held",attachments:[],goalMode:false};
const ack={version:1,decision:"retry",sessionId:"s",itemId:"old",replacementId:"q-ext-new"};
const options=()=>({sessionId:"s" as string|null,queue:[item],sync:{enabled:true,ready:true,loading:false,error:false,refresh:vi.fn()}});
function deferred(){let resolve!:(value:unknown)=>void;let reject!:(reason:unknown)=>void;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return{promise,resolve,reject};}
beforeEach(()=>{api.recover.mockReset().mockResolvedValue(ack);api.cancel.mockReset();});
afterEach(cleanup);
it.each(["pending","held"] as const)("requires independent review and risk confirmations for %s",async state=>{
 const p=options();p.queue=[{...item,externalState:state}];const{result}=renderHook(()=>useExternalQueueReview(p));
 act(()=>result.current.open(p.queue[0]));
 await act(async()=>result.current.recover("retry"));expect(api.recover).not.toHaveBeenCalled();
 act(()=>result.current.setReviewed(true));await act(async()=>result.current.recover("retry"));expect(api.recover).not.toHaveBeenCalled();
 act(()=>{result.current.setReviewed(false);result.current.setRiskConfirmed(true);});
 await act(async()=>result.current.recover("retry"));expect(api.recover).not.toHaveBeenCalled();
 act(()=>result.current.setReviewed(true));await act(async()=>result.current.recover("retry"));
 expect(api.recover).toHaveBeenCalledExactlyOnceWith({sessionId:"s",itemId:"old",prompt:" exact prompt ",createdAt:1,decision:"retry",reviewed:true,confirmed:true});
 expect(result.current.done).toBe(true);expect(result.current.resolution).toEqual(ack);expect(result.current.canRecover).toBe(false);
 expect(result.current.reviewed).toBe(false);expect(result.current.riskConfirmed).toBe(false);
 expect(p.queue).toEqual([{...item,externalState:state}]);expect(p.sync.refresh).toHaveBeenCalledTimes(1);
 expect(api.cancel).not.toHaveBeenCalled();
});
it("dismiss is a separate confirmed decision with no fresh attempt",async()=>{
 api.recover.mockResolvedValue({...ack,decision:"dismiss",replacementId:null});const p=options();const{result}=renderHook(()=>useExternalQueueReview(p));
 act(()=>{result.current.open(item);result.current.setReviewed(true);result.current.setRiskConfirmed(true);});
 await act(async()=>result.current.recover("dismiss"));
 expect(api.recover.mock.calls[0][0].decision).toBe("dismiss");expect(result.current.resolution?.replacementId).toBeNull();
 expect(p.queue).toEqual([item]);expect(api.cancel).not.toHaveBeenCalled();
});
it.each(["queued","missing","prompt","timestamp","unready","error","disabled","local"] as const)("refuses stale or non-uncertain row %s",async condition=>{
 const p=options();const{result,rerender}=renderHook(x=>useExternalQueueReview(x),{initialProps:p});
 act(()=>{result.current.open(item);result.current.setReviewed(true);result.current.setRiskConfirmed(true);});
 const n={...p,queue:[...p.queue],sync:{...p.sync}};
 if(condition==="queued")n.queue=[{...item,externalState:"queued"}];
 if(condition==="missing")n.queue=[];
 if(condition==="prompt")n.queue=[{...item,storedDisplay:"other"}];
 if(condition==="timestamp")n.queue=[{...item,createdAt:2}];
 if(condition==="local")n.queue=[{...item,source:undefined}];
 if(condition==="unready")n.sync.ready=false;if(condition==="error")n.sync.error=true;if(condition==="disabled")n.sync.enabled=false;
 rerender(n);await act(async()=>result.current.recover("retry"));expect(api.recover).not.toHaveBeenCalled();
});
it("serializes dismiss, retry and cancel; closing or reopening cannot bypass an active call",async()=>{
 const d=deferred();api.recover.mockReturnValue(d.promise);const p=options();const{result}=renderHook(()=>useExternalQueueReview(p));
 act(()=>{result.current.open(item);result.current.setReviewed(true);result.current.setRiskConfirmed(true);});
 act(()=>{void result.current.recover("retry");void result.current.recover("dismiss");void result.current.cancel();});
 expect(api.recover).toHaveBeenCalledTimes(1);expect(api.cancel).not.toHaveBeenCalled();expect(result.current.busy).toBe(true);
 act(()=>{result.current.close();result.current.open(item);result.current.setReviewed(true);result.current.setRiskConfirmed(true);});
 await act(async()=>result.current.recover("retry"));expect(api.recover).toHaveBeenCalledTimes(1);
 await act(async()=>d.resolve(ack));expect(result.current.done).toBe(false);expect(result.current.resolution).toBeNull();
 expect(result.current.busy).toBe(false);expect(p.sync.refresh).toHaveBeenCalledTimes(1);
});
it.each(["success","failure"] as const)("late %s cannot mutate the newly selected session",async outcome=>{
 const d=deferred();api.recover.mockReturnValue(d.promise);const p=options();
 const{result,rerender}=renderHook(x=>useExternalQueueReview(x),{initialProps:p});
 act(()=>{result.current.open(item);result.current.setReviewed(true);result.current.setRiskConfirmed(true);});
 act(()=>{void result.current.recover("retry");});rerender({...p,sessionId:"other"});
 expect(result.current.target).toBeNull();act(()=>result.current.open(item));
 await act(async()=>{if(outcome==="success")d.resolve(ack);else d.reject(new Error("SESSION_QUEUE_ACTIVE"));});
 expect(result.current.done).toBe(false);expect(result.current.error).toBeNull();expect(result.current.resolution).toBeNull();
 expect(result.current.reviewed).toBe(false);expect(result.current.riskConfirmed).toBe(false);
});
it.each([["SESSION_QUEUE_ACTIVE","active"],["SESSION_QUEUE_CHANGED","changed"],["private/path/secret","unavailable"]])("sanitizes %s and never removes or recreates the row",async(message,error)=>{
 api.recover.mockRejectedValue(new Error(message));const p=options();const{result}=renderHook(()=>useExternalQueueReview(p));
 act(()=>{result.current.open(item);result.current.setReviewed(true);result.current.setRiskConfirmed(true);});
 await act(async()=>result.current.recover("retry"));expect(result.current.error).toBe(error);expect(result.current.done).toBe(false);
 expect(p.queue).toEqual([item]);expect(result.current.resolution).toBeNull();expect(api.recover).toHaveBeenCalledTimes(1);
});
it("unmount suppresses late UI mutation and refresh",async()=>{
 const d=deferred();api.recover.mockReturnValue(d.promise);const p=options();const{result,unmount}=renderHook(()=>useExternalQueueReview(p));
 act(()=>{result.current.open(item);result.current.setReviewed(true);result.current.setRiskConfirmed(true);});
 act(()=>{void result.current.recover("retry");});unmount();await act(async()=>d.resolve(ack));expect(p.sync.refresh).not.toHaveBeenCalled();
});
