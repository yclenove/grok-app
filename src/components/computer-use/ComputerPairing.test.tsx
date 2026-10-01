/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import * as api from "@/lib/api/computerUse";
import { ComputerPairing } from "./ComputerPairing";
vi.mock("@/lib/api/computerUse", () => ({ computerBeginPairing: vi.fn(), computerConfirmPairingApp: vi.fn(), computerRevokePairing: vi.fn() }));
const challenge = () => ({ nonce: "one", instanceId: "app", verificationCode: "12345-ABCDE-12345-ABCDE",
  endpoint: "http://127.0.0.1:32100", installedExtensionId: "extension", expiresAtMs: Date.now() + 5000 });
beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(api.computerBeginPairing).mockResolvedValue(challenge());
  vi.mocked(api.computerConfirmPairingApp).mockResolvedValue();
  vi.mocked(api.computerRevokePairing).mockResolvedValue();
});
afterEach(() => { cleanup(); vi.useRealTimers(); });
describe("App pairing lifecycle", () => {
  it("does not display a code after Host rejects stale confirmation", async () => {
    vi.mocked(api.computerConfirmPairingApp).mockRejectedValueOnce(new Error("pairing challenge changed"));
    render(<ComputerPairing locale="en" disabled={false} onChanged={() => {}} />);
    fireEvent.click(screen.getByText("Pair browser extension"));
    fireEvent.click(await screen.findByText("Confirm pairing in App"));
    expect(await screen.findByRole("alert")).toHaveTextContent("Pairing is unavailable.");
    expect(screen.queryByTestId("cu-pairing-code")).toBeNull();
  });
  it("clears the displayed code when the challenge expires", async () => {
    vi.useFakeTimers();
    render(<ComputerPairing locale="en" disabled={false} onChanged={() => {}} />);
    await act(async () => fireEvent.click(screen.getByText("Pair browser extension")));
    await act(async () => fireEvent.click(screen.getByText("Confirm pairing in App")));
    expect(screen.getByTestId("cu-pairing-code")).toBeInTheDocument();
    await act(async () => vi.advanceTimersByTime(5001));
    expect(screen.queryByTestId("cu-pairing-code")).toBeNull();
    expect(screen.getByText("This pairing code expired. Start pairing again.")).toBeInTheDocument();
  });
  it("suppresses a late begin response after the panel unmounts", async () => {
    let resolve!: (value: api.ComputerPairingChallenge) => void;
    vi.mocked(api.computerBeginPairing).mockReturnValueOnce(new Promise(done => { resolve = done; }));
    const changed = vi.fn();
    const view = render(<ComputerPairing locale="en" disabled={false} onChanged={changed} />);
    fireEvent.click(screen.getByText("Pair browser extension")); view.unmount();
    await act(async () => resolve(challenge()));
    expect(changed).not.toHaveBeenCalled();
    expect(api.computerConfirmPairingApp).not.toHaveBeenCalled();
  });
  it("replaces the old code only after a new explicit confirmation", async () => {
    render(<ComputerPairing locale="en" disabled={false} onChanged={() => {}} />);
    fireEvent.click(screen.getByText("Pair browser extension"));
    fireEvent.click(await screen.findByText("Confirm pairing in App"));
    await screen.findByTestId("cu-pairing-code");
    await waitFor(() => expect(screen.getByText("Pair browser extension")).toBeEnabled());
    fireEvent.click(screen.getByText("Pair browser extension"));
    await screen.findByText("Confirm pairing in App");
    expect(screen.queryByTestId("cu-pairing-code")).toBeNull();
    expect(api.computerConfirmPairingApp).toHaveBeenCalledTimes(1);
  });
  it("does not confirm an expired challenge between countdown ticks", async () => {
    vi.useFakeTimers();
    render(<ComputerPairing locale="en" disabled={false} onChanged={() => {}} />);
    await act(async () => fireEvent.click(screen.getByText("Pair browser extension")));
    vi.setSystemTime(Date.now() + 6000);
    await act(async () => fireEvent.click(screen.getByText("Confirm pairing in App")));
    expect(api.computerConfirmPairingApp).not.toHaveBeenCalled();
    expect(screen.getByText("This pairing code expired. Start pairing again.")).toBeInTheDocument();
  });
  it("reveals copy controls only after explicit confirmation", async () => {
    render(<ComputerPairing locale="en" disabled={false} onChanged={() => {}} />);
    fireEvent.click(screen.getByText("Pair browser extension"));
    const confirm = await screen.findByText("Confirm pairing in App");
    expect(screen.queryByRole("button", { name: "Copy pairing code" })).toBeNull();
    fireEvent.click(confirm);
    expect(await screen.findByRole("button", { name: "Copy pairing code" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Copy App address" })).toBeEnabled();
    expect(screen.getByRole("list", { name: "Pairing steps" }).querySelector('[aria-current="step"]'))
      .toHaveTextContent("Share a tab");
  });
  it("moves keyboard focus from the removed confirmation button to the first field", async () => {
    render(<ComputerPairing locale="en" disabled={false} onChanged={() => {}} />);
    fireEvent.click(screen.getByText("Pair browser extension"));
    const confirm = await screen.findByText("Confirm pairing in App");
    confirm.focus();
    fireEvent.click(confirm);
    expect(await screen.findByRole("textbox", { name: "App address" })).toHaveFocus();
  });
  it("does not steal focus moved elsewhere while confirmation is pending", async () => {
    let resolve!: () => void;
    vi.mocked(api.computerConfirmPairingApp).mockReturnValueOnce(new Promise(done => { resolve = done; }));
    render(<><button type="button">Other task</button>
      <ComputerPairing locale="en" disabled={false} onChanged={() => {}} /></>);
    fireEvent.click(screen.getByText("Pair browser extension"));
    const confirm = await screen.findByText("Confirm pairing in App");
    confirm.focus();
    fireEvent.click(confirm);
    const other = screen.getByRole("button", { name: "Other task" });
    other.focus();
    await act(async () => resolve());
    expect(await screen.findByRole("textbox", { name: "App address" })).not.toHaveFocus();
    expect(other).toHaveFocus();
  });
});
