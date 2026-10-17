/**
 * Sensitive-action confirmation. Deny is the default. Chat Enter is not handled here.
 */

import { useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { PermissionCountdown } from "@/components/PermissionCountdown";
import { useComputerUseConfirm } from "@/hooks/useComputerUseSensitive";
import { createT, type Locale, type MessageKey } from "@/i18n";
import { cuConfirmDecide } from "@/lib/api/computerUse";
import { faultFrom, sanitizePlain, type Fault } from "@/lib/computer-use/approvalDesk";
import {
  cardsForSession,
  confirmClaimFor,
  redactedCropSrc,
  reduceConfirm,
  type ConfirmAct,
  type ConfirmCard,
  type ConfirmClaim,
  type ConfirmEnd,
  type ConfirmPhase,
} from "@/lib/computer-use/confirmState";
import { permissionTimeoutRemainingSec } from "@/lib/permissionTimeout";

type Transport = (claim: ConfirmClaim) => Promise<ConfirmEnd>;

function errorKey(fault: Fault): MessageKey {
  switch (fault) {
    case "absent":
      return "approval.error.absent";
    case "identity":
      return "approval.error.identity";
    case "replay":
      return "approval.error.replay";
    case "failed":
      return "approval.error.failed";
    case "stale":
      return "approval.error.stale";
  }
}

function signatureKey(signature: ConfirmCard["image"]["signature"]): MessageKey {
  switch (signature) {
    case "valid":
      return "approval.signature.valid";
    case "invalid":
      return "approval.signature.invalid";
    case "unsigned":
      return "approval.signature.unsigned";
    case "unknown":
      return "approval.signature.unknown";
  }
}

function statusKey(phase: ConfirmPhase): MessageKey | null {
  switch (phase) {
    case "allowed":
      return "approval.confirm.allowed_state";
    case "denied":
      return "approval.confirm.denied_state";
    case "timed_out":
      return "approval.confirm.timed_out";
    case "stale":
      return "approval.confirm.stale";
    case "closed":
      return "approval.confirm.closed";
    case "pending":
    case "submitting":
      return null;
  }
}

export function ComputerUseActionConfirmCard(props: {
  card: ConfirmCard;
  locale: Locale;
  phase: ConfirmPhase;
  error: Fault | null;
  onSubmit: (claim: ConfirmClaim) => void;
}) {
  const { card, locale, phase, error, onSubmit } = props;
  const tr = createT(locale);
  const titleId = `cu-confirm-${card.requestId}-title`;
  const reasonId = `cu-confirm-${card.requestId}-reason`;
  const locked = phase !== "pending" || error === "absent" || error === "replay";
  const onSubmitRef = useRef(onSubmit);
  onSubmitRef.current = onSubmit;
  const cardRef = useRef(card);
  cardRef.current = card;
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  useEffect(() => {
    if (phase !== "pending") return;
    const fire = () => {
      if (phaseRef.current !== "pending") return;
      onSubmitRef.current(confirmClaimFor(cardRef.current, "timeout"));
    };
    const left = permissionTimeoutRemainingSec(card.openedMs, card.timeoutSecs);
    if (left <= 0) {
      fire();
      return;
    }
    const id = window.setTimeout(fire, left * 1000);
    return () => window.clearTimeout(id);
  }, [card.openedMs, card.requestId, card.timeoutSecs, phase]);

  useEffect(() => {
    if (phase !== "pending") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onSubmitRef.current(confirmClaimFor(cardRef.current, "deny"));
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [card.requestId, phase]);

  function onDialogKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (phase !== "pending" || event.key !== "Enter") return;
    const target = event.target;
    if (!(target instanceof HTMLElement) || target.closest("button")) return;
    if (target !== event.currentTarget) return;
    event.preventDefault();
    onSubmit(confirmClaimFor(card, "deny"));
  }

  const path = sanitizePlain(card.image.imagePath);
  const publisher = sanitizePlain(card.image.publisher ?? "");
  const name = sanitizePlain(card.name);
  const summary = sanitizePlain(card.summary);
  const reason = sanitizePlain(card.reason);
  const cropSrc = redactedCropSrc(card.crop);
  const status = statusKey(phase);
  const cropKey: MessageKey =
    card.crop.kind === "missing" && cropSrc === null
      ? "approval.confirm.crop_missing"
      : "approval.confirm.crop_unlocated";

  return (
    <div
      className="perm-bar"
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      aria-describedby={reasonId}
      aria-busy={phase === "submitting"}
      onKeyDown={onDialogKeyDown}
    >
      <div className="sr-only" aria-live="assertive">
        {tr("approval.confirm.title")}
      </div>
      <div className="perm-bar__head">
        <span className="perm-bar__badge" id={titleId}>
          {tr("approval.confirm.title")}
        </span>
        <span className="perm-bar__tool">{tr("approval.confirm.question", { name })}</span>
        {phase === "pending" ? (
          <PermissionCountdown
            startedAtMs={card.openedMs}
            timeoutSec={card.timeoutSecs}
            format={(seconds) => tr("approval.countdown", { seconds })}
          />
        ) : null}
      </div>
      <h2 className="perm-bar__identity">{tr("approval.identity_heading")}</h2>
      <p className="perm-bar__meta">
        {tr("approval.path_label")}: {path || tr("approval.path_missing")}
      </p>
      <p className="perm-bar__meta">
        {tr("approval.publisher_label")}: {publisher || tr("approval.publisher_missing")}
      </p>
      <p className="perm-bar__meta">
        {tr("approval.signature_label")}: {tr(signatureKey(card.image.signature))}
      </p>
      <p className="perm-bar__aux" data-testid="cu-summary">
        <span>{tr("approval.confirm.summary_label")}</span> {summary}
      </p>
      <p className="perm-bar__reason" id={reasonId} data-testid="cu-reason">
        <span className="perm-bar__aux">{tr("approval.reason_label")}</span>{" "}
        {reason || tr("approval.reason_empty")}
      </p>
      {cropSrc ? (
        <img className="perm-bar__crop" src={cropSrc} alt={tr("approval.crop_attached")} />
      ) : (
        <p className="perm-bar__aux" data-testid="cu-crop">
          {tr(cropKey)}
        </p>
      )}
      <p className="perm-bar__aux">{tr("approval.confirm.hint")}</p>
      {status ? (
        <p className="perm-bar__aux" role="status">
          {tr(status)}
        </p>
      ) : null}
      {error ? (
        <p className="perm-bar__error" role="alert">
          {tr(errorKey(error))}
        </p>
      ) : null}
      {phase === "submitting" ? (
        <p className="perm-bar__aux">{tr("approval.confirm.submitting")}</p>
      ) : null}
      <div className="perm-bar__actions" role="group">
        <button
          type="button"
          className="perm-bar__btn perm-bar__btn--deny"
          disabled={locked}
          onClick={() => onSubmit(confirmClaimFor(card, "deny"))}
        >
          {tr("approval.confirm.deny")}
        </button>
        <button
          type="button"
          className="perm-bar__btn perm-bar__btn--allow"
          disabled={locked}
          onClick={() => onSubmit(confirmClaimFor(card, "allow"))}
        >
          {tr("approval.confirm.allow")}
        </button>
      </div>
    </div>
  );
}

export function ComputerUseActionConfirmSession(props: {
  cards: ConfirmCard[];
  phases: Record<number, ConfirmPhase>;
  activeSessionId: string;
  locale: Locale;
  transport: Transport;
  onPhase: (requestId: number, phase: ConfirmPhase) => void;
}) {
  const { cards, phases, activeSessionId, locale, transport, onPhase } = props;
  const inflight = useRef(false);
  const [error, setError] = useState<{ id: number; fault: Fault } | null>(null);
  const latest = useRef({ cards, phases, transport, onPhase, inflight, setError });
  latest.current = { cards, phases, transport, onPhase, inflight, setError };

  async function submit(claim: ConfirmClaim): Promise<void> {
    const state = latest.current;
    const card = state.cards.find((item) => item.requestId === claim.requestId);
    const phase = state.phases[claim.requestId] ?? "pending";
    if (!card || phase !== "pending" || state.inflight.current) return;
    state.inflight.current = true;
    state.onPhase(card.requestId, reduceConfirm(phase, { type: "submit" }));
    state.setError(null);
    try {
      const end = await state.transport(claim);
      state.onPhase(card.requestId, reduceConfirm("submitting", { type: "resolve", end }));
    } catch (err) {
      const code = faultFrom(err);
      state.setError({ id: card.requestId, fault: code });
      const next = code === "stale" ? "stale" : "pending";
      state.onPhase(
        card.requestId,
        next === "stale"
          ? reduceConfirm("submitting", { type: "resolve", end: "stale" })
          : reduceConfirm("submitting", { type: "fault" }),
      );
      if (next === "pending") state.inflight.current = false;
    }
  }

  const visible = cardsForSession(cards, activeSessionId);
  if (visible.length === 0) return null;
  return (
    <>
      {visible.map((card) => (
        <ComputerUseActionConfirmCard
          key={card.requestId}
          card={card}
          locale={locale}
          phase={phases[card.requestId] ?? "pending"}
          error={error?.id === card.requestId ? error.fault : null}
          onSubmit={(claim) => {
            void submit(claim);
          }}
        />
      ))}
    </>
  );
}

export function ComputerUseActionConfirmSlot(props: { sessionId: string | null; locale: Locale }) {
  const desk = useComputerUseConfirm();
  return (
    <ComputerUseActionConfirmSession
      cards={desk.cards}
      phases={desk.phases}
      activeSessionId={props.sessionId ?? ""}
      locale={props.locale}
      transport={cuConfirmDecide}
      onPhase={desk.setPhase}
    />
  );
}

export type { ConfirmAct };
