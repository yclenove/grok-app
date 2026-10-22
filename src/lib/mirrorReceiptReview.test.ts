/** @vitest-environment jsdom */
import { afterEach,beforeEach,expect,it,vi } from "vitest";
import { sessionReceiptReview,sessionReceiptSnapshot } from "./api/sessionReceipts";
import { mirrorCloseTransport } from "./mirrorTransport";
import { MIRROR_WRITE_CATEGORIES } from "./mirrorWriteSurface";
const frames:Array<{id:string;method:string;params:{request?:{decision:string;sessionId:string;key:string;revision:string}}}>=[];
class Socket { static OPEN=1;static CONNECTING=0;readyState=0;onopen:(()=>void)|null=null;onclose:(()=>void)|null=null;onmessage:((e:{data:string})=>void)|null=null;onerror:(()=>void)|null=null;
 constructor(){queueMicrotask(()=>{this.readyState=1;this.onopen?.();});}
 send(raw:string){const f=JSON.parse(raw);frames.push(f);const r=f.params?.request;const result=r?{version:1,sessionId:r.sessionId,key:r.key,revision:r.revision,decision:r.decision,replacementKey:null,replacementStatus:null,queueItemId:null}:{version:1,items:[]};queueMicrotask(()=>this.onmessage?.({data:JSON.stringify({type:"res",id:f.id,ok:true,result})}));}
 close(){this.readyState=3;this.onclose?.();}
}
beforeEach(()=>{vi.useFakeTimers({toFake:["setTimeout","clearTimeout"]});frames.length=0;window.__MIRROR__={token:"synthetic"};vi.stubGlobal("WebSocket",Socket);});
afterEach(()=>{mirrorCloseTransport();delete window.__MIRROR__;vi.clearAllTimers();vi.useRealTimers();vi.unstubAllGlobals();});
it("routes snapshot and explicitly reviewed mutation through authenticated mirror RPC",async()=>{
 await expect(sessionReceiptSnapshot()).resolves.toEqual({version:1,items:[]});
 const request={sessionId:"s",key:"old",revision:"a".repeat(64),prompt:"",decision:"dismiss" as const,reviewed:true,confirmed:true};await sessionReceiptReview(request);
 expect(frames.map(f=>f.method)).toEqual(["session.receiptSnapshot","session.receiptReview"]);expect(frames[1].params).toEqual({request});
});
it("classifies review as send-capable, not read or cancel permission",()=>{expect(MIRROR_WRITE_CATEGORIES.find(c=>c.id==="send")?.methods).toContain("session.receiptReview");expect(MIRROR_WRITE_CATEGORIES.find(c=>c.id==="stop")?.methods).not.toContain("session.receiptReview");});
