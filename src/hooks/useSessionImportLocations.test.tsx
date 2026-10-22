/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { SourceLocation, SourceLocationPage, SourceRegistration } from "@/lib/api/sessionMigrationLocations";
import { groupDeferred } from "@/test/sessionImportGroupFixture";
import { useSessionImportLocations } from "./useSessionImportLocations";

const source: SourceRegistration = { sourceId: "synthetic-installation", provider: "cursor", rootPath: "/synthetic/primary", profile: "default" };
const primary: SourceLocation = { version: 1, sourceId: source.sourceId, provider: "cursor", host: "local",
  locationId: `primary:${source.sourceId}`, rootPath: source.rootPath, kind: "primary" };
const associated: SourceLocation = { ...primary, locationId: "00000000-0000-0000-0000-000000000001", kind: "associated", rootPath: "/synthetic/other" };
const page = (locations = [primary], nextCursor: string | null = null): SourceLocationPage => ({ locations, nextCursor, totalCount: 51 });
const client = { list: vi.fn(), associate: vi.fn(), pick: vi.fn() };
beforeEach(() => {
  client.list.mockReset().mockResolvedValue(page());
  client.associate.mockReset().mockResolvedValue(associated);
  client.pick.mockReset().mockResolvedValue(associated.rootPath);
});
afterEach(cleanup);

test("lazy open owns exactly one 50-metadata page and never registers or relocates", async () => {
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  expect(client.list).not.toHaveBeenCalled();
  await act(async () => { await result.current.start(); });
  expect(client.list).toHaveBeenCalledExactlyOnceWith(source.sourceId, "cursor", { host: "local", limit: 50 });
  expect(client.associate).not.toHaveBeenCalled();
  expect(result.current.selected).toEqual([]);
});

test("selection is explicit, cloned and taken only once; off-page choices remain bounded", async () => {
  client.list.mockResolvedValueOnce(page([primary], "signed-next")).mockResolvedValueOnce(page([associated]));
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  await act(async () => { await result.current.start(); });
  act(() => { result.current.toggle(primary); });
  await act(async () => { await result.current.next(); });
  act(() => { result.current.toggle(associated); });
  let taken: ReturnType<typeof result.current.takeSelection>;
  act(() => { taken = result.current.takeSelection(); expect(result.current.takeSelection()).toBeNull(); });
  expect(taken!.locations).toEqual([primary, associated]);
  expect(taken!.locations[0]).not.toBe(primary);
  expect(result.current.open).toBe(false);
});

test.each(["success", "error"])("close/reopen expires an old page %s without touching the newer owner", async (outcome) => {
  const pending = groupDeferred<SourceLocationPage>();
  client.list.mockReturnValueOnce(pending.promise).mockResolvedValueOnce(page([associated]));
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  let old!: Promise<boolean>;
  act(() => { old = result.current.start(); result.current.close(); });
  await act(async () => { await result.current.start(); });
  await act(async () => { if (outcome === "success") pending.resolve(page()); else pending.reject(new Error("old failure")); await old; });
  expect(result.current.page?.locations).toEqual([associated]);
  expect(result.current.error).toBeNull();
  expect(result.current.busy).toBe(false);
});

test("failed next retains its accepted page and retries the same signed cursor", async () => {
  client.list.mockResolvedValueOnce(page([primary], "signed-next")).mockRejectedValueOnce(new Error("SOURCE_BUSY: synthetic"))
    .mockResolvedValueOnce(page([associated]));
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  await act(async () => { await result.current.start(); await result.current.next(); });
  expect(result.current.page?.locations).toEqual([primary]);
  await act(async () => { await result.current.retry(); });
  expect(client.list.mock.calls[2][2]).toEqual({ host: "local", limit: 50, cursor: "signed-next" });
  expect(result.current.page?.locations).toEqual([associated]);
});

test("50-row pages replace metadata rather than collecting every associated root", async () => {
  const locations = Array.from({ length: 50 }, (_, index) => ({ ...associated,
    locationId: `00000000-0000-0000-0000-${String(index + 2).padStart(12, "0")}` }));
  client.list.mockResolvedValueOnce(page(locations, "signed-next")).mockResolvedValueOnce(page([primary]));
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  await act(async () => { await result.current.start(); });
  expect(result.current.page?.locations).toHaveLength(50);
  await act(async () => { await result.current.next(); });
  expect(result.current.page?.locations).toEqual([primary]);
  expect(client.list.mock.calls[1][2]).toEqual({ host: "local", limit: 50, cursor: "signed-next" });
});

test.each(["foreign", "duplicate", "primary-drift", "over-budget"])("invalid %s metadata is not adopted", async (kind) => {
  const locations = kind === "foreign" ? [{ ...primary, sourceId: "another-installation" }]
    : kind === "duplicate" ? [primary, primary]
      : kind === "primary-drift" ? [{ ...primary, rootPath: "/synthetic/moved" }]
        : Array.from({ length: 51 }, () => primary);
  client.list.mockResolvedValueOnce(page(locations));
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  await act(async () => { expect(await result.current.start()).toBe(false); });
  expect(result.current.page).toBeNull();
  expect(result.current.error).toContain("SOURCE_CHANGED");
  expect(result.current.takeSelection()).toBeNull();
});

test("saving is explicit and same-turn commands cannot duplicate it or take a selection", async () => {
  const pending = groupDeferred<SourceLocation>();
  client.associate.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  await act(async () => { await result.current.start(); await result.current.pick(); });
  act(() => { result.current.toggle(primary); });
  expect(client.associate).not.toHaveBeenCalled();
  let saving!: Promise<boolean>;
  act(() => { saving = result.current.save(); void result.current.save(); expect(result.current.takeSelection()).toBeNull(); });
  expect(client.associate).toHaveBeenCalledExactlyOnceWith({ sourceId: source.sourceId, provider: "cursor", host: "local",
    path: associated.rootPath, expectedPrimaryRoot: source.rootPath });
  await act(async () => { pending.resolve(associated); await saving; });
  expect(result.current.saved).toEqual(associated);
  expect(result.current.attempt).toBeNull();
  expect(result.current.selected).toEqual([primary]);
});

test("closing a save expires its completion but reopening checks the exact idempotent request", async () => {
  const pending = groupDeferred<SourceLocation>();
  client.associate.mockReturnValueOnce(pending.promise);
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  await act(async () => { await result.current.start(); await result.current.pick(); });
  let saving!: Promise<boolean>;
  act(() => { saving = result.current.save(); result.current.close(); });
  await act(async () => { pending.resolve(associated); await saving; await result.current.start(); });
  expect(result.current.saved).toBeNull();
  expect(result.current.attempt?.path).toBe(associated.rootPath);
  await act(async () => { expect(await result.current.pick()).toBe(false); await result.current.save(); });
  expect(client.associate.mock.calls[1][0]).toEqual(client.associate.mock.calls[0][0]);
  expect(result.current.attempt).toBeNull();
});

test("save ACK survives its following read failure without pretending rollback", async () => {
  const { result } = renderHook(() => useSessionImportLocations(source, client));
  await act(async () => { await result.current.start(); await result.current.pick(); });
  client.list.mockRejectedValueOnce(new Error("SOURCE_BUSY: read after acknowledged save"));
  await act(async () => { expect(await result.current.save()).toBe(true); });
  expect(result.current.saved).toEqual(associated);
  expect(result.current.error).toContain("SOURCE_BUSY");
  expect(result.current.attempt).toBeNull();
  expect(result.current.takeSelection()).toBeNull();
});

test("a late picker cannot supply a folder to a changed installation or an unmounted dialog", async () => {
  const pending = groupDeferred<string | null>();
  client.pick.mockReturnValueOnce(pending.promise);
  const view = renderHook(({ reg }) => useSessionImportLocations(reg, client), { initialProps: { reg: source } });
  await act(async () => { await view.result.current.start(); });
  let picking!: Promise<boolean>;
  act(() => { picking = view.result.current.pick(); });
  view.rerender({ reg: { ...source, sourceId: "other-installation" } });
  await act(async () => { pending.resolve(associated.rootPath); await picking; });
  expect(view.result.current.open).toBe(false);
  expect(view.result.current.path).toBeNull();
  view.unmount();
  expect(client.associate).not.toHaveBeenCalled();
});

test("a changed wizard source intent expires a registered installation's pending metadata", async () => {
  const pending = groupDeferred<SourceLocationPage>();
  client.list.mockReturnValueOnce(pending.promise);
  const view = renderHook(({ scope }) => useSessionImportLocations(source, client, scope), { initialProps: { scope: "old-intent" } });
  let reading!: Promise<boolean>;
  act(() => { reading = view.result.current.start(); });
  view.rerender({ scope: "new-intent" });
  await act(async () => { pending.resolve(page()); await reading; });
  expect(view.result.current.open).toBe(false);
  expect(view.result.current.page).toBeNull();
});

test("unmount expires a pending page without reporting its old error", async () => {
  const pending = groupDeferred<SourceLocationPage>();
  client.list.mockReturnValueOnce(pending.promise);
  const view = renderHook(() => useSessionImportLocations(source, client));
  let reading!: Promise<boolean>;
  act(() => { reading = view.result.current.start(); });
  view.unmount();
  await act(async () => { pending.reject(new Error("old unmounted page failure")); expect(await reading).toBe(false); });
  expect(client.associate).not.toHaveBeenCalled();
});
