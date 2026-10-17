/**
 * Handoff card. "I've handled it" sends a new message. It does not replay or clear Stop.
 */

import { useRef, useState, type KeyboardEvent as ReactKeyboardEvent } from "react";
import { useComputerUseHandoff } from "@/hooks/useComputerUseSensitive";
import { createT, type Locale, type MessageKey } from "@/i18n";
import { cuHandoffDecide } from "@/lib/api/computerUse";
import { sessionSend } from "@/lib/api/session";
import { faultFrom, sanitizePlain, type Fault } from "@/lib/computer-use/approvalDesk";
import {
  cardsForSession,
  handoffClaimFor,
  handoffContinuation,
  reduceHandoff,
  type HandoffCard,
  type HandoffClaim,
  type HandoffEnd,
  type HandoffFollow,
  type HandoffPhase,
} from "@/lib/computer-use/confirmState";

type Transport = (claim: HandoffClaim) => Promise<HandoffEnd>;

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

function signatureKey(signature: HandoffCard["image"]["signature"]): MessageKey {
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

function reasonKey(code: string): MessageKey {
  switch (code) {
    case "login":
      return "approval.handoff.login";
    case "captcha":
      return "approval.handoff.captcha";
    case "system_prompt":
      return "approval.handoff.system_prompt";
    default:
      return "approval.handoff.other";
  }
}

export function ComputerUseHandoffCard(props: {
  card: HandoffCard;
  locale: Locale;
  phase: HandoffPhase;
  error: Fault | null;
  onSubmit: (claim: HandoffClaim) => void;
}) {
  const { card, locale, phase, error, onSubmit } = props;
  const tr = createT(locale);
  const titleId = `cu-handoff-${card.requestId}-title`;
  const detailId = `cu-handoff-${card.requestId}-detail`;
  const hintId = `cu-handoff-${card.requestId}-hint`;
  const locked = phase !== "pending" || error === "absent" || error === "replay";
  const name = sanitizePlain(card.name);
  const detail = sanitizePlain(card.detail);
  const path = sanitizePlain(card.image.imagePath);
  const publisher = sanitizePlain(card.image.publisher ?? "");

  function onDialogKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (phase !== "pending" || event.key !== "Enter") return;
    const target = event.target;
    if (!(target instanceof HTMLElement) || target.closest("button")) return;
    if (target !== event.currentTarget) return;
    event.preventDefault();
  }

  return (
    <div
      className="perm-bar"
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      aria-describedby={detail ? detailId : hintId}
      aria-busy={phase === "submitting"}
      onKeyDown={onDialogKeyDown}
    >
      <div className="sr-only" aria-live="assertive">
        {tr("approval.handoff.title")}
      </div>
      <div className="perm-bar__head">
        <span className="perm-bar__badge" id={titleId}>
          {tr("approval.handoff.title")}
        </span>
        <span className="perm-bar__tool">{tr(reasonKey(card.code), { name })}</span>
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
      {detail ? (
        <p className="perm-bar__reason" id={detailId} data-testid="cu-handoff-detail">
          <span className="perm-bar__aux">{tr("approval.handoff.detail_label")}</span> {detail}
        </p>
      ) : null}
      <p className="perm-bar__aux" id={hintId}>
        {tr("approval.handoff.hint")}
      </p>
      {phase === "closed" ? (
        <p className="perm-bar__aux" role="status">
          {tr("approval.confirm.closed")}
        </p>
      ) : null}
      {error ? (
        <p className="perm-bar__error" role="alert">
          {tr(errorKey(error))}
        </p>
      ) : null}
      {phase === "submitting" ? (
        <p className="perm-bar__aux">{tr("approval.handoff.submitting")}</p>
      ) : null}
      <div className="perm-bar__actions" role="group">
        <button
          type="button"
          className="perm-bar__btn perm-bar__btn--allow"
          disabled={locked}
          onClick={() => onSubmit(handoffClaimFor(card))}
        >
          {tr("approval.handoff.handled")}
        </button>
      </div>
    </div>
  );
}

export function ComputerUseHandoffSession(props: {
  cards: HandoffCard[];
  phases: Record<number, HandoffPhase>;
  activeSessionId: string;
  locale: Locale;
  transport: Transport;
  onPhase: (requestId: number, phase: HandoffPhase) => void;
  onContinue: (follow: HandoffFollow) => Promise<void>;
}) {
  const { cards, phases, activeSessionId, locale, transport, onPhase, onContinue } = props;
  const inflight = useRef(false);
  const [error, setError] = useState<{ id: number; fault: Fault } | null>(null);
  const latest = useRef({ cards, phases, transport, onPhase, onContinue, inflight, setError, locale });
  latest.current = { cards, phases, transport, onPhase, onContinue, inflight, setError, locale };

  async function submit(claim: HandoffClaim): Promise<void> {
    const state = latest.current;
    const card = state.cards.find((item) => item.requestId === claim.requestId);
    const phase = state.phases[claim.requestId] ?? "pending";
    if (!card || phase !== "pending" || state.inflight.current) return;
    state.inflight.current = true;
    state.onPhase(card.requestId, reduceHandoff(phase, { type: "submit" }));
    state.setError(null);
    let end: HandoffEnd;
    try {
      end = await state.transport(claim);
    } catch (err) {
      state.setError({ id: card.requestId, fault: faultFrom(err) });
      state.onPhase(card.requestId, reduceHandoff("submitting", { type: "fault" }));
      state.inflight.current = false;
      return;
    }
    const follow = handoffContinuation(
      createT(state.locale)("approval.handoff.continue_message", {
        name: sanitizePlain(card.name),
      }),
    );
    if (end === "handled" && !follow.clearStop && !follow.replay) {
      try {
        await state.onContinue(follow);
      } catch (err) {
        state.setError({ id: card.requestId, fault: faultFrom(err) });
      }
    }
    state.onPhase(card.requestId, reduceHandoff("submitting", { type: "resolve", end }));
  }

  const visible = cardsForSession(cards, activeSessionId);
  if (visible.length === 0) return null;
  return (
    <>
      {visible.map((card) => (
        <ComputerUseHandoffCard
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

export function ComputerUseHandoffSlot(props: { sessionId: string | null; locale: Locale }) {
  const desk = useComputerUseHandoff();
  return (
    <ComputerUseHandoffSession
      cards={desk.cards}
      phases={desk.phases}
      activeSessionId={props.sessionId ?? ""}
      locale={props.locale}
      transport={cuHandoffDecide}
      onPhase={desk.setPhase}
      onContinue={async (follow) => {
        if (follow.clearStop || follow.replay || !props.sessionId) return;
        await sessionSend(follow.text, follow.text, props.sessionId);
      }}
    />
  );
}
