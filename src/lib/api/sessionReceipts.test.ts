import { beforeEach, expect, it, vi } from "vitest";
import { parseReceiptAck, parseReceiptSnapshot, sessionReceiptReview, sessionReceiptSnapshot, type ReceiptReviewRequest } from "./sessionReceipts";
const host=vi.hoisted(()=>({has:true,invoke:vi.fn()}));vi.mock("./host",()=>({hasHost:()=>host.has,invoke:host.invoke}));
const id={sessionId:"s",key:"old",revision:"a".repeat(64)};
const request:ReceiptReviewRequest={...id,prompt:"original",decision:"retry",reviewed:true,confirmed:true};
const ack={...id,version:1,decision:"retry",replacementKey:"r-ext-10000000-0000-4000-8000-000000000001",replacementStatus:"unknown",queueItemId:null};
const row={...id,canReview:true,review:null};
beforeEach(()=>{host.has=true;host.invoke.mockReset().mockResolvedValue(ack);});
it("sends exact identity and both human confirmations; accepts only matched ACK",async()=>{
 await expect(sessionReceiptReview({...request,extra:"omit"} as ReceiptReviewRequest)).resolves.toEqual(ack);
 expect(host.invoke).toHaveBeenCalledExactlyOnceWith("session_api_receipt_review",{request});
});
it("reads versioned receipts without exposing or synthesizing missing prompts",async()=>{
 host.invoke.mockResolvedValue({version:1,items:[row]});expect(await sessionReceiptSnapshot()).toEqual({version:1,items:[row]});
 expect(host.invoke).toHaveBeenCalledExactlyOnceWith("session_api_receipt_snapshot");
});
it("permits dismiss without prompt but no replacement or claimed success",async()=>{
 const dismissed={...ack,decision:"dismiss",replacementKey:null,replacementStatus:null};host.invoke.mockResolvedValue(dismissed);
 expect(await sessionReceiptReview({...request,decision:"dismiss",prompt:""})).toEqual(dismissed);
});
it.each([{reviewed:false},{confirmed:false},{reviewed:"true"},{confirmed:1},{decision:"cancel"},{sessionId:"imp:x"},{sessionId:"__draft__"},{sessionId:" s"},{key:""},{key:"old "},{revision:"x"},{revision:12},{prompt:" "},{prompt:null}])("rejects invalid mutation before IPC %j",async patch=>{
 await expect(sessionReceiptReview({...request,...patch} as ReceiptReviewRequest)).rejects.toThrow("SESSION_RECEIPT_INVALID");expect(host.invoke).not.toHaveBeenCalled();
});
it.each([null,[],{}, {...ack,version:2},{...ack,key:"wrong"},{...ack,sessionId:"other"},{...ack,revision:"b".repeat(64)},
 {...ack,replacementKey:"r-ext-"+"-".repeat(36)},{...ack,replacementKey:"old"},{...ack,replacementKey:null},
 {...ack,replacementStatus:"retry_later"},{...ack,replacementStatus:null},{...ack,replacementStatus:"queued"},
 {...ack,queueItemId:"q"},{...ack,extra:1}])("rejects malformed or mismatched ACK %j",async value=>{
 host.invoke.mockResolvedValue(value);await expect(sessionReceiptReview(request)).rejects.toThrow("SESSION_RECEIPT_UNAVAILABLE");
});
it.each([null,[],{}, {version:2,items:[]},{version:1,items:[row,row]},{version:1,items:[{...row,extra:1}]},
 {version:1,items:[{...row,canReview:"true"}]},{version:1,items:[{...row,review:ack}]},
 {version:1,items:[{...row,canReview:false,review:{...ack,key:"other"}}]},{version:1,items:Array(201).fill(row)}])("fails closed on malformed snapshots %j",value=>{
 expect(()=>parseReceiptSnapshot(value)).toThrow("SESSION_RECEIPT_UNAVAILABLE");
});
it("parses legitimate queued and recorded review states without changing original identity",()=>{
 const queued={...ack,replacementStatus:"queued",queueItemId:"q"};expect(parseReceiptAck(queued)).toEqual(queued);
 expect(parseReceiptSnapshot({version:1,items:[{...row,canReview:false,review:queued}]}).items[0].review).toEqual(queued);
});
it("no Host refuses and authoritative errors propagate without automatic retry",async()=>{
 host.has=false;await expect(sessionReceiptSnapshot()).rejects.toThrow();await expect(sessionReceiptReview(request)).rejects.toThrow();expect(host.invoke).not.toHaveBeenCalled();
 host.has=true;host.invoke.mockRejectedValue(new Error("SESSION_RECEIPT_ACTIVE"));await expect(sessionReceiptReview(request)).rejects.toThrow("SESSION_RECEIPT_ACTIVE");expect(host.invoke).toHaveBeenCalledTimes(1);
});
