import { useCallback, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import type { Locale, MessageKey, Vars } from "@/i18n";
import * as api from "@/lib/api";
import { parseContextInfoPayload } from "@/lib/contextInfoSnapshot";
import {
  contextWindowSlashFeedback,
  formatWindowLabel,
  resolveContextWindowCommand,
  selectableContextWindows,
} from "@/lib/contextWindowCommand";
import {
  formatTokenCount,
  resolveContextUsageDisplay,
  type ContextUsageMessage,
  type ContextUsageState,
} from "@/lib/contextUsage";
import { resolveContextWindow, type ModelOption } from "@/lib/grokCatalog";
import { withModelContextWindow } from "@/lib/providerModelConfig";

/** Composer context commands and display, shared by the menu and slash entry. */
export function useComposerContext(opts: {
  sessionId: string | null;
  modelId: string;
  activeCustomProvider: api.CustomProvider | null;
  availableModels: ModelOption[];
  contextUsage: ContextUsageState;
  setContextUsage: Dispatch<SetStateAction<ContextUsageState>>;
  messages: ContextUsageMessage[];
  locale: Locale;
  refreshProviderRoute: () => Promise<void>;
  showToast: (message: string, ms?: number) => void;
  tr: (key: MessageKey, vars?: Vars) => string;
}) {
  const {
    sessionId, modelId, activeCustomProvider, availableModels, contextUsage,
    setContextUsage, messages, locale, refreshProviderRoute, showToast, tr,
  } = opts;
  const viewedSession = useRef(sessionId);
  viewedSession.current = sessionId;
  const [draftContextWindow, setDraftContextWindow] = useState<number | null>(null);
  const currentModelWindow = useMemo(
    () => resolveContextWindow({
      activeCustomProvider,
      modelId,
      models: availableModels,
      agentContextWindow:
        contextUsage.agentContextWindow ?? (sessionId ? null : draftContextWindow),
    }),
    [activeCustomProvider, modelId, availableModels, contextUsage.agentContextWindow, sessionId, draftContextWindow],
  );
  const contextUsageDisplay = useMemo(() => ({
    ...resolveContextUsageDisplay(contextUsage, messages, locale, currentModelWindow),
    contextInfo: contextUsage.contextInfo ?? null,
    contextInfoNonce: contextUsage.contextInfoNonce ?? 0,
  }), [contextUsage, messages, locale, currentModelWindow]);

  const handleContextWindow = useCallback(async (tokens: number) => {
    if (!api.isTauri()) return false;
    if (activeCustomProvider) {
      try {
        await api.providersUpsert({
          id: activeCustomProvider.id,
          model: activeCustomProvider.model,
          baseUrl: activeCustomProvider.baseUrl,
          name: activeCustomProvider.name,
          apiBackend: activeCustomProvider.apiBackend,
          models: withModelContextWindow(
            activeCustomProvider.models, activeCustomProvider.model, tokens,
          ),
          efforts: activeCustomProvider.efforts,
          setAsDefault: false,
          contextWindow: tokens,
        });
        await refreshProviderRoute();
        return true;
      } catch (e) {
        showToast(String(e), 4000);
        return false;
      }
    }
    const options = selectableContextWindows(availableModels.find((m) => m.id === modelId));
    if (!options.includes(tokens)) {
      showToast(tr("slash.contextWindowUnknown", {
        token: String(tokens),
        options: options.map(formatWindowLabel).join(", "),
      }), 4000);
      return false;
    }
    try {
      await api.sessionSetContextWindow(tokens, { sessionId });
      // The RPC still belongs to the captured chat after the view changes.
      if (viewedSession.current === sessionId) {
        if (!sessionId) setDraftContextWindow(tokens);
        setContextUsage((prev) => ({ ...prev, agentContextWindow: tokens }));
      }
      return true;
    } catch (e) {
      showToast(String(e), 4000);
      return false;
    }
  }, [activeCustomProvider, availableModels, modelId, refreshProviderRoute, sessionId, setContextUsage, showToast, tr]);

  const runContextWindowSlash = useCallback(async (args: string) => {
    const result = resolveContextWindowCommand({
      args,
      hasModel: Boolean(modelId),
      hasSession: Boolean(sessionId),
      options: activeCustomProvider ? [] : selectableContextWindows(availableModels.find((m) => m.id === modelId)),
      current: currentModelWindow,
    });
    if (result.kind === "switch") {
      if (await handleContextWindow(result.window)) {
        showToast(tr("slash.contextWindowSwitched", {
          size: formatTokenCount(result.window, locale),
        }), 2500);
      }
      return;
    }
    const feedback = contextWindowSlashFeedback(result);
    showToast(tr(feedback.key, feedback.vars), 4000);
  }, [activeCustomProvider, availableModels, currentModelWindow, handleContextWindow, locale, modelId, sessionId, showToast, tr]);

  const runContextSlash = useCallback(async (announce: boolean) => {
    if (!api.isTauri() || !sessionId) {
      if (announce) showToast(tr("slash.contextNoSession"), 4000);
      return;
    }
    try {
      const raw = await api.sessionContextInfo(sessionId);
      if (viewedSession.current !== sessionId) return;
      const view = parseContextInfoPayload(raw);
      if (!view) {
        if (announce) showToast(tr("slash.contextUnavailable"), 4000);
        return;
      }
      setContextUsage((prev) => ({
        ...prev,
        knownTokens: view.used,
        agentContextWindow: view.total > 0 ? view.total : prev.agentContextWindow,
        agentPercentage: null,
        lastCompactMessageId: null,
        contextInfo: view,
        contextInfoNonce: (prev.contextInfoNonce ?? 0) + 1,
      }));
    } catch (e) {
      if (announce && viewedSession.current === sessionId) showToast(String(e), 4000);
    }
  }, [sessionId, setContextUsage, showToast, tr]);

  return { currentModelWindow, contextUsageDisplay, handleContextWindow, runContextWindowSlash, runContextSlash };
}
