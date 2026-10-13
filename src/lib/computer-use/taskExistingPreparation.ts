/** One task-bound metadata observer, never a global pairing/grant controller. */
import type { ComputerTaskPickerStarted, ComputerTaskPickerView, ComputerTaskPickerPreparation,
  computerTaskPickerPreparation, computerTaskPickerPairingConfirm, computerTaskPickerLoad } from "@/lib/api/computerTaskPicker";
import { validatePickerPreparation, validatePickerView } from "./taskPickerValidation";
import type { ComputerPairingChallenge } from "@/lib/api/computerUse";

type Patch = { preparation?: ComputerTaskPickerPreparation | null; preparationBusy?: boolean; preparationConfirming?: boolean;
  picker?: ComputerTaskPickerView; failed?: boolean };
type Dependencies = { preparation: typeof computerTaskPickerPreparation; confirm: typeof computerTaskPickerPairingConfirm; load: typeof computerTaskPickerLoad };

export class TaskExistingPreparation {
  private visible = false;
  private visibilityEpoch = 0;
  private disposed = false;
  private busy = false;
  private confirming = false;
  private candidatesFound = false;
  private failed = false;
  private preparation: ComputerTaskPickerPreparation | null = null;
  private queuedConfirmation: ComputerPairingChallenge | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly picker: ComputerTaskPickerStarted, private readonly deps: Dependencies,
    private readonly current: () => boolean, private readonly publish: (patch: Patch) => void) {}

  private live() { return !this.disposed && this.current(); }
  private clearTimer() { if (this.timer !== null) clearTimeout(this.timer); this.timer = null; }
  private schedule() {
    this.clearTimer();
    if (!this.live() || !this.visible || this.busy || this.failed || this.candidatesFound) return;
    this.timer = setTimeout(() => { this.timer = null; void this.read(true); }, 1000);
  }
  setVisible(visible: boolean) {
    if (visible === this.visible || this.disposed) return;
    this.visible = visible;
    // Showing the original modal is not retirement. Hiding it fences every
    // outstanding read, including one that resolves after it is shown again.
    if (!visible) {
      this.visibilityEpoch++; this.queuedConfirmation = null;
      if (this.live()) this.publish({ preparationConfirming: false });
    }
    this.schedule();
  }
  dispose() { this.disposed = true; this.visibilityEpoch++; this.queuedConfirmation = null; this.clearTimer(); }
  refresh = () => this.read(false);
  confirm = async () => {
    const challenge = this.preparation?.pairing;
    if (!this.live() || !challenge || this.preparation?.pairingConfirmed || this.queuedConfirmation || this.confirming
      || challenge.expiresAtMs <= Date.now() || this.candidatesFound) return;
    if (this.busy) {
      // Retain only this explicit gesture, never authority over a future code.
      this.queuedConfirmation = { ...challenge };
      this.publish({ preparationConfirming: true });
      return;
    }
    await this.read(false, true, challenge.nonce);
  };

  private async read(background: boolean, confirm = false, nonce?: string) {
    if (!this.live() || this.busy || this.candidatesFound || (background && !this.visible)) return;
    const challenge = this.preparation?.pairing;
    if (confirm && (!challenge || challenge.nonce !== nonce || this.preparation?.pairingConfirmed || challenge.expiresAtMs <= Date.now())) return;
    const visibility = this.visibilityEpoch;
    const current = () => this.live() && visibility === this.visibilityEpoch;
    this.busy = true; this.confirming = confirm; this.clearTimer();
    this.publish({ preparationBusy: true, preparationConfirming: confirm, failed: false });
    try {
      if (confirm && challenge) {
        await this.deps.confirm(this.picker, challenge.nonce);
        if (!current()) return;
      }
      const prep = await this.deps.preparation(this.picker);
      if (!current()) return;
      validatePickerPreparation(prep, this.picker);
      if (prep.pairing && prep.pairing.expiresAtMs <= Date.now()) throw new Error("picker pairing expired");
      this.preparation = prep;
      this.publish({ preparation: prep });
      if (prep.ready) {
        const view = await this.deps.load(this.picker);
        if (!current()) return;
        validatePickerView(view, this.picker);
        this.candidatesFound = view.candidates.length > 0;
        this.publish({ picker: view });
      }
      this.failed = false;
    } catch {
      if (current()) {
        this.failed = true; this.preparation = null;
        this.publish({ preparation: null, failed: true });
      }
    } finally {
      this.busy = false;
      this.confirming = false;
      const queued = this.queuedConfirmation;
      this.queuedConfirmation = null;
      const next = this.preparation?.pairing;
      const sameChallenge = queued && next && (["nonce", "instanceId", "verificationCode", "endpoint", "installedExtensionId", "expiresAtMs"] as const)
        .every(field => queued[field] === next[field]);
      if (queued && current() && !this.failed && !this.candidatesFound && sameChallenge
        && !this.preparation?.pairingConfirmed && queued.expiresAtMs > Date.now()) {
        await this.read(false, true, queued.nonce);
      } else {
        if (this.live()) this.publish({ preparationBusy: false, preparationConfirming: false });
        this.schedule();
      }
    }
  }
}
