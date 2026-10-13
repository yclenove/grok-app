/** @vitest-environment jsdom */
import { act, cleanup, renderHook } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createT } from "@/i18n";
import * as api from "@/lib/api";
import { INITIAL_CONTEXT_USAGE } from "@/lib/contextUsage";
import { useComposerContext } from "./useComposerContext";

vi.mock("@/lib/api", () => ({
  isTauri: vi.fn(() => true),
  sessionSetContextWindow: vi.fn(async () => {}),
  sessionContextInfo: vi.fn(),
  providersUpsert: vi.fn(async () => {}),
}));

const showToast = vi.fn();
const refreshProviderRoute = vi.fn(async () => {});
const tr = createT("en");
const model = { id: "grok-test", label: "Grok", contextWindow: 256000, contextWindows: [256000, 500000] };
const provider: api.CustomProvider = {
  id: "relay", name: "Relay", model: "relay-model", baseUrl: "https://example.invalid",
  hasApiKey: false, apiBackend: "openai", providerMode: "generic", isDefault: false,
  models: [{ id: "relay-model", name: "Relay model" }],
};

function useTestContext(props: { sessionId: string | null; provider?: api.CustomProvider | null }) {
  const [contextUsage, setContextUsage] = useState(INITIAL_CONTEXT_USAGE);
  return {
    ...useComposerContext({
      sessionId: props.sessionId, modelId: model.id,
      activeCustomProvider: props.provider ?? null, availableModels: [model],
      contextUsage, setContextUsage, messages: [], locale: "en",
      refreshProviderRoute, showToast, tr,
    }),
    contextUsage,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.isTauri).mockReturnValue(true);
  vi.mocked(api.sessionSetContextWindow).mockResolvedValue(undefined);
});
afterEach(cleanup);

describe("composer context integration", () => {
  it("switches the live official window through the slash entry", async () => {
    const { result } = renderHook(() => useTestContext({ sessionId: "chat-a" }));
    await act(async () => { await result.current.runContextWindowSlash("500k"); });
    expect(api.sessionSetContextWindow).toHaveBeenCalledWith(500000, { sessionId: "chat-a" });
    expect(result.current.currentModelWindow).toBe(500000);
    expect(showToast).toHaveBeenCalledWith(tr("slash.contextWindowSwitched", { size: "500K" }), 2500);
  });

  it("stages an official choice before session creation", async () => {
    const { result } = renderHook(() => useTestContext({ sessionId: null }));
    await act(async () => { expect(await result.current.handleContextWindow(500000)).toBe(true); });
    expect(api.sessionSetContextWindow).toHaveBeenCalledWith(500000, { sessionId: null });
    expect(result.current.currentModelWindow).toBe(500000);
  });

  it("keeps the displayed window on an unsupported size or failed RPC", async () => {
    const { result } = renderHook(() => useTestContext({ sessionId: "chat-a" }));
    await act(async () => { expect(await result.current.handleContextWindow(123)).toBe(false); });
    expect(api.sessionSetContextWindow).not.toHaveBeenCalled();
    vi.mocked(api.sessionSetContextWindow).mockRejectedValueOnce(new Error("offline"));
    await act(async () => { expect(await result.current.handleContextWindow(500000)).toBe(false); });
    expect(result.current.contextUsage.agentContextWindow).toBeNull();
    expect(showToast).toHaveBeenLastCalledWith("Error: offline", 4000);
  });

  it("saves custom channel windows without sending an official ACP retune", async () => {
    const { result } = renderHook(() => useTestContext({ sessionId: "chat-a", provider }));
    await act(async () => { expect(await result.current.handleContextWindow(64000)).toBe(true); });
    expect(api.providersUpsert).toHaveBeenCalledWith(expect.objectContaining({
      id: "relay", contextWindow: 64000, setAsDefault: false,
      models: [expect.objectContaining({ id: "relay-model", contextWindow: 64000 })],
    }));
    expect(refreshProviderRoute).toHaveBeenCalledOnce();
    expect(api.sessionSetContextWindow).not.toHaveBeenCalled();
  });

  it("parses context info and opens the usage snapshot", async () => {
    vi.mocked(api.sessionContextInfo).mockResolvedValue({ context: { used: 12000, total: 500000 } });
    const { result } = renderHook(() => useTestContext({ sessionId: "chat-a" }));
    await act(async () => { await result.current.runContextSlash(true); });
    expect(api.sessionContextInfo).toHaveBeenCalledWith("chat-a");
    expect(result.current.contextUsage.knownTokens).toBe(12000);
    expect(result.current.contextUsageDisplay.contextInfo?.total).toBe(500000);
    expect(result.current.contextUsageDisplay.contextInfoNonce).toBe(1);
  });

  it("ignores context info that arrives after switching chats", async () => {
    let complete!: (value: unknown) => void;
    vi.mocked(api.sessionContextInfo).mockReturnValue(new Promise((resolve) => { complete = resolve; }));
    const { result, rerender } = renderHook(useTestContext, { initialProps: { sessionId: "chat-a" } });
    let request!: Promise<void>;
    act(() => { request = result.current.runContextSlash(true); });
    rerender({ sessionId: "chat-b" });
    await act(async () => {
      complete({ context: { used: 12000, total: 500000 } });
      await request;
    });
    expect(result.current.contextUsage.knownTokens).toBeNull();
    expect(result.current.contextUsageDisplay.contextInfoNonce).toBe(0);
  });

  it("does not apply a late window-switch reply to the newly viewed chat", async () => {
    let complete!: () => void;
    vi.mocked(api.sessionSetContextWindow).mockReturnValueOnce(new Promise((resolve) => { complete = resolve; }));
    const { result, rerender } = renderHook(useTestContext, { initialProps: { sessionId: "chat-a" } });
    let request!: Promise<boolean>;
    act(() => { request = result.current.handleContextWindow(500000); });
    rerender({ sessionId: "chat-b" });
    await act(async () => { complete(); expect(await request).toBe(true); });
    expect(result.current.contextUsage.agentContextWindow).toBeNull();
  });

  it("keeps background refresh quiet when no context can be loaded", async () => {
    const { result } = renderHook(() => useTestContext({ sessionId: null }));
    await act(async () => { await result.current.runContextSlash(false); });
    expect(showToast).not.toHaveBeenCalled();
    expect(api.sessionContextInfo).not.toHaveBeenCalled();
    await act(async () => { await result.current.runContextSlash(true); });
    expect(showToast).toHaveBeenCalledWith(tr("slash.contextNoSession"), 4000);
  });
});
