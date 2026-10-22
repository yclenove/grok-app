/** @vitest-environment jsdom */
import { adoptionUiFixture } from "@/test/sessionImportAdoptionUiFixture";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test } from "vitest";
import { groupDeferred, groupMetadata } from "@/test/sessionImportGroupFixture";
import { openSessionImport } from "@/lib/sessionImport/open";
import { useSessionImportUi } from "./useSessionImport";

let fixture: ReturnType<typeof adoptionUiFixture>;
beforeEach(() => { fixture = adoptionUiFixture(); });
afterEach(cleanup);
async function activeBatch() {
  fixture.batch.selectionStateVersion = 2;
  fixture.batch.state = "preparing";
  fixture.batch.active = 1;
  fixture.batch.prepared = 0;
  const hook = renderHook(() => useSessionImportUi());
  await act(async () => { openSessionImport({}); });
  await act(async () => { await hook.result.current.scan(); });
  await act(async () => { await hook.result.current.selectThese([groupMetadata("a").groupId]); });
  await act(async () => { await hook.result.current.previewSelected([]); });
  expect(hook.result.current.step).toBe("prepare");
  return hook;
}

test("failed cancellation preserves the preparation lock and does not reinitialize selection", async () => {
  const { result } = await activeBatch();
  const invoke = fixture.ipc.invoke.getMockImplementation()!;
  fixture.ipc.invoke.mockImplementation((command, args) => command === "session_import_prepare_cancel"
    ? Promise.reject(new Error("SOURCE_BUSY: synthetic cancel failure")) : invoke(command, args));
  const reads = fixture.host.client.get.mock.calls.length;
  await act(async () => { expect(await result.current.abandonPrepare("content")).toBe(false); });
  expect(result.current.prepareBatch?.batchId).toBe(fixture.batch.batchId);
  expect(result.current.step).toBe("prepare");
  expect(result.current.error).toContain("synthetic cancel failure");
  expect(result.current.busy).toBe(false);
  await act(async () => { expect(await result.current.refreshScan()).toBe(false); });
  expect(fixture.host.client.get).toHaveBeenCalledTimes(reads);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(1);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
});

test.each(["close", "source", "unmount"] as const)("%s during cancellation prevents obsolete Back from adopting groups or new intent", async (boundary) => {
  const hook = await activeBatch();
  const pending = groupDeferred<typeof fixture.batch>();
  const invoke = fixture.ipc.invoke.getMockImplementation()!;
  fixture.ipc.invoke.mockImplementation((command, args) => command === "session_import_prepare_cancel"
    ? pending.promise : invoke(command, args));
  const reads = fixture.host.client.get.mock.calls.length;
  let leaving!: Promise<boolean>;
  act(() => { leaving = hook.result.current.abandonPrepare("content"); });
  await waitFor(() => expect(fixture.calls("session_import_prepare_cancel")).toHaveLength(1));
  act(() => {
    if (boundary === "unmount") hook.unmount();
    else {
      hook.result.current.setOpen(false);
      if (boundary === "source") hook.result.current.chooseListedRoot("codex", "/synthetic/next-source");
    }
  });
  await act(async () => { pending.resolve({ ...fixture.batch, state: "cancelled", active: 0 }); await leaving; });
  expect(await leaving).toBe(false);
  expect(fixture.host.client.get).toHaveBeenCalledTimes(reads);
  expect(fixture.calls("session_import_prepare_start")).toHaveLength(1);
  expect(fixture.calls("session_import_commit")).toHaveLength(0);
  if (boundary === "source") {
    await act(async () => { openSessionImport({}); });
    expect(hook.result.current.choice?.path).toBe("/synthetic/next-source");
    expect(hook.result.current.scanState).toBeNull();
    expect(hook.result.current.step).toBe("source");
    expect(fixture.host.client.get).toHaveBeenCalledTimes(reads);
  }
});
