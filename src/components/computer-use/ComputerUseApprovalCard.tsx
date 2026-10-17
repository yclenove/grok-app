/**
 * Computer Use app approval. Deny is the default. Chat Enter is not handled here.
 */

import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { PermissionCountdown } from "@/components/PermissionCountdown";
import { useComputerUseApprovals } from "@/hooks/useComputerUseApprovals";
import { createT, type Locale, type MessageKey } from "@/i18n";
import { cuApprovalDecide } from "@/lib/api/computerUse";
import {
  assess,
  cardsForSession,
  claimFor,
  faultFrom,
  sanitizePlain,
  type Act,
  type Card,
  type Claim,
  type Fault,
} from "@/lib/computer-use/approvalDesk";
import { permissionTimeoutRemainingSec } from "@/lib/permissionTimeout";

export type ApprovalHandle = {
  submit: (claim: Claim) => Promise<void>;
};

type Transport = (claim: Claim) => Promise<void>;

const CHOICES: Array<{ act: Act; label: MessageKey; className: string }> = [
  { act: "deny", label: "approval.choice.deny", className: "perm-bar__btn perm-bar__btn--deny" },
  { act: "turn", label: "approval.choice.turn", className: "perm-bar__btn perm-bar__btn--allow" },
  {
    act: "session",
    label: "approval.choice.session",
    className: "perm-bar__btn perm-bar__btn--session",
  },
  {
    act: "always",
    label: "approval.choice.always",
    className: "perm-bar__btn perm-bar__btn--allow",
  },
];

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
  }
}

function signatureKey(signature: Card["image"]["signature"]): MessageKey {
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

export function ComputerUseApprovalCard(props: {
  card: Card;
  locale: Locale;
  busy: boolean;
  error: Fault | null;
  onSubmit: (claim: Claim) => void;
}) {
  const { card, locale, busy, error, onSubmit } = props;
  const tr = createT(locale);
  const titleId = `cu-approval-${card.requestId}-title`;
  const reasonId = `cu-approval-${card.requestId}-reason`;
  const locked = busy || error === "absent" || error === "replay";
  const onSubmitRef = useRef(onSubmit);
  onSubmitRef.current = onSubmit;
  const cardRef = useRef(card);
  cardRef.current = card;

  useEffect(() => {
    const fire = () => onSubmitRef.current(claimFor(cardRef.current, "timeout"));
    const left = permissionTimeoutRemainingSec(card.openedMs, card.timeoutSecs);
    if (left <= 0) {
      fire();
      return;
    }
    const id = window.setTimeout(fire, left * 1000);
    return () => window.clearTimeout(id);
  }, [card.openedMs, card.requestId, card.timeoutSecs]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onSubmitRef.current(claimFor(cardRef.current, "deny"));
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [card.requestId]);

  function onDialogKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Enter") return;
    const target = event.target;
    if (!(target instanceof HTMLElement) || target.closest("button")) return;
    if (target !== event.currentTarget) return;
    event.preventDefault();
    onSubmit(claimFor(card, "deny"));
  }

  const path = sanitizePlain(card.image.imagePath);
  const publisher = sanitizePlain(card.image.publisher ?? "");
  const name = sanitizePlain(card.name);
  const reason = sanitizePlain(card.reason);
  const choices = CHOICES.filter((choice) => choice.act !== "always" || card.alwaysEnabled);

  return (
    <div
      className="perm-bar"
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      aria-describedby={reasonId}
      aria-busy={busy}
      onKeyDown={onDialogKeyDown}
    >
      <div className="sr-only" aria-live="assertive">
        {tr("approval.title")}
      </div>
      <div className="perm-bar__head">
        <span className="perm-bar__badge" id={titleId}>
          {tr("approval.title")}
        </span>
        <span className="perm-bar__tool">
          {tr(card.risk === "high" ? "approval.risk.high" : "approval.risk.low")}
        </span>
        <PermissionCountdown
          startedAtMs={card.openedMs}
          timeoutSec={card.timeoutSecs}
          format={(seconds) => tr("approval.countdown", { seconds })}
        />
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
      <p className="perm-bar__aux">
        {tr("approval.name_label")}: {name}
      </p>
      {card.hasIcon ? <p className="perm-bar__aux">{tr("approval.icon_label")}</p> : null}
      <p className="perm-bar__reason" id={reasonId} data-testid="cu-reason">
        <span className="perm-bar__aux">{tr("approval.reason_label")}</span>{" "}
        {reason || tr("approval.reason_empty")}
      </p>
      {error ? (
        <p className="perm-bar__error" role="alert">
          {tr(errorKey(error))}
        </p>
      ) : null}
      {busy ? <p className="perm-bar__aux">{tr("approval.submitting")}</p> : null}
      <div className="perm-bar__actions" role="group">
        {choices.map((choice) => (
          <button
            key={choice.act}
            type="button"
            className={choice.className}
            disabled={locked}
            onClick={() => onSubmit(claimFor(card, choice.act))}
          >
            {tr(choice.label)}
          </button>
        ))}
      </div>
    </div>
  );
}

export const ComputerUseApprovalSession = forwardRef<
  ApprovalHandle,
  {
    cards: Card[];
    activeSessionId: string;
    locale: Locale;
    transport: Transport;
  }
>(function ComputerUseApprovalSession(props, ref) {
  const { cards, activeSessionId, locale, transport } = props;
  const inflight = useRef(false);
  const [busyId, setBusyId] = useState<number | null>(null);
  const [error, setError] = useState<{ id: number; fault: Fault } | null>(null);
  const latest = useRef({ cards, transport, inflight, setBusyId, setError });
  latest.current = { cards, transport, inflight, setBusyId, setError };

  async function submit(claim: Claim): Promise<void> {
    const state = latest.current;
    const card = state.cards.find((item) => item.requestId === claim.requestId);
    if (!card) {
      state.setError({ id: claim.requestId, fault: "absent" });
      return;
    }
    if (state.inflight.current) return;
    const fault = assess(card, claim);
    if (fault) {
      state.setError({ id: card.requestId, fault });
      return;
    }
    state.inflight.current = true;
    state.setBusyId(card.requestId);
    state.setError(null);
    try {
      await state.transport(claim);
    } catch (err) {
      const code = faultFrom(err);
      state.setError({ id: card.requestId, fault: code });
      if (code === "failed") {
        state.inflight.current = false;
        state.setBusyId(null);
      }
    }
  }

  const submitRef = useRef(submit);
  submitRef.current = submit;
  useImperativeHandle(ref, () => ({ submit: (claim) => submitRef.current(claim) }), []);

  const visible = cardsForSession({ cards, consumed: [] }, activeSessionId);
  if (visible.length === 0) return null;
  return (
    <>
      {visible.map((card) => (
        <ComputerUseApprovalCard
          key={card.requestId}
          card={card}
          locale={locale}
          busy={busyId === card.requestId}
          error={error?.id === card.requestId ? error.fault : null}
          onSubmit={(claim) => {
            void submit(claim);
          }}
        />
      ))}
    </>
  );
});

export function ComputerUseApprovalSlot(props: { sessionId: string | null; locale: Locale }) {
  const desk = useComputerUseApprovals();
  return (
    <ComputerUseApprovalSession
      cards={desk.cards}
      activeSessionId={props.sessionId ?? ""}
      locale={props.locale}
      transport={cuApprovalDecide}
    />
  );
}
