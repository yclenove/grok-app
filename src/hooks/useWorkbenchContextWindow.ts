import { useCallback, useEffect, useMemo, useRef, useState, type Dispatch, type SetStateAction } from "react";
import type { createT } from "@/i18n";
import * as api from "@/lib/api";
import type { SessionSnapshot } from "@/lib/session";
import { resolveContextWindow, type ModelOption } from "@/lib/grokCatalog";
import { formatTokenCount, resolveContextUsageDisplay, type ContextUsageState } from "@/lib/contextUsage";
import { withModelContextWindow } from "@/lib/providerModelConfig";
import { parseContextInfoPayload } from "@/lib/contextInfoSnapshot";
import { contextWindowSlashFeedback, positiveContextWindows, resolveContextWindowCommand, formatWindowLabel, selectableContextWindows } from "@/lib/contextWindowCommand";

type ContextInputs = {
  session: Pick<SessionSnapshot, "sessionId" | "state">;
  activeCustomProvider: api.CustomProvider | null;
  availableModels: ModelOption[];
  modelId: string;
  locale: string;
};

/** Keep upstream context state/effects in its domain, outside AppWorkbench. */
export function useWorkbenchContextDisplay({
  session, activeCustomProvider, availableModels, modelId, locale,
  contextUsage, messages, setAvailableModels,
}: ContextInputs & {
  contextUsage: ContextUsageState;
  messages: Parameters<typeof resolveContextUsageDisplay>[1];
  setAvailableModels: Dispatch<SetStateAction<ModelOption[]>>;
}) {
  /** Window picked on the new-chat composer, before a chat row exists. */
  const [draftContextWindow, setDraftContextWindow] = useState<number | null>(
    null,
  );
  /** Guards a one-time-per-session re-fetch of live model context windows. */
  const refreshedModelsSidRef = useRef<string | null>(null);
  const currentModelWindow = useMemo(
    () =>
      resolveContextWindow({
        activeCustomProvider,
        modelId,
        models: availableModels,
        // Prefer live agent occupancy denominator (Grok Build 1.0 → 500k).
        // A new chat has no agent yet; the draft choice is what the menu shows.
        agentContextWindow:
          contextUsage.agentContextWindow ??
          (session.sessionId ? null : draftContextWindow),
      }),
    [
      activeCustomProvider,
      modelId,
      availableModels,
      contextUsage.agentContextWindow,
      draftContextWindow,
      session.sessionId,
    ],
  );
  useEffect(() => {
    if (
      session.state === "ready" &&
      session.sessionId &&
      refreshedModelsSidRef.current !== session.sessionId
    ) {
      refreshedModelsSidRef.current = session.sessionId;
      api.modelsListAvailable()
        .then((res) => {
          if (!res?.models?.length) return;
          // Full merge so official live windows (500k) replace cold-start null
          // / stale catalog values — never leave a silent 200k official default.
          setAvailableModels((prev) => {
            const prevById = new Map(prev.map((m) => [m.id, m]));
            return res.models.map((m) => {
              const prior = prevById.get(m.id);
              return {
                id: m.id,
                label: m.label || m.id,
                source: m.source,
                isDefault: m.isDefault,
                reasoningEfforts:
                  m.reasoningEfforts?.length
                    ? m.reasoningEfforts.map((e) => ({
                        id: e.id,
                        value: e.value,
                        label: e.label,
                        description: e.description,
                        isDefault: e.isDefault,
                      }))
                    : prior?.reasoningEfforts,
                contextWindow:
                  m.contextWindow != null && m.contextWindow > 0
                    ? m.contextWindow
                    : (prior?.contextWindow ?? null),
                contextWindows:
                  positiveContextWindows(m.contextWindows) ??
                  prior?.contextWindows,
              };
            });
          });
        })
        .catch(() => {});
    }
  }, [session.state, session.sessionId]);
  /** Context usage chip label/state from compact events + message estimate. */
  const contextUsageDisplay = useMemo(() => {
    const base = resolveContextUsageDisplay(
      contextUsage,
      messages,
      locale,
      currentModelWindow,
    );
    return {
      ...base,
      contextInfo: contextUsage.contextInfo ?? null,
      contextInfoNonce: contextUsage.contextInfoNonce ?? 0,
    };
  }, [contextUsage, messages, locale, currentModelWindow]);
  return { currentModelWindow, contextUsageDisplay, setDraftContextWindow };
}

export function useWorkbenchContextActions({
  session, activeCustomProvider, availableModels, modelId, locale,
  currentModelWindow, refreshProviderRoute, setDraftContextWindow,
  setContextUsage, showToast, tr,
}: ContextInputs & {
  currentModelWindow: number | null;
  refreshProviderRoute: () => Promise<unknown>;
  setDraftContextWindow: Dispatch<SetStateAction<number | null>>;
  setContextUsage: Dispatch<SetStateAction<ContextUsageState>>;
  showToast: (message: string, duration: number) => void;
  tr: ReturnType<typeof createT>;
}) {
  const handleContextWindow = useCallback(
    async (tokens: number) => {
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
              activeCustomProvider.models,
              activeCustomProvider.model,
              tokens,
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
      const model = availableModels.find((m) => m.id === modelId);
      const options = selectableContextWindows(model);
      if (!options.includes(tokens)) {
        showToast(
          tr("slash.contextWindowUnknown", {
            token: String(tokens),
            options: options.map((n) => formatWindowLabel(n)).join(", "),
          }),
          4000,
        );
        return false;
      }
      try {
        await api.sessionSetContextWindow(tokens, {
          sessionId: session.sessionId,
        });
        if (!session.sessionId) setDraftContextWindow(tokens);
        setContextUsage((prev) => ({
          ...prev,
          agentContextWindow: tokens,
        }));
        return true;
      } catch (e) {
        showToast(String(e), 4000);
        return false;
      }
    },
    [
      activeCustomProvider,
      availableModels,
      modelId,
      refreshProviderRoute,
      session.sessionId,
      showToast,
      tr,
    ],
  );
  const runContextWindowSlash = useCallback(
    async (args: string) => {
      const model = availableModels.find((m) => m.id === modelId);
      const result = resolveContextWindowCommand({
        args,
        hasModel: Boolean(modelId),
        hasSession: Boolean(session.sessionId),
        options: activeCustomProvider ? [] : selectableContextWindows(model),
        current: currentModelWindow,
      });
      if (result.kind === "switch") {
        const ok = await handleContextWindow(result.window);
        if (ok) {
          showToast(
            tr("slash.contextWindowSwitched", {
              size: formatTokenCount(result.window, locale),
            }),
            2500,
          );
        }
        return;
      }
      const feedback = contextWindowSlashFeedback(result);
      showToast(tr(feedback.key, feedback.vars), 4000);
    },
    [
      activeCustomProvider,
      availableModels,
      currentModelWindow,
      handleContextWindow,
      locale,
      modelId,
      session.sessionId,
      showToast,
      tr,
    ],
  );
  const runContextSlash = useCallback(
    async (announce: boolean) => {
      if (!api.isTauri() || !session.sessionId) {
        if (announce) showToast(tr("slash.contextNoSession"), 4000);
        return;
      }
      try {
        const raw = await api.sessionContextInfo(session.sessionId);
        const view = parseContextInfoPayload(raw);
        if (!view) {
          if (announce) showToast(tr("slash.contextUnavailable"), 4000);
          return;
        }
        setContextUsage((prev) => ({
          ...prev,
          knownTokens: view.used,
          agentContextWindow:
            view.total > 0 ? view.total : prev.agentContextWindow,
          agentPercentage: null,
          lastCompactMessageId: null,
          contextInfo: view,
          contextInfoNonce: (prev.contextInfoNonce ?? 0) + 1,
        }));
      } catch (e) {
        if (announce) showToast(String(e), 4000);
      }
    },
    [session.sessionId, showToast, tr],
  );
  return { handleContextWindow, runContextWindowSlash, runContextSlash };
}
