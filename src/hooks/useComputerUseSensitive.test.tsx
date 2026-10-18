/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { ConfirmCard, CropView } from "@/lib/computer-use/confirmState";
import { sharedTimeline } from "@/lib/computer-use/store";
import { useComputerUseConfirm } from "./useComputerUseSensitive";

const host = vi.hoisted(() => ({
  pending: null as ((card: ConfirmCard) => void) | null,
  settled: null as ((requestId: number, end: "allowed") => void) | null,
  closed: null as ((sessionId: string) => void) | null,
  turn: null as ((sessionId: string) => void) | null,
}));

vi.mock("@/lib/api/computerUse", () => ({
  listenComputerUseConfirm: async (handler: (card: ConfirmCard) => void) => {
    host.pending = handler;
    return () => {};
  },
  listenComputerUseConfirmSettled: async (handler: (requestId: number, end: "allowed") => void) => {
    host.settled = handler;
    return () => {};
  },
}));

vi.mock("@/lib/api/computerUseShot", () => ({
  listenComputerUseSessionClosed: async (handler: (sessionId: string) => void) => {
    host.closed = handler;
    return () => {};
  },
  listenComputerUseTurnEnded: async (handler: (sessionId: string) => void) => {
    host.turn = handler;
    return () => {};
  },
}));

const crop: CropView = {
  kind: "redacted",
  mime: "image/png",
  width: 2,
  height: 2,
  bytes: [1, 2, 3, 4],
};

function card(sessionId: string, requestId: number): ConfirmCard {
  return {
    requestId,
    sessionId,
    enginePid: 1,
    engineStarted: 2,
    policyVersion: 3,
    image: {
      platform: "linux",
      key: "linux:app",
      teamId: null,
      publisher: null,
      sha256Hex: null,
      signature: "unknown",
      imagePath: "/bin/app",
      pid: 4,
    },
    name: "App",
    summary: "sum",
    reason: "why",
    snapshotId: "snap",
    actionDigest: "digest",
    crop,
    timeoutSecs: 120,
    openedMs: 5,
    hasIcon: false,
  };
}

afterEach(() => {
  cleanup();
  if (sharedTimeline.view("").stopped) sharedTimeline.userResume();
  host.pending = null;
  host.settled = null;
  host.closed = null;
  host.turn = null;
});

async function mounted() {
  const hook = renderHook(() => useComputerUseConfirm());
  await waitFor(() => expect(host.pending).toBeTypeOf("function"));
  await waitFor(() => expect(host.closed).toBeTypeOf("function"));
  await waitFor(() => expect(host.turn).toBeTypeOf("function"));
  return hook;
}

it("drops a session crop on session end and turn end and keeps the other session", async () => {
  const { result } = await mounted();
  act(() => {
    host.pending?.(card("alpha", 1));
    host.pending?.(card("beta", 2));
  });
  expect(result.current.cards.map((item) => item.crop)).toEqual([crop, crop]);
  act(() => host.closed?.("alpha"));
  expect(result.current.cards.map((item) => item.sessionId)).toEqual(["beta"]);
  expect(result.current.phases[1]).toBeUndefined();
  act(() => host.turn?.("beta"));
  expect(result.current.cards).toEqual([]);
});

it("leaves crops in place when a card only settles", async () => {
  const { result } = await mounted();
  act(() => host.pending?.(card("alpha", 1)));
  act(() => host.settled?.(1, "allowed"));
  expect(result.current.cards).toHaveLength(1);
  expect(result.current.cards[0]?.crop).toEqual(crop);
  expect(result.current.phases[1]).toBe("allowed");
});

it("drops every crop on Stop and ignores a new card until resume", async () => {
  const { result } = await mounted();
  act(() => host.pending?.(card("alpha", 1)));
  act(() => sharedTimeline.userStop());
  expect(result.current.cards).toEqual([]);
  act(() => host.pending?.(card("alpha", 2)));
  expect(result.current.cards).toEqual([]);
  act(() => sharedTimeline.userResume());
  act(() => host.pending?.(card("alpha", 3)));
  expect(result.current.cards.map((item) => item.requestId)).toEqual([3]);
});
