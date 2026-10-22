import { MIRROR_WRITE_CATEGORIES } from "./mirrorWriteSurface";
/** @vitest-environment jsdom */
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { sessionQueueRecover } from "./api/sessionQueue";
import { mirrorCloseTransport } from "./mirrorTransport";

const frames: Array<{ id: string; type: string; method: string; params: unknown }> = [];
class TestSocket {
  static OPEN=1; static CONNECTING=0;
  readyState=0;
  onopen: (()=>void)|null=null;
  onclose: (()=>void)|null=null;
  onmessage: ((event:{data:string})=>void)|null=null;
  onerror: (()=>void)|null=null;
  constructor() { queueMicrotask(()=>{this.readyState=1;this.onopen?.();}); }
  send(raw:string) {
    const frame=JSON.parse(raw); frames.push(frame);
    queueMicrotask(()=>this.onmessage?.({data:JSON.stringify({type:"res",id:frame.id,ok:true,
      result:{version:1,decision:frame.params.request.decision,sessionId:"s",itemId:"q",replacementId:frame.params.request.decision==="retry"?"q-ext-new":null}})}));
  }
  close() { this.readyState=3;this.onclose?.(); }
}
beforeEach(()=>{
  vi.useFakeTimers({toFake:["setTimeout","clearTimeout"]});frames.length=0;
  window.__MIRROR__={token:"synthetic-test-token"};vi.stubGlobal("WebSocket",TestSocket);
});
afterEach(()=>{mirrorCloseTransport();delete window.__MIRROR__;vi.clearAllTimers();vi.useRealTimers();vi.unstubAllGlobals();});
it.each(["dismiss","retry"] as const)("maps confirmed %s to authenticated write RPC",async decision=>{
 const request={sessionId:"s",itemId:"q",prompt:"exact prompt",createdAt:1,decision,reviewed:true,confirmed:true};
 const result=await sessionQueueRecover(request);
 expect(frames).toEqual([{id:expect.any(String),type:"req",method:"session.queueRecover",params:{request}}]);
 expect(result).toEqual({version:1,decision,sessionId:"s",itemId:"q",replacementId:decision==="retry"?"q-ext-new":null});
});
it("classifies recovery as send-capable, never as cancellation-only permission",()=>{
 expect(MIRROR_WRITE_CATEGORIES.find(c=>c.id==="send")?.methods).toContain("session.queueRecover");
 expect(MIRROR_WRITE_CATEGORIES.find(c=>c.id==="stop")?.methods).not.toContain("session.queueRecover");
});
