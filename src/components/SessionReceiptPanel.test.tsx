/** @vitest-environment jsdom */
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { createT, loadLocaleCatalog, type Locale } from "@/i18n";
import { SessionReceiptPanel } from "./SessionReceiptPanel";
import type { ReceiptReviewAck, ReceiptRow } from "@/lib/api/sessionReceipts";
const host=vi.hoisted(()=>({has:true,invoke:vi.fn()}));vi.mock("@/lib/api/host",()=>({hasHost:()=>host.has,invoke:host.invoke}));
const locales:Locale[]=["en","de","es","fr","it","pt-BR","ru","uk","zh","zh-TW","ja","ko","id","fil","ta"];
const row:ReceiptRow={sessionId:"s",key:"old",revision:"a".repeat(64),canReview:true,review:null};
const retryAck:ReceiptReviewAck={version:1,...row,decision:"retry",replacementKey:"r-ext-10000000-0000-4000-8000-000000000001",replacementStatus:"unknown",queueItemId:null};
// Build wire ACK without row-only properties: production parser remains in this test.
function ack(decision:"retry"|"dismiss"):ReceiptReviewAck { return {version:1,sessionId:row.sessionId,key:row.key,revision:row.revision,decision,replacementKey:decision==="retry"?retryAck.replacementKey:null,replacementStatus:decision==="retry"?"unknown":null,queueItemId:null}; }
let items:ReceiptRow[]=[];
beforeAll(async()=>{await Promise.all(locales.map(loadLocaleCatalog));});
beforeEach(()=>{host.has=true;items=[{...row}];host.invoke.mockReset().mockImplementation(async(cmd:string,args?:{request:{decision:"retry"|"dismiss"}})=>{
 if(cmd==="session_api_receipt_snapshot")return {version:1,items:structuredClone(items)};
 if(cmd==="session_api_receipt_review"){const a=ack(args!.request.decision);items=[{...row,canReview:false,review:a}];return a;}throw Error("unexpected IPC");
});});
afterEach(cleanup);
async function open(locale:Locale="en") {const t=createT(locale);render(<SessionReceiptPanel t={t}/>);fireEvent.click(screen.getByRole("button",{name:t("receipt.title")}));await screen.findByRole("button",{name:/old/});fireEvent.click(screen.getByRole("button",{name:/old/}));return t;}
function confirmations(){for(const checkbox of screen.getAllByRole("checkbox"))fireEvent.click(checkbox);}
it.each(locales)("real modal/hook/API allows a single reviewed retry in %s",async locale=>{
 const t=await open(locale);const retry=screen.getByRole("button",{name:t("receipt.retry")}) as HTMLButtonElement;
 expect(retry.disabled).toBe(true);confirmations();expect(retry.disabled).toBe(true);
 fireEvent.change(screen.getByRole("textbox"),{target:{value:"original"}});expect(retry.disabled).toBe(false);
 fireEvent.click(retry);fireEvent.click(retry);
 await screen.findByText(t("receipt.recorded"));expect(screen.getByText(retryAck.replacementKey!)).toBeTruthy();expect(screen.getByText(t("receipt.unknown"))).toBeTruthy();
 expect(host.invoke.mock.calls.filter(c=>c[0]==="session_api_receipt_review")).toHaveLength(1);expect(screen.queryByRole("checkbox")).toBeNull();
 fireEvent.keyDown(document,{key:"Escape"});expect(screen.queryByRole("dialog")).toBeNull();
});
it("dismiss needs both confirmations but no prompt and retains visible review history",async()=>{
 const t=await open();const dismiss=screen.getByRole("button",{name:t("composer.queueRecoverDismiss")}) as HTMLButtonElement;
 fireEvent.click(screen.getAllByRole("checkbox")[0]);expect(dismiss.disabled).toBe(true);fireEvent.click(screen.getAllByRole("checkbox")[1]);fireEvent.click(dismiss);
 await screen.findByText(t("composer.queueRecoverDismissed"));fireEvent.click(screen.getByRole("button",{name:t("receipt.back")}));
 expect(await screen.findByRole("button",{name:/old/})).toBeTruthy();expect(screen.getByText(t("receipt.reviewed"))).toBeTruthy();
});
it.each(["SESSION_RECEIPT_ACTIVE","SESSION_RECEIPT_CHANGED","SESSION_RECEIPT_PROMPT_MISMATCH","SESSION_RECEIPT_CAPACITY","SESSION_RECEIPT_UNAVAILABLE"])("shows authoritative failure %s without retry or false completion",async error=>{
 const t=await open();host.invoke.mockImplementation(async(cmd:string)=>{if(cmd==="session_api_receipt_review")throw Error(error);return {version:1,items:[row]};});
 confirmations();fireEvent.change(screen.getByRole("textbox"),{target:{value:"original"}});fireEvent.click(screen.getByRole("button",{name:t("receipt.retry")}));
 await screen.findByRole("alert");expect(screen.queryByText(t("receipt.recorded"))).toBeNull();expect(host.invoke.mock.calls.filter(c=>c[0]==="session_api_receipt_review")).toHaveLength(1);
});
it("close/reopen during an in-flight action does not duplicate it or leak late ACK into another view",async()=>{
 const t=await open();let resolve!:(value:unknown)=>void;
 host.invoke.mockImplementation((cmd:string)=>cmd==="session_api_receipt_review"?new Promise(r=>{resolve=r;}):Promise.resolve({version:1,items:structuredClone(items)}));
 confirmations();fireEvent.change(screen.getByRole("textbox"),{target:{value:"original"}});fireEvent.click(screen.getByRole("button",{name:t("receipt.retry")}));
 fireEvent.keyDown(document,{key:"Escape"});fireEvent.click(screen.getByRole("button",{name:t("receipt.title")}));await screen.findByRole("button",{name:/old/});
 fireEvent.click(screen.getByRole("button",{name:/old/}));expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
 items=[{...row,canReview:false,review:ack("retry")}];await act(async()=>{resolve(ack("retry"));});await screen.findByText(t("receipt.recorded"));
 expect(host.invoke.mock.calls.filter(c=>c[0]==="session_api_receipt_review")).toHaveLength(1);
});
it("legacy rows are view-only; failed refresh cannot authorize a stale row",async()=>{
 items=[{...row,canReview:false}];const t=await open();expect(screen.getByText(t("receipt.legacy"))).toBeTruthy();for(const c of screen.getAllByRole("checkbox"))expect((c as HTMLInputElement).disabled).toBe(true);
 host.invoke.mockRejectedValue(Error("offline"));fireEvent.click(screen.getByRole("button",{name:t("receipt.refresh")}));await screen.findByRole("alert");expect((screen.getByRole("button",{name:t("receipt.retry")}) as HTMLButtonElement).disabled).toBe(true);
});
it("renders empty and no-Host states without mutation",async()=>{
 items=[];const t=createT("en");const v=render(<SessionReceiptPanel t={t}/>);fireEvent.click(screen.getByRole("button",{name:t("receipt.title")}));await screen.findByText(t("receipt.empty"));
 fireEvent.keyDown(document,{key:"Escape"});host.has=false;v.rerender(<SessionReceiptPanel t={t}/>);expect((screen.getByRole("button",{name:t("receipt.title")}) as HTMLButtonElement).disabled).toBe(true);
 expect(host.invoke.mock.calls.every(c=>c[0]==="session_api_receipt_snapshot")).toBe(true);
});
it("ignores a late snapshot after close and never stores prompt in localStorage",async()=>{
 let resolve!:(value:unknown)=>void;host.invoke.mockImplementation(()=>new Promise(r=>{resolve=r;}));const t=createT("en");render(<SessionReceiptPanel t={t}/>);
 fireEvent.click(screen.getByRole("button",{name:t("receipt.title")}));fireEvent.keyDown(document,{key:"Escape"});await act(async()=>resolve({version:1,items:[row]}));
 await waitFor(()=>expect(screen.queryByRole("dialog")).toBeNull());expect(localStorage.getItem("receipt.prompt")).toBeNull();
});

it.each(["dismiss", "retry"] as const)("focuses the retained %s acknowledgement after the action disappears", async decision => {
 const t=await open(); confirmations(); fireEvent.change(screen.getByRole("textbox"),{target:{value:"original"}});
 const button=screen.getByRole("button",{name:t(decision==="retry"?"receipt.retry":"composer.queueRecoverDismiss")}); button.focus(); fireEvent.click(button);
 const message=await screen.findByText(t(decision==="retry"?"receipt.recorded":"composer.queueRecoverDismissed"));
 await waitFor(()=>expect(document.activeElement).toBe(message));
});
it("keeps focus on pending feedback while dispatch is unresolved", async()=>{
 const t=await open(); let resolve!:(v:unknown)=>void;
 host.invoke.mockImplementation((cmd:string)=>cmd==="session_api_receipt_review"?new Promise(r=>{resolve=r;}):Promise.resolve({version:1,items:[row]}));
 confirmations(); const button=screen.getByRole("button",{name:t("composer.queueRecoverDismiss")}); button.focus(); fireEvent.click(button);
 expect(document.activeElement).toBe(screen.getByText(t("composer.queueRecovering")));
 await act(async()=>resolve(ack("dismiss")));
});
it("moves removed pending feedback focus to the authoritative error", async()=>{
 const t=await open(); let reject!:(v:unknown)=>void;
 host.invoke.mockImplementation((cmd:string)=>cmd==="session_api_receipt_review"?new Promise((_,r)=>{reject=r;}):Promise.resolve({version:1,items:[row]}));
 confirmations(); const button=screen.getByRole("button",{name:t("composer.queueRecoverDismiss")}); button.focus(); fireEvent.click(button);
 await act(async()=>reject(Error("SESSION_RECEIPT_ACTIVE")));
 const error=await screen.findByRole("alert"); await waitFor(()=>expect(document.activeElement).toBe(error));
});
it("returns focus to the inspected row when navigating back to the list", async()=>{
 items=[{...row,key:"first"},{...row,key:"second"}];const t=createT("en");render(<SessionReceiptPanel t={t}/>);
 fireEvent.click(screen.getByRole("button",{name:t("receipt.title")}));const second=await screen.findByRole("button",{name:/second/});second.focus();fireEvent.click(second);
 const back=screen.getByRole("button",{name:t("receipt.back")});back.focus();fireEvent.click(back);
 expect(document.activeElement).toBe(screen.getByRole("button",{name:/second/}));
});
it("does not steal valid dialog focus when a pending acknowledgement arrives", async()=>{
 const t=await open();let resolve!:(v:unknown)=>void;
 host.invoke.mockImplementation((cmd:string)=>cmd==="session_api_receipt_review"?new Promise(r=>{resolve=r;}):Promise.resolve({version:1,items:[row]}));
 confirmations();const button=screen.getByRole("button",{name:t("composer.queueRecoverDismiss")});button.focus();fireEvent.click(button);
 const back=screen.getByRole("button",{name:t("receipt.back")});back.focus();
 await act(async()=>resolve(ack("dismiss")));await screen.findByText(t("composer.queueRecoverDismissed"));
 expect(document.activeElement).toBe(back);
});
it("restores the opener on Escape and ignores focus work from a late acknowledgement", async()=>{
 let resolve!:(v:unknown)=>void;const t=createT("en");render(<SessionReceiptPanel t={t}/>);
 const opener=screen.getByRole("button",{name:t("receipt.title")});opener.focus();fireEvent.click(opener);
 const item=await screen.findByRole("button",{name:/old/});fireEvent.click(item);
 host.invoke.mockImplementation((cmd:string)=>cmd==="session_api_receipt_review"?new Promise(r=>{resolve=r;}):Promise.resolve({version:1,items:[row]}));
 confirmations();const button=screen.getByRole("button",{name:t("composer.queueRecoverDismiss")});button.focus();fireEvent.click(button);
 fireEvent.keyDown(document,{key:"Escape"});await waitFor(()=>expect(document.activeElement).toBe(opener));
 await act(async()=>resolve(ack("dismiss")));expect(document.activeElement).toBe(opener);expect(screen.queryByRole("dialog")).toBeNull();
});
