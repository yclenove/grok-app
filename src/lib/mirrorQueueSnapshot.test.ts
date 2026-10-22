/** @vitest-environment jsdom */
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { sessionQueueSnapshot } from "./api/sessionQueue";
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
      result:{version:1,items:[{sessionId:"s",itemId:"q",prompt:"queued prompt",createdAt:1,state:"held"}]}})}));
  }
  close() { this.readyState=3;this.onclose?.(); }
}
beforeEach(()=>{
  vi.useFakeTimers({toFake:["setTimeout","clearTimeout"]});frames.length=0;
  window.__MIRROR__={token:"synthetic-test-token"};vi.stubGlobal("WebSocket",TestSocket);
});
afterEach(()=>{mirrorCloseTransport();delete window.__MIRROR__;vi.clearAllTimers();vi.useRealTimers();vi.unstubAllGlobals();});
it("maps the public API to the read-only mirror RPC and decodes its actual wire response",async()=>{
  const result=await sessionQueueSnapshot();
  expect(frames).toEqual([{id:expect.any(String),type:"req",method:"session.queueSnapshot",params:{}}]);
  expect(result.items[0]).toEqual({sessionId:"s",itemId:"q",prompt:"queued prompt",createdAt:1,state:"held"});
});
