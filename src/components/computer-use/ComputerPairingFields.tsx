import { useEffect, useId, useRef, useState } from "react";
import { createT, type Locale } from "@/i18n";
import { IconCheck, IconCopy } from "@/components/icons";
import type { ComputerPairingChallenge } from "@/lib/api/computerUse";

/** Only mount for an explicitly confirmed challenge; never copy its nonce. */
export function ComputerPairingFields({ locale, challenge, disabled, onExpired, focusFrom }: {
  locale: Locale;
  challenge: ComputerPairingChallenge;
  disabled: boolean;
  onExpired: () => void;
  focusFrom?: HTMLElement | null;
}) {
  const tr = createT(locale);
  const id = useId();
  const live = useRef(false);
  const revision = useRef(0);
  const copying = useRef(false);
  const address = useRef<HTMLInputElement>(null);
  const [pending, setPending] = useState(false);
  const [copied, setCopied] = useState<"address" | "code" | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    live.current = true;
    return () => { live.current = false; revision.current += 1; };
  }, []);
  useEffect(() => {
    if (focusFrom && (document.activeElement === focusFrom
      || (!focusFrom.isConnected && document.activeElement === document.body))) {
      address.current?.focus();
    }
  }, [focusFrom]);

  async function copy(field: "address" | "code") {
    if (disabled || copying.current) return;
    if (challenge.expiresAtMs <= Date.now()) { onExpired(); return; }
    const epoch = ++revision.current;
    copying.current = true;
    setPending(true);
    setCopied(null);
    setFailed(false);
    const current = () => live.current && epoch === revision.current;
    try {
      await navigator.clipboard.writeText(field === "address" ? challenge.endpoint : challenge.verificationCode);
      if (current() && challenge.expiresAtMs > Date.now()) setCopied(field);
    } catch {
      if (current() && challenge.expiresAtMs > Date.now()) setFailed(true);
    } finally {
      if (current()) {
        copying.current = false;
        setPending(false);
        if (challenge.expiresAtMs <= Date.now()) onExpired();
      }
    }
  }

  return <div className="cu-pairing__fields">
    {(["address", "code"] as const).map(field => {
      const label = tr(field === "address" ? "cu.panel.pairingEndpoint" : "cu.extension.code");
      const copyLabel = tr(field === "address" ? "cu.pairing.copyAddress" : "cu.pairing.copyCode");
      return <div className="cu-pairing__field" key={field}>
        <label htmlFor={`${id}-${field}`}>{label}</label>
        <div className="cu-pairing__copy-row">
          <input id={`${id}-${field}`} ref={field === "address" ? address : undefined}
            className="input" readOnly spellCheck={false}
            autoComplete="off" value={field === "address" ? challenge.endpoint : challenge.verificationCode}
            data-testid={field === "code" ? "cu-pairing-code" : undefined}
            onFocus={event => event.currentTarget.select()} />
          <button type="button" className="btn btn--ghost cu-pairing__copy"
            disabled={disabled || pending} aria-label={copyLabel} title={copyLabel}
            onClick={() => void copy(field)}>
            {copied === field ? <IconCheck size={16} aria-hidden /> : <IconCopy size={16} aria-hidden />}
          </button>
        </div>
      </div>;
    })}
    <p className="cu-pairing__feedback" data-state={failed ? "error" : "idle"} role="status" aria-live="polite">
      {failed ? tr("cu.pairing.copyFailed") : copied ? tr("cu.pairing.copied") : null}
    </p>
  </div>;
}
