/** @vitest-environment jsdom */
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { useEffect } from "react";
import { useImportContinuation } from "@/hooks/useImportContinuation";
import type { useSessionImportUi } from "@/hooks/useSessionImport";
import type { HandoffDraft } from "@/lib/api/sessionMigration";
import { defaultSelection } from "@/lib/sessionImport/selection";
import { SessionImportContinuationModal } from "./SessionImportContinuationModal";
const mocks=vi.hoisted(()=>({list:vi.fn(),scope:vi.fn(),create:vi.fn(),restore:vi.fn(),context:vi.fn()}));
vi.mock("@/lib/api/workspace",()=>({workspacesList:mocks.list}));
vi.mock("@/lib/api",()=>({sessionImportContinueSelection:mocks.scope,sessionImportContinueCreate:mocks.create,sessionImportContinueRestore:mocks.restore}));
vi.mock("@/lib/api/sessionMigration",async(importOriginal)=>({...await importOriginal<typeof import("@/lib/api/sessionMigration")>(),sessionImportEventContext:mocks.context}));
const draft:HandoffDraft={draftId:"d",draftHash:"h",importId:"imp:1",revisionId:"r1",leaf:"leaf",text:"secret old edit",coverage:"full",estimatedTokens:3,runtimeCapability:"local-full",selection:{...defaultSelection(),eventIds:["a","b"],pinnedEventIds:["b"],assetIds:["image"]}};
const noop=()=>{};
function Harness({initial=draft}:{initial?:HandoffDraft}){const state=useImportContinuation(null,noop);const {setDraft}=state;useEffect(()=>setDraft(initial),[initial,setDraft]);return <SessionImportContinuationModal ui={{...state,busy:state.continuationBusy,projects:[]} as unknown as ReturnType<typeof useSessionImportUi>} locale="en"/>;}
beforeEach(()=>{vi.resetAllMocks();mocks.list.mockResolvedValue([]);mocks.scope.mockImplementation(async(_id,_hash,selection)=>({...draft,draftId:"new",draftHash:"new-hash",text:"safe regenerated context",selection}));mocks.create.mockResolvedValue({appSessionId:"native"});mocks.restore.mockRejectedValue(new Error("offline"));});
afterEach(cleanup);
async function review(initial=draft){render(<Harness initial={initial}/>);fireEvent.click(await screen.findByRole("button",{name:"Review history permissions"}));return screen.getByRole("region",{name:"Review history permissions"});}
test("review narrows events, pins, assets and fields then regenerates the handoff before creation",async()=>{
 const region=await review();const create=screen.getByRole("button",{name:"Create continuation"});expect((create as HTMLButtonElement).disabled).toBe(true);
 fireEvent.click(within(screen.getByRole("region",{name:"Events"})).getByRole("checkbox",{name:"b"}));
 expect((within(screen.getByRole("region",{name:"Pinned events"})).getByRole("checkbox",{name:"b"}) as HTMLInputElement).checked).toBe(false);
 fireEvent.click(within(screen.getByRole("region",{name:"Attachments"})).getByRole("checkbox",{name:"image"}));
 fireEvent.click(within(region).getByRole("checkbox",{name:"Tool inputs"}));
 expect((within(region).getByRole("checkbox",{name:"Include reasoning"}) as HTMLInputElement).disabled).toBe(true);
 fireEvent.click(screen.getByRole("button",{name:"Apply and regenerate handoff"}));
 await waitFor(()=>expect(screen.queryByRole("region",{name:"Review history permissions"})).toBeNull());
 expect(mocks.scope).toHaveBeenCalledWith("d","h",{...draft.selection,eventIds:["a"],pinnedEventIds:[],assetIds:[],fields:{...draft.selection!.fields,toolCallInput:false}});
 expect((screen.getByRole("textbox",{name:/Edit the first message/}) as HTMLTextAreaElement).value).toBe("safe regenerated context");
 fireEvent.click(create);await waitFor(()=>expect(mocks.create).toHaveBeenCalledWith("new","new-hash","continuation:new","safe regenerated context"));
});
test("scope rows are bounded and selected state survives page/filter changes",async()=>{
 await review({...draft,selection:{...defaultSelection(),eventIds:Array.from({length:85},(_,i)=>`event-${i}`)}});
 const events=screen.getByRole("region",{name:"Events"});expect(within(events).getAllByRole("checkbox")).toHaveLength(40);
 fireEvent.click(within(events).getByRole("checkbox",{name:"event-1"}));fireEvent.click(within(events).getByRole("button",{name:"Load more"}));expect(within(events).getAllByRole("checkbox")).toHaveLength(40);
 fireEvent.change(within(events).getByRole("textbox"),{target:{value:"event-1"}});expect((within(events).getByRole("checkbox",{name:"event-1"}) as HTMLInputElement).checked).toBe(false);
 fireEvent.change(within(events).getByRole("textbox"),{target:{value:"none"}});expect(within(events).queryAllByRole("checkbox")).toHaveLength(0);
});
test("event preview uses the exact immutable snapshot and never displays a conflicting response",async()=>{
 mocks.context.mockResolvedValueOnce({revision:"wrong",leaf:"leaf",events:[{kind:"message",id:"a",text:"FOREIGN_SENTINEL"}]}).mockResolvedValueOnce({revision:"r1",leaf:"leaf",events:[{kind:"message",id:"a",text:"selected preview"}]});
 await review();const preview=within(screen.getByRole("region",{name:"Events"})).getAllByRole("button",{name:"Preview"})[0];fireEvent.click(preview);await screen.findByRole("alert");expect(screen.queryByText("FOREIGN_SENTINEL")).toBeNull();
 fireEvent.click(preview);await screen.findByText("selected preview");expect(mocks.context).toHaveBeenLastCalledWith("imp:1","a","r1","leaf",{before:0,after:0});
});
test("a lost response freezes controls but permits the same apply retry without creating",async()=>{
 mocks.scope.mockRejectedValueOnce(new Error("lost response"));await review();fireEvent.click(screen.getByRole("button",{name:"Apply and regenerate handoff"}));await screen.findByRole("alert");
 expect((screen.getByRole("button",{name:"Keep current permissions"}) as HTMLButtonElement).disabled).toBe(true);expect((screen.getByRole("button",{name:"Create continuation"}) as HTMLButtonElement).disabled).toBe(true);expect((screen.getByRole("checkbox",{name:"Message text"}) as HTMLInputElement).disabled).toBe(true);
 fireEvent.click(screen.getByRole("button",{name:"Apply and regenerate handoff"}));await waitFor(()=>expect(mocks.scope).toHaveBeenCalledTimes(2));expect(mocks.scope.mock.calls[0]).toEqual(mocks.scope.mock.calls[1]);expect(mocks.create).not.toHaveBeenCalled();
});
test("keyboard apply returns focus to the review trigger without submitting continuation",async()=>{
 const user=userEvent.setup();await review();const apply=screen.getByRole("button",{name:"Apply and regenerate handoff"});apply.focus();await user.keyboard("{Enter}");await waitFor(()=>expect(document.activeElement).toBe(screen.getByRole("button",{name:"Review history permissions"})));expect(mocks.create).not.toHaveBeenCalled();
});

test("keyboard review starts at its heading and cancel restores its trigger", async () => {
  const user = userEvent.setup();
  await review();
  expect(document.activeElement).toBe(screen.getByRole("heading", {name: "Review history permissions"}));
  await user.tab();
  expect(document.activeElement).toBe(screen.getByRole("checkbox", {name: "Message text"}));
  await user.click(screen.getByRole("button", {name: "Keep current permissions"}));
  expect(document.activeElement).toBe(screen.getByRole("button", {name: "Review history permissions"}));
  expect(mocks.scope).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
});
