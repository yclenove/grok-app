import { useEffect, useRef, useState } from "react";
import { createT, intlLocale, type Locale } from "@/i18n";
import * as api from "@/lib/api/computerUse";
import { ComputerPairingFields } from "./ComputerPairingFields";

export function ComputerPairing({ locale, disabled, onChanged }: {
  locale: Locale;
  disabled: boolean;
  onChanged: () => void;
}) {
  const tr = createT(locale);
  const [challenge, setChallenge] = useState<api.ComputerPairingChallenge | null>(null);
  const [confirmed, setConfirmed] = useState(false);
  const [focusAfterConfirm, setFocusAfterConfirm] = useState<HTMLElement | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [seconds, setSeconds] = useState(0);
  const [expired, setExpired] = useState(false);
  const live = useRef(false);
  const inFlight = useRef(false);
  const step = !challenge ? 0 : confirmed ? 2 : 1;
  function expire() { setChallenge(null); setConfirmed(false); setExpired(true); }
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  useEffect(() => {
    if (!challenge) return;
    function tick() {
      const remaining = Math.max(0, Math.ceil((challenge!.expiresAtMs - Date.now()) / 1000));
      setSeconds(remaining);
      if (!remaining) { setChallenge(null); setConfirmed(false); setExpired(true); }
    }
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [challenge]);

  async function perform(action: "begin" | "confirm" | "revoke", trigger?: HTMLElement) {
    if (disabled || inFlight.current) return;
    // The countdown is presentation, not authority to confirm between ticks.
    if (action === "confirm" && (!challenge || challenge.expiresAtMs <= Date.now())) {
      expire();
      return;
    }
    inFlight.current = true;
    setBusy(true);
    setError(null);
    try {
      if (action === "begin") {
        setExpired(false);
        setChallenge(null);
        setConfirmed(false);
        setFocusAfterConfirm(null);
        const next = await api.computerBeginPairing();
        if (!next) throw new Error(tr("cu.panel.pairingUnavailable"));
        if (live.current) setChallenge(next);
      } else if (action === "confirm" && challenge) {
        await api.computerConfirmPairingApp(challenge.nonce);
        if (live.current && challenge.expiresAtMs > Date.now()) {
          setConfirmed(true);
          setFocusAfterConfirm(trigger ?? null);
        }
      } else if (action === "revoke") {
        await api.computerRevokePairing();
        if (live.current) { setChallenge(null); setConfirmed(false); setExpired(false); }
      }
      if (live.current) onChanged();
    } catch (cause) {
      if (live.current) setError(String(cause));
    } finally {
      inFlight.current = false;
      if (live.current) setBusy(false);
    }
  }

  return <section className="cu-panel__pairing" aria-label={tr("cu.panel.pairExtension")}>
    <ol className="cu-pairing__steps" aria-label={tr("cu.pairing.steps")}>
      {(["cu.pairing.stepStart", "cu.pairing.stepConfirm", "cu.pairing.stepShare"] as const).map((key, index) => (
        <li key={key} aria-current={index === step ? "step" : undefined}
          data-state={index < step ? "complete" : index === step ? "current" : "pending"}>
          <span className="cu-pairing__step-number" aria-hidden>{new Intl.NumberFormat(intlLocale(locale)).format(index + 1)}</span>
          <span>{tr(key)}</span>
        </li>
      ))}
    </ol>
    <div className="cu-panel__actions">
      <button type="button" className="btn btn--ghost btn--sm" disabled={disabled || busy}
        onClick={() => void perform("begin")}>{tr("cu.panel.pairExtension")}</button>
      <button type="button" className="btn btn--ghost btn--sm" disabled={disabled || busy}
        onClick={() => void perform("revoke")}>{tr("cu.panel.revokePairing")}</button>
    </div>
    {challenge && <>
      <p className="muted">{tr("cu.panel.pairingIdentity", { id: challenge.installedExtensionId })}</p>
      {!confirmed ? <button type="button" className="btn btn--solid btn--sm" disabled={disabled || busy}
        onClick={event => void perform("confirm", event.currentTarget)}>{tr("cu.panel.confirmPairing")}</button> : <>
        <p className="cu-panel__hint">{tr("cu.pairing.waitingShare")}</p>
        <ComputerPairingFields key={challenge.nonce} locale={locale} challenge={challenge}
          disabled={disabled || busy} onExpired={expire} focusFrom={focusAfterConfirm} />
        <p className="cu-panel__pairing-expiry">{tr("cu.pairing.expires", {
          seconds: new Intl.NumberFormat(intlLocale(locale)).format(seconds),
        })}</p>
      </>}
    </>}
    {expired && <p className="cu-panel__warn">{tr("cu.pairing.expired")}</p>}
    {error && <p role="alert" className="cu-panel__error">{tr("cu.panel.pairingUnavailable")}</p>}
  </section>;
}
