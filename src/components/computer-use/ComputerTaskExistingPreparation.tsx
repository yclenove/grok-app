import { useEffect, useState } from "react";
import { createT, intlLocale, type Locale } from "@/i18n";
import type { ComputerTaskActions, ComputerTaskActionState } from "@/lib/computer-use/taskActions";
import { ComputerPairingFields } from "./ComputerPairingFields";
import { ComputerExtensionSetup } from "./ComputerExtensionSetup";

/** Mount only in the original owner's picker. A mirror never sees the code. */
export function ComputerTaskExistingPreparation({ locale, owner, state, actions }: {
  locale: Locale; owner: string; state: ComputerTaskActionState; actions: () => ComputerTaskActions | undefined;
}) {
  const tr = createT(locale);
  const prep = state.preparation;
  const challenge = prep?.pairing;
  const [now, setNow] = useState(() => Date.now());
  const [focusAfterConfirm, setFocusAfterConfirm] = useState<HTMLElement | null>(null);
  const hasCandidates = !!state.picker && "candidates" in state.picker && state.picker.candidates.length > 0;
  const busy = !!state.pending || state.preparationBusy;
  const confirming = !!state.pending || state.preparationConfirming;
  const expired = !!challenge && challenge.expiresAtMs <= now;
  const format = (n: number) => new Intl.NumberFormat(intlLocale(locale)).format(n);
  useEffect(() => {
    const update = () => actions()?.setPickerVisible(owner, !document.hidden);
    update(); document.addEventListener("visibilitychange", update);
    return () => { document.removeEventListener("visibilitychange", update); actions()?.setPickerVisible(owner, false); };
  }, [actions, owner]);
  useEffect(() => {
    if (!challenge) return;
    let timer: ReturnType<typeof setInterval> | undefined;
    const update = () => {
      if (timer !== undefined) clearInterval(timer);
      if (!document.hidden) { setNow(Date.now()); timer = setInterval(() => setNow(Date.now()), 1000); }
    };
    update(); document.addEventListener("visibilitychange", update);
    return () => { if (timer !== undefined) clearInterval(timer); document.removeEventListener("visibilitychange", update); };
  }, [challenge]);

  return <section className="cu-consent__preparation" aria-label={tr("cu.panel.pairExtension")}>
    <p className="cu-panel__hint">{tr("cu.consent.existingScope")}</p>
    {!hasCandidates ? <>
      <ComputerExtensionSetup locale={locale} disabled={!!state.pending} />
      {challenge && !expired ? <>
        {!prep?.pairingConfirmed ? <>
          <p className="cu-panel__hint">{tr("cu.consent.existingPair")}</p>
          <button type="button" className="btn btn--solid" aria-disabled={confirming}
            onClick={event => { if (!confirming) { setFocusAfterConfirm(event.currentTarget); void actions()?.confirmPairing(); } }}>{tr("cu.panel.confirmPairing")}</button>
        </> : <>
          <p className="cu-panel__hint">{tr("cu.panel.pairingHint")}</p>
          <ComputerPairingFields key={challenge.nonce} locale={locale} challenge={challenge} disabled={!!state.pending}
            focusFrom={focusAfterConfirm}
            onExpired={() => { setNow(Date.now()); void actions()?.refreshPreparation(); }} />
        </>}
        <p className="cu-panel__pairing-expiry">{tr("cu.pairing.expires", { seconds: format(Math.max(0, Math.ceil((challenge.expiresAtMs - now) / 1000))) })}</p>
      </> : expired ? <p className="cu-panel__warn">{tr("cu.pairing.expired")}</p>
        : prep ? <p className="cu-panel__hint">{tr("cu.consent.existingShare")}</p> : null}
      {prep?.borrowedTabs ? <p className="cu-panel__warn">{tr("cu.consent.existingBusy", { count: format(prep.borrowedTabs) })}</p> : null}
      {prep?.incompatibleTabs ? <p className="cu-panel__warn">{tr("cu.consent.existingIncompatible", { count: format(prep.incompatibleTabs) })}</p> : null}
      {busy && !prep ? <p role="status">{tr("cu.status.checking")}</p> : null}
      <button type="button" className="btn btn--ghost" aria-disabled={busy || expired}
        onClick={() => { if (!busy && !expired) void actions()?.refreshPreparation(); }}>{tr("cu.consent.existingRefresh")}</button>
    </> : null}
  </section>;
}
