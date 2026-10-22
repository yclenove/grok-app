import { beforeEach, expect, it, vi } from "vitest";
import { sessionQueueRecover, type QueueRecoveryRequest } from "./sessionQueue";
const host=vi.hoisted(()=>({has:true,invoke:vi.fn()}));
vi.mock("./host",()=>({hasHost:()=>host.has,invoke:host.invoke}));
const request:QueueRecoveryRequest={sessionId:"s",itemId:"old",prompt:" exact prompt ",createdAt:1,decision:"retry",reviewed:true,confirmed:true};
const ack={version:1,decision:"retry",sessionId:"s",itemId:"old",replacementId:"q-ext-new"};
beforeEach(()=>{host.has=true;host.invoke.mockReset().mockResolvedValue(ack);});
it("sends exact identity and both human confirmations, stripping extra caller properties",async()=>{
  await expect(sessionQueueRecover({...request,extra:"not-on-wire"} as QueueRecoveryRequest)).resolves.toEqual(ack);
  expect(host.invoke.mock.calls).toEqual([["session_api_queue_recover",{request}]]);
});
it("requires a null replacement for dismiss and does not fabricate a cancellation",async()=>{
  const dismissed={...ack,decision:"dismiss",replacementId:null};host.invoke.mockResolvedValue(dismissed);
  await expect(sessionQueueRecover({...request,decision:"dismiss"})).resolves.toEqual(dismissed);
});
it.each([
  {reviewed:false},{confirmed:false},{decision:"cancel"},{sessionId:"__draft__"},{sessionId:"imp:x"},
  {sessionId:" s"},{sessionId:""},{itemId:"old "},{itemId:""},{prompt:" \n "},{createdAt:NaN},
  {createdAt:1.5},{createdAt:Number.MAX_SAFE_INTEGER+1},{reviewed:"true"},{confirmed:1}
])("rejects invalid request before IPC: %j",async patch=>{
  await expect(sessionQueueRecover({...request,...patch} as QueueRecoveryRequest)).rejects.toThrow("SESSION_QUEUE_INVALID");
  expect(host.invoke).not.toHaveBeenCalled();
});
it.each([
  null,[],{}, {...ack,version:2},{...ack,decision:"dismiss"},{...ack,sessionId:"other"},{...ack,itemId:"other"},
  {...ack,replacementId:"old"},{...ack,replacementId:null},{...ack,replacementId:" "},{...ack,replacementId:" x"},
  {...ack,replacementId:42},{...ack,status:"started"}
])("rejects malformed or mismatched acknowledgement: %j",async result=>{
  host.invoke.mockResolvedValue(result);
  await expect(sessionQueueRecover(request)).rejects.toThrow("SESSION_QUEUE_UNAVAILABLE");
});
it("does not accept a new-attempt identity as a dismiss acknowledgement",async()=>{
  host.invoke.mockResolvedValue({...ack,decision:"dismiss"});
  await expect(sessionQueueRecover({...request,decision:"dismiss"})).rejects.toThrow("SESSION_QUEUE_UNAVAILABLE");
});
it("rejects no Host and propagates authoritative refusal without retry",async()=>{
  host.has=false;await expect(sessionQueueRecover(request)).rejects.toThrow("SESSION_QUEUE_UNAVAILABLE");
  expect(host.invoke).not.toHaveBeenCalled();host.has=true;
  host.invoke.mockRejectedValue(new Error("SESSION_QUEUE_ACTIVE"));
  await expect(sessionQueueRecover(request)).rejects.toThrow("SESSION_QUEUE_ACTIVE");
  expect(host.invoke).toHaveBeenCalledTimes(1);
});
