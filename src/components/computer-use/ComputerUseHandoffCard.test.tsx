/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { ComputerUseHandoffSession } from "./ComputerUseHandoffCard";
import {
  handoffContinuation,
  type HandoffCard,
  type HandoffClaim,
  type HandoffEnd,
  type HandoffFollow,
  type HandoffPhase,
} from "@/lib/computer-use/confirmState";

afterEach(() => {
  cleanup();
});

function sample(over: Partial<HandoffCard> = {}): HandoffCard {
  return {
    requestId: 4,
    sessionId: "sess",
    enginePid: 41,
    engineStarted: 9001,
    policyVersion: 1,
    image: {
      platform: "linux",
      key: "linux:editor",
      teamId: null,
      publisher: "Desk",
      sha256Hex: null,
      signature: "unknown",
      imagePath: "/usr/bin/editor",
      pid: 77,
    },
    name: "Editor",
    code: "login",
    detail: "",
    openedMs: 1_500,
    hasIcon: false,
    ...over,
  };
}

function Host(props: {
  card: HandoffCard;
  sessionId?: string;
  transport: (claim: HandoffClaim) => Promise<HandoffEnd>;
  onContinue?: (follow: HandoffFollow) => Promise<void>;
}) {
  const [phases, setPhases] = useState<Record<number, HandoffPhase>>({});
  return (
    <ComputerUseHandoffSession
      cards={[props.card]}
      phases={phases}
      activeSessionId={props.sessionId ?? props.card.sessionId}
      locale="en"
      transport={props.transport}
      onPhase={(requestId, phase) => setPhases((current) => ({ ...current, [requestId]: phase }))}
      onContinue={props.onContinue ?? (async () => {})}
    />
  );
}

describe("ComputerUseHandoffCard", () => {
  it("asks the user to take over and does not show a countdown", () => {
    render(<Host card={sample()} transport={async () => "handled"} />);
    expect(screen.getByRole("button", { name: "I've handled it, continue" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "I've handled it, continue" })).not.toHaveFocus();
    expect(screen.queryByText(/Auto-deny/)).not.toBeInTheDocument();
    expect(screen.getByText(/does not approve the action/)).toBeInTheDocument();
    expect(screen.getByText(/Sign in to/)).toBeInTheDocument();
  });

  it("sends a new message only after the handoff resolves and does not replay or clear stop", async () => {
    const follow = handoffContinuation("unused");
    expect(follow.clearStop).toBe(false);
    expect(follow.replay).toBe(false);
    let resolve: ((end: HandoffEnd) => void) | undefined;
    const transport = vi.fn(
      (_claim: HandoffClaim) =>
        new Promise<HandoffEnd>((done) => {
          resolve = done;
        }),
    );
    const onContinue = vi.fn(async (_follow: HandoffFollow) => {});
    const clearStop = vi.fn();
    render(<Host card={sample({ name: "Edit\u202Aor" })} transport={transport} onContinue={onContinue} />);
    fireEvent.click(screen.getByRole("button", { name: "I've handled it, continue" }));
    fireEvent.click(screen.getByRole("button", { name: "I've handled it, continue" }));
    expect(transport).toHaveBeenCalledTimes(1);
    expect(transport.mock.calls[0]?.[0].act).toBe("handled");
    expect(onContinue).not.toHaveBeenCalled();
    expect(clearStop).not.toHaveBeenCalled();
    await act(async () => {
      resolve?.("handled");
    });
    expect(onContinue).toHaveBeenCalledTimes(1);
    const sent = onContinue.mock.calls[0]?.[0];
    expect(sent?.clearStop).toBe(false);
    expect(sent?.replay).toBe(false);
    expect(sent?.text).toContain("Edit\uFFFDor");
    expect(sent?.text).toContain("do not repeat the last action");
    expect(sent?.text).not.toContain("\u202A");
    expect(screen.getByRole("button", { name: "I've handled it, continue" })).toBeDisabled();
  });

  it("does not send a message when the desk rejects the click", async () => {
    const onContinue = vi.fn(async () => {});
    const transport = vi.fn(async () => {
      throw new Error("failed");
    });
    render(<Host card={sample()} transport={transport} onContinue={onContinue} />);
    fireEvent.click(screen.getByRole("button", { name: "I've handled it, continue" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not record the decision.");
    expect(onContinue).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "I've handled it, continue" })).toBeEnabled();
  });

  it("does not send a message when the session already closed", async () => {
    const onContinue = vi.fn(async () => {});
    render(<Host card={sample()} transport={async () => "closed"} onContinue={onContinue} />);
    fireEvent.click(screen.getByRole("button", { name: "I've handled it, continue" }));
    expect(await screen.findByRole("status")).toHaveTextContent("The session closed.");
    expect(onContinue).not.toHaveBeenCalled();
  });

  it("leaves chat Enter on the composer and survives a session switch", () => {
    const transport = vi.fn(async () => "handled" as const);
    const card = sample({ sessionId: "alpha" });
    const { rerender } = render(
      <>
        <textarea data-testid="chat" />
        <Host card={card} transport={transport} />
      </>,
    );
    const chat = screen.getByTestId("chat");
    chat.focus();
    fireEvent.keyDown(chat, { key: "Enter" });
    expect(transport).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(chat);
    rerender(
      <>
        <textarea data-testid="chat" />
        <Host card={card} sessionId="beta" transport={transport} />
      </>,
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    rerender(
      <>
        <textarea data-testid="chat" />
        <Host card={card} sessionId="alpha" transport={transport} />
      </>,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("shows other engine detail as plain text, not model-provided", () => {
    render(
      <Host
        card={sample({ code: "other", detail: "go\u202Ewest" })}
        transport={async () => "handled"}
      />,
    );
    const detail = screen.getByTestId("cu-handoff-detail");
    expect(detail.textContent).toContain("Details");
    expect(detail.textContent).toContain("go\uFFFDwest");
    expect(detail.textContent).not.toContain("Model-provided");
    expect(detail.textContent).not.toContain("\u202E");
  });
});
