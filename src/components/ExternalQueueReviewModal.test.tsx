/** @vitest-environment jsdom */
import { afterEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createT, type Locale } from "@/i18n";
import { ExternalQueueReviewModal } from "./ExternalQueueReviewModal";
import type { ExternalQueueReview } from "@/hooks/useExternalQueueReview";
import { SendQueueItemActions } from "./SendQueueItemActions";
afterEach(cleanup);
const locales: Locale[] = ["en","de","es","fil","fr","id","it","ja","ko","pt-BR","ru","ta","uk","zh","zh-TW"];
function review(): ExternalQueueReview { return { target:{sessionId:"s",itemId:"q",prompt:"full prompt\nsecond line",createdAt:1,state:"queued"},currentState:"queued",canCancel:true,canRecover:false,reviewed:false,setReviewed:vi.fn(),riskConfirmed:false,setRiskConfirmed:vi.fn(),operation:null,resolution:null,recover:vi.fn(),confirmed:false,setConfirmed:vi.fn(),busy:false,done:false,error:null,open:vi.fn(),close:vi.fn(),cancel:vi.fn().mockResolvedValue(undefined),refresh:vi.fn(),verified:true,refreshing:false }; }
it.each(locales)("renders complete review/confirm/close controls for %s",locale=>{
  const tr=createT(locale),r=review();const v=render(<ExternalQueueReviewModal review={r} tr={tr}/>);
  expect(screen.getByRole("dialog").textContent).toContain(tr("composer.queueCancelLimit"));
  expect(screen.getByText("full prompt second line",{exact:false})).toBeTruthy();
  const cancel=screen.getByRole("button",{name:tr("composer.queueCancelAction")}) as HTMLButtonElement;
  expect(cancel.disabled).toBe(true);fireEvent.click(screen.getByRole("checkbox"));expect(r.setConfirmed).toHaveBeenCalledWith(true);
  v.rerender(<ExternalQueueReviewModal review={{...r,confirmed:true}} tr={tr}/>);
  fireEvent.click(screen.getByRole("button",{name:tr("composer.queueCancelAction")}));expect(r.cancel).toHaveBeenCalledTimes(1);
  fireEvent.keyDown(document,{key:"Escape"});expect(r.close).toHaveBeenCalled();
});
it.each(["pending","held"] as const)("requires reviewed-risk confirmations for %s; never offers cancellation",state=>{
  const tr=createT("en"),r={...review(),currentState:state,canCancel:false};render(<ExternalQueueReviewModal review={r} tr={tr}/>);
  expect(screen.getAllByRole("checkbox")).toHaveLength(2);
  for(const input of screen.getAllByRole("checkbox"))expect((input as HTMLInputElement).disabled).toBe(true);
  expect((screen.getByRole("button",{name:tr("composer.queueRecoverRetry")}) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button",{name:tr("composer.queueCancelAction")})).toBeNull();
  fireEvent.click(screen.getByRole("button",{name:tr("composer.queueSyncRefresh")}));expect(r.refresh).toHaveBeenCalledTimes(1);
});
it("Host row review never invokes composer ownership mutations",()=>{
  const tr=createT("en");const item={id:"q",storedDisplay:"prompt",createdAt:1,source:"external" as const,externalState:"held" as const,attachments:[],goalMode:false};
  const p={queue:[item],index:0,busy:true,guidingItemId:null,canGuide:false,tr,onMove:vi.fn(),onEdit:vi.fn(),onGuide:vi.fn(),onRemove:vi.fn(),onReview:vi.fn()};
  render(<SendQueueItemActions {...p}/>);fireEvent.click(screen.getByRole("button",{name:tr("composer.queueReviewAction")}));
  expect(p.onReview).toHaveBeenCalledWith(item);for(const f of [p.onMove,p.onEdit,p.onGuide,p.onRemove])expect(f).not.toHaveBeenCalled();
});
it("acknowledgement survives stale snapshot and no longer offers cancellation",()=>{
  const tr=createT("en");render(<ExternalQueueReviewModal review={{...review(),done:true,verified:false}} tr={tr}/>);
  expect(screen.getByRole("status").textContent).toBe(tr("composer.queueCancelled"));
  expect(screen.queryByRole("checkbox")).toBeNull();expect(screen.queryByRole("button",{name:tr("composer.queueCancelAction")})).toBeNull();
});

it.each(["dismiss","retry"] as const)("offers %s only after both independent confirmations and shows truthful receipt",decision=>{
 const tr=createT("en"),r={...review(),currentState:"held" as const,canCancel:false,canRecover:true};
 const v=render(<ExternalQueueReviewModal review={r} tr={tr}/>);
 const action=()=>screen.getByRole("button",{name:tr(decision==="retry"?"composer.queueRecoverRetry":"composer.queueRecoverDismiss")}) as HTMLButtonElement;
 expect(screen.getByText(tr("composer.queueRecoverExplain"))).toBeTruthy();
 expect(screen.getByText(tr("composer.queueRecoverRisk"))).toBeTruthy();
 expect(action().disabled).toBe(true);
 fireEvent.click(screen.getAllByRole("checkbox")[0]);expect(r.setReviewed).toHaveBeenCalledWith(true);
 fireEvent.click(screen.getAllByRole("checkbox")[1]);expect(r.setRiskConfirmed).toHaveBeenCalledWith(true);
 v.rerender(<ExternalQueueReviewModal review={{...r,reviewed:true}} tr={tr}/>);expect(action().disabled).toBe(true);
 v.rerender(<ExternalQueueReviewModal review={{...r,riskConfirmed:true}} tr={tr}/>);expect(action().disabled).toBe(true);
 v.rerender(<ExternalQueueReviewModal review={{...r,reviewed:true,riskConfirmed:true}} tr={tr}/>);expect(action().disabled).toBe(false);
 fireEvent.click(action());expect(r.recover).toHaveBeenCalledExactlyOnceWith(decision);expect(r.cancel).not.toHaveBeenCalled();
 const resolution={version:1 as const,decision,sessionId:"s",itemId:"q",replacementId:decision==="retry"?"q-ext-new":null};
 v.rerender(<ExternalQueueReviewModal review={{...r,done:true,resolution,operation:decision}} tr={tr}/>);
 expect(screen.queryByRole("checkbox")).toBeNull();expect(screen.queryByRole("button",{name:tr("composer.queueRecoverRetry")})).toBeNull();
 expect(screen.getByRole("status").textContent).toBe(tr(decision==="retry"?"composer.queueRecoverRetried":"composer.queueRecoverDismissed"));
 expect(screen.queryByText("q-ext-new")!==null).toBe(decision==="retry");
});
it("keeps the busy review dismissible and the active-dispatch refusal non-successful",()=>{
 const tr=createT("en"),r={...review(),currentState:"pending" as const,canCancel:false,canRecover:false,busy:true,operation:"retry" as const};
 const v=render(<ExternalQueueReviewModal review={r} tr={tr}/>);
 expect(screen.getByRole("status").textContent).toBe(tr("composer.queueRecovering"));
 for(const cb of screen.getAllByRole("checkbox"))expect((cb as HTMLInputElement).disabled).toBe(true);
 fireEvent.keyDown(document,{key:"Escape"});expect(r.close).toHaveBeenCalled();
 v.rerender(<ExternalQueueReviewModal review={{...r,busy:false,error:"active"}} tr={tr}/>);
 expect(screen.getByRole("alert").textContent).toBe(tr("composer.queueRecoverActive"));
 expect(screen.queryByText(tr("composer.queueRecoverRetried"))).toBeNull();
});
