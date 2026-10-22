/**
 * Shared questionnaire state + auto-cancel clock for the composer gate
 * and the settings demo modal.
 */
import { useEffect, useMemo, useRef, useState } from "react";
import { dropAskUserClock, getAskUserClocks, isAskUserClockPaused, pauseAskUserClock } from "@/lib/askUser/askUserClocks";
import { askUserClockKey, sameAskUserRequest } from "@/lib/askUser/askUserIdentity";
import type { AskUserActionOutcome, AskUserActionResult } from "@/lib/askUser/askUserSettle";
import {
  askUserBuildAnswers,
  askUserCanSubmit,
  askUserQuestionKey,
} from "@/lib/askUser/askUserForm";
import { askUserTimeoutRemainingSec } from "@/lib/askUser/askUserTimeout";
import { resumeGateClock } from "@/lib/gateClock";
import type { AskUserPayload, AskUserQuestionItem } from "@/lib/session";
type Action = () => AskUserActionResult | Promise<AskUserActionResult>;

export function useAskUserQuestionnaire(
  payload: AskUserPayload | null,
  timeoutSec: number,
  onCancel: Action,
) {
  const questions = payload?.questions ?? [];
  const requestKey = payload ? askUserClockKey(payload) : null;
  const payloadRef = useRef(payload);
  payloadRef.current = payload;
  const [selected, setSelected] = useState<Record<string, string[]>>({});
  const [freeText, setFreeText] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [countdownSec, setCountdownSec] = useState<number | null>(null);
  const claimsRef = useRef(new Set<{ payload: AskUserPayload }>());
  const hasClaim = (request: AskUserPayload) => [...claimsRef.current].some((claim) => sameAskUserRequest(claim.payload, request));
  const completedRef = useRef<AskUserPayload | null>(null);
  const onCancelRef = useRef(onCancel);
  onCancelRef.current = onCancel;

  useEffect(() => {
    setSelected({});
    setFreeText({});
    setBusy(Boolean(payload && hasClaim(payload)));
    setCountdownSec(null);
  }, [requestKey]);

  const runAction = async (action: Action): Promise<AskUserActionOutcome> => {
    if (!payload || !sameAskUserRequest(payloadRef.current, payload) ||
        hasClaim(payload) || sameAskUserRequest(completedRef.current, payload)) {
      return { kind: "skipped" };
    }
    const claim = { payload };
    claimsRef.current.add(claim);
    setBusy(true);
    setCountdownSec(null);
    const key = askUserClockKey(payload);
    try {
      const outcome: AskUserActionOutcome = (await action()) ?? { kind: "delivered" };
      if (outcome.kind === "failed") {
        pauseAskUserClock(key);
      } else if (outcome.kind === "delivered") {
        dropAskUserClock(key);
        if (sameAskUserRequest(payloadRef.current, payload)) completedRef.current = payload;
      }
      return outcome;
    } catch (error) {
      pauseAskUserClock(key);
      return { kind: "failed", error };
    } finally {
      claimsRef.current.delete(claim);
      if (sameAskUserRequest(payloadRef.current, payload)) setBusy(false);
    }
  };
  const cancel = () => runAction(() => onCancelRef.current());
  const cancelRef = useRef(cancel);
  cancelRef.current = cancel;

  const open = Boolean(payload && questions.length > 0);
  useEffect(() => {
    if (!open || !payload || !requestKey || busy || !(timeoutSec > 0) ||
        isAskUserClockPaused(requestKey) || sameAskUserRequest(completedRef.current, payload)) {
      setCountdownSec(null);
      return;
    }
    const startedAt = resumeGateClock(getAskUserClocks(), requestKey);
    setCountdownSec(askUserTimeoutRemainingSec(startedAt, timeoutSec));
    const tick = window.setInterval(() => {
      if (sameAskUserRequest(payloadRef.current, payload)) {
        setCountdownSec(askUserTimeoutRemainingSec(startedAt, timeoutSec, Date.now()));
      }
    }, 250);
    const t = window.setTimeout(
      () => {
        if (!sameAskUserRequest(payloadRef.current, payload) ||
            hasClaim(payload) || sameAskUserRequest(completedRef.current, payload)) return;
        void cancelRef.current();
      },
      Math.max(0, timeoutSec * 1000 - (Date.now() - startedAt)),
    );
    return () => {
      window.clearTimeout(t);
      window.clearInterval(tick);
    };
  }, [open, requestKey, timeoutSec, busy]);

  const canSubmit = useMemo(
    () => askUserCanSubmit(questions, selected, freeText),
    [questions, selected, freeText],
  );

  const toggleOption = (
    q: AskUserQuestionItem,
    index: number,
    optionId: string,
  ) => {
    const key = askUserQuestionKey(q, index);
    setSelected((prev) => {
      const cur = prev[key] || [];
      if (q.multiSelect) {
        const has = cur.includes(optionId);
        return {
          ...prev,
          [key]: has ? cur.filter((id) => id !== optionId) : [...cur, optionId],
        };
      }
      return { ...prev, [key]: [optionId] };
    });
    setFreeText((prev) => {
      if (!prev[key]) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  };

  const writeFreeText = (
    key: string,
    value: string,
    clearSingleSelect: boolean,
  ) => {
    setFreeText((prev) => ({ ...prev, [key]: value }));
    if (clearSingleSelect) {
      setSelected((prev) => ({ ...prev, [key]: [] }));
    }
  };

  const submit = async (
    onSubmit: (answers: Record<string, string>) => AskUserActionResult | Promise<AskUserActionResult>,
    answers?: Record<string, string>,
  ) => runAction(() => onSubmit(answers ?? askUserBuildAnswers(questions, selected, freeText)));

  return {
    questions,
    open,
    selected,
    freeText,
    writeFreeText,
    busy,
    countdownSec,
    canSubmit,
    toggleOption,
    submit,
    cancel,
  };
}
