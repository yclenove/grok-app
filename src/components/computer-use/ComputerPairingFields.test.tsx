/** @vitest-environment jsdom */
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { ComputerPairingFields } from "./ComputerPairingFields";

const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
const writeText = vi.fn<(value: string) => Promise<void>>();
const challenge = () => ({ nonce: "private-host-nonce", instanceId: "private-instance",
  verificationCode: "12345-ABCDE-12345-ABCDE", endpoint: "http://127.0.0.1:32100",
  installedExtensionId: "extension", expiresAtMs: Date.now() + 5000 });

beforeEach(() => {
  writeText.mockReset().mockResolvedValue();
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
  else Reflect.deleteProperty(navigator, "clipboard");
});

describe("explicit pairing copy", () => {
  it.each([
    ["Copy App address", "http://127.0.0.1:32100"],
    ["Copy pairing code", "12345-ABCDE-12345-ABCDE"],
  ])("copies only the displayed field for %s", async (label, value) => {
    render(<ComputerPairingFields locale="en" challenge={challenge()} disabled={false} onExpired={() => {}} />);
    expect(writeText).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: label }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Copied"));
    expect(writeText.mock.calls).toEqual([[value]]);
    expect(screen.queryByText("private-host-nonce")).toBeNull();
    expect(screen.queryByText("private-instance")).toBeNull();
  });
  it("keeps readonly fields manually selectable when clipboard permission is denied", async () => {
    writeText.mockRejectedValueOnce(new Error("private exception detail"));
    render(<ComputerPairingFields locale="en" challenge={challenge()} disabled={false} onExpired={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy pairing code" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Could not copy."));
    expect(screen.queryByText(/private exception detail/)).toBeNull();
    const field = screen.getByRole("textbox", { name: "Pairing code" }) as HTMLInputElement;
    expect(field).toHaveAttribute("readonly");
    fireEvent.focus(field);
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, field.value.length]);
    expect(screen.getByRole("button", { name: "Copy pairing code" })).toBeEnabled();
  });
  it("provides manual fallback when the clipboard API is unavailable", async () => {
    Reflect.deleteProperty(navigator, "clipboard");
    render(<ComputerPairingFields locale="en" challenge={challenge()} disabled={false} onExpired={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy App address" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("copy the field manually"));
    expect(writeText).not.toHaveBeenCalled();
  });
  it("does not dispatch for disabled controls", () => {
    render(<ComputerPairingFields locale="en" challenge={challenge()} disabled onExpired={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy pairing code" }));
    expect(writeText).not.toHaveBeenCalled();
  });
  it("checks the actual deadline before a click, independently of the countdown", () => {
    vi.useFakeTimers();
    const expired = vi.fn();
    render(<ComputerPairingFields locale="en" challenge={challenge()} disabled={false} onExpired={expired} />);
    vi.setSystemTime(Date.now() + 6000);
    fireEvent.click(screen.getByRole("button", { name: "Copy pairing code" }));
    expect(writeText).not.toHaveBeenCalled();
    expect(expired).toHaveBeenCalledTimes(1);
  });
  it("deduplicates pending clicks without blocking manual selection", async () => {
    let resolve!: () => void;
    writeText.mockReturnValueOnce(new Promise(done => { resolve = done; }));
    render(<ComputerPairingFields locale="en" challenge={challenge()} disabled={false} onExpired={() => {}} />);
    const copy = screen.getByRole("button", { name: "Copy pairing code" });
    fireEvent.click(copy);
    fireEvent.click(copy);
    fireEvent.click(screen.getByRole("button", { name: "Copy App address" }));
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("textbox", { name: "Pairing code" })).toBeEnabled();
    await act(async () => resolve());
    expect(copy).toBeEnabled();
  });
  it("does not announce late success after expiration", async () => {
    vi.useFakeTimers();
    let resolve!: () => void;
    writeText.mockReturnValueOnce(new Promise(done => { resolve = done; }));
    const expired = vi.fn();
    render(<ComputerPairingFields locale="en" challenge={challenge()} disabled={false} onExpired={expired} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy pairing code" }));
    vi.setSystemTime(Date.now() + 6000);
    await act(async () => resolve());
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(expired).toHaveBeenCalledTimes(1);
  });
  it.each(["success", "failure"])("ignores old %s after replacing a confirmed challenge", async outcome => {
    let resolve!: () => void;
    let reject!: (error: Error) => void;
    writeText.mockReturnValueOnce(new Promise((done, fail) => { resolve = done; reject = fail; }));
    const old = challenge();
    const expired = vi.fn();
    const view = render(<ComputerPairingFields key={old.nonce} locale="en" challenge={old} disabled={false} onExpired={expired} />);
    fireEvent.click(screen.getByRole("button", { name: "Copy pairing code" }));
    const next = { ...challenge(), nonce: "replacement", verificationCode: "NEW-CODE" };
    view.rerender(<ComputerPairingFields key={next.nonce} locale="en" challenge={next} disabled={false} onExpired={expired} />);
    await act(async () => { if (outcome === "success") resolve(); else reject(new Error("late old failure")); });
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    expect(screen.getByRole("textbox", { name: "Pairing code" })).toHaveValue("NEW-CODE");
    expect(screen.getByRole("button", { name: "Copy pairing code" })).toBeEnabled();
    expect(expired).not.toHaveBeenCalled();
  });
});
