import { useCallback, useEffect, useRef, useState } from "react";
import { sessionResolvePermission } from "@/lib/api";
import { samePermissionRequest } from "@/lib/session/permissionIdentity";
import type { VoicePermissionPrompt } from "@/lib/voiceOverlay";
import type { PermissionDecision } from "./usePermissionResolution";

type Reply = { token: symbol; prompt: VoicePermissionPrompt };

/** Reply ownership includes the overlay lifetime and the displayed request. */
export function useVoicePermissionResolution(c: {
  open: boolean;
  onResolved: (prompt: VoicePermissionPrompt, decision: PermissionDecision) => void;
  onFailure: (prompt: VoicePermissionPrompt, error: unknown) => void;
}) {
  const [pendingPermission, setPending] = useState<VoicePermissionPrompt | null>(null);
  const [reply, setReply] = useState<Reply | null>(null);
  const currentRef = useRef<VoicePermissionPrompt | null>(null);
  const revisionRef = useRef(0);
  const replyRef = useRef<Reply | null>(null);
  const liveRef = useRef(true);
  const effectsRef = useRef(c);
  effectsRef.current = c;
  const setPendingPermission = useCallback((prompt: VoicePermissionPrompt | null) => {
    if (!prompt || !samePermissionRequest(currentRef.current, prompt)) {
      revisionRef.current += 1;
    }
    currentRef.current = prompt;
    setPending(prompt);
    if (!prompt) {
      replyRef.current = null;
      setReply(null);
    }
  }, []);
  useEffect(() => {
    liveRef.current = true;
    return () => {
      liveRef.current = false;
      currentRef.current = null;
      revisionRef.current += 1;
    };
  }, []);
  useEffect(() => {
    if (!c.open) setPendingPermission(null);
  }, [c.open, setPendingPermission]);

  const resolveVoicePermission = useCallback(async (
    prompt: VoicePermissionPrompt, decision: PermissionDecision, optionId: string,
  ) => {
    if (!effectsRef.current.open || !samePermissionRequest(currentRef.current, prompt) ||
      (replyRef.current && samePermissionRequest(replyRef.current.prompt, prompt))) return;
    const revision = revisionRef.current;
    const ownedReply: Reply = { token: Symbol(), prompt };
    replyRef.current = ownedReply;
    setReply(ownedReply);
    const owns = () => liveRef.current && effectsRef.current.open &&
      revisionRef.current === revision && samePermissionRequest(currentRef.current, prompt);
    try {
      await sessionResolvePermission({
        rpcId: prompt.rpcId,
        requestGeneration: prompt.requestGeneration,
        decision, optionId, scopeKey: prompt.scopeKey || undefined,
        sessionId: prompt.sessionId, options: prompt.options, toolName: prompt.toolName,
      });
      if (!owns()) return;
      effectsRef.current.onResolved(prompt, decision);
      // A synchronous resolved-event listener may already have raised a new card.
      if (owns()) setPendingPermission(null);
    } catch (error) {
      if (owns()) effectsRef.current.onFailure(prompt, error);
    } finally {
      if (liveRef.current && replyRef.current?.token === ownedReply.token) {
        replyRef.current = null;
        setReply((current) => current?.token === ownedReply.token ? null : current);
      }
    }
  }, [setPendingPermission]);
  return {
    pendingPermission, setPendingPermission, resolveVoicePermission,
    resolvingPermission: !!reply && !!pendingPermission && samePermissionRequest(reply.prompt, pendingPermission),
  };
}
