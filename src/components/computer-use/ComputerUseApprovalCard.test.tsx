/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import {
  ComputerUseApprovalSession,
  type ApprovalHandle,
} from "./ComputerUseApprovalCard";
import {
  applySettled,
  bindCard,
  cardsForSession,
  claimFor,
  emptyDesk,
  type Card,
  type Claim,
} from "@/lib/computer-use/approvalDesk";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

function sample(over: Partial<Card> = {}): Card {
  return {
    requestId: 7,
    sessionId: "sess",
    enginePid: 41,
    engineStarted: 9001,
    policyVersion: 1,
    image: {
      platform: "linux",
      key: "linux:editor",
      teamId: null,
      publisher: "Desk",
      sha256Hex: "ab".repeat(32),
      signature: "valid",
      imagePath: "/usr/bin/editor",
      pid: 77,
    },
    name: "Editor",
    reason: "open the file",
    risk: "low",
    timeoutSecs: 120,
    openedMs: Date.now(),
    hasIcon: false,
    alwaysEnabled: true,
    ...over,
  };
}

function renderCard(card: Card, transport: (claim: Claim) => Promise<void> = async () => {}) {
  const ref = createRef<ApprovalHandle>();
  const view = render(
    <ComputerUseApprovalSession
      ref={ref}
      cards={[card]}
      activeSessionId={card.sessionId}
      locale="en"
      transport={transport}
    />,
  );
  return { ...view, ref, transport };
}

describe("ComputerUseApprovalCard", () => {
  it("offers deny then turn, session, and always", () => {
    renderCard(sample());
    const names = screen.getAllByRole("button").map((button) => button.textContent);
    expect(names).toEqual(["Deny", "This turn", "This session", "Always allow"]);
    expect(screen.getByRole("button", { name: "Deny" })).not.toHaveFocus();
    expect(screen.getByRole("dialog")).toHaveAttribute("aria-modal", "false");
  });

  it.each([
    ["Deny", "deny"],
    ["This turn", "turn"],
    ["This session", "session"],
    ["Always allow", "always"],
  ] as const)("sends %s as %s", (label, act) => {
    const transport = vi.fn(async (_claim: Claim) => {});
    renderCard(sample(), transport);
    fireEvent.click(screen.getByRole("button", { name: label }));
    expect(transport).toHaveBeenCalledTimes(1);
    expect(transport.mock.calls[0]?.[0].act).toBe(act);
    expect(transport.mock.calls[0]?.[0].image.imagePath).toBe("/usr/bin/editor");
  });

  it("hides always when persistent grants are off", () => {
    renderCard(sample({ alwaysEnabled: false }));
    expect(screen.queryByRole("button", { name: "Always allow" })).not.toBeInTheDocument();
  });

  it("does not reset the countdown after leaving and returning", () => {
    vi.useFakeTimers();
    const started = 1_000_000;
    vi.setSystemTime(started);
    const transport = vi.fn(async (_claim: Claim) => {});
    const card = sample({ openedMs: started, timeoutSecs: 5, sessionId: "alpha" });
    const { rerender } = render(
      <ComputerUseApprovalSession
        cards={[card]}
        activeSessionId="alpha"
        locale="en"
        transport={transport}
      />,
    );
    expect(screen.getByText("Auto-deny in 5s")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.getByText("Auto-deny in 3s")).toBeInTheDocument();
    rerender(
      <ComputerUseApprovalSession
        cards={[card]}
        activeSessionId="beta"
        locale="en"
        transport={transport}
      />,
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(transport).not.toHaveBeenCalled();
    rerender(
      <ComputerUseApprovalSession
        cards={[card]}
        activeSessionId="alpha"
        locale="en"
        transport={transport}
      />,
    );
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("Auto-deny in 1s")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(transport).toHaveBeenCalledTimes(1);
    expect(transport.mock.calls[0]?.[0].act).toBe("timeout");
  });

  it("keeps a pending card on the desk while another session is open", () => {
    let desk = bindCard(emptyDesk(), sample({ sessionId: "alpha" }));
    desk = bindCard(desk, sample({ requestId: 8, sessionId: "beta" }));
    expect(cardsForSession(desk, "beta").map((card) => card.requestId)).toEqual([8]);
    expect(cardsForSession(desk, "alpha")).toHaveLength(1);
    desk = applySettled(desk, 8);
    expect(cardsForSession(desk, "alpha")).toHaveLength(1);
    expect(cardsForSession(desk, "beta")).toHaveLength(0);
    desk = bindCard(desk, sample({ requestId: 8, sessionId: "beta" }));
    expect(cardsForSession(desk, "beta")).toHaveLength(0);
  });

  it("shows an alert for a forged image and does not call the transport", async () => {
    const transport = vi.fn(async (_claim: Claim) => {});
    const card = sample();
    const { ref } = renderCard(card, transport);
    const forged = {
      ...claimFor(card, "turn"),
      image: { ...card.image, imagePath: "/tmp/fake" },
    };
    await act(async () => {
      await ref.current?.submit(forged);
    });
    expect(screen.getByRole("alert")).toHaveTextContent(
      "This decision does not match the checked app.",
    );
    expect(transport).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Deny" })).toBeEnabled();
  });

  it("ignores a second decision while the first is still saving", async () => {
    let resolve: (() => void) | undefined;
    const transport = vi.fn(
      () =>
        new Promise<void>((done) => {
          resolve = done;
        }),
    );
    const card = sample();
    const { ref } = renderCard(card, transport);
    const claim = claimFor(card, "deny");
    await act(async () => {
      const first = ref.current?.submit(claim);
      const second = ref.current?.submit(claim);
      await Promise.resolve();
      expect(transport).toHaveBeenCalledTimes(1);
      resolve?.();
      await first;
      await second;
    });
    expect(screen.getByRole("button", { name: "Deny" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(transport).toHaveBeenCalledTimes(1);
  });

  it("leaves chat Enter on the composer", () => {
    const transport = vi.fn(async (_claim: Claim) => {});
    render(
      <>
        <textarea data-testid="chat" />
        <ComputerUseApprovalSession
          cards={[sample()]}
          activeSessionId="sess"
          locale="en"
          transport={transport}
        />
      </>,
    );
    const chat = screen.getByTestId("chat");
    chat.focus();
    fireEvent.keyDown(chat, { key: "Enter" });
    expect(document.activeElement).toBe(chat);
    expect(transport).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Deny" })).not.toHaveFocus();
  });

  it("denies on escape only while the card is mounted", () => {
    const transport = vi.fn(async (_claim: Claim) => {});
    const card = sample();
    const { rerender } = render(
      <ComputerUseApprovalSession
        cards={[card]}
        activeSessionId="sess"
        locale="en"
        transport={transport}
      />,
    );
    fireEvent.keyDown(document, { key: "Escape" });
    expect(transport.mock.calls[0]?.[0].act).toBe("deny");
    rerender(
      <ComputerUseApprovalSession
        cards={[card]}
        activeSessionId="other"
        locale="en"
        transport={transport}
      />,
    );
    fireEvent.click(document.body);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(transport).toHaveBeenCalledTimes(1);
  });

  it("renders the model reason as plain text with the model-provided label", () => {
    renderCard(sample({ reason: "go\u202Ewest", hasIcon: true, name: "Edit\u202Aor" }));
    const reason = screen.getByTestId("cu-reason");
    expect(reason.textContent).toContain("Model-provided");
    expect(reason.textContent).toContain("go\uFFFDwest");
    expect(reason.textContent).not.toContain("\u202E");
    expect(reason.querySelector("script")).toBeNull();
    expect(
      screen.getByRole("heading", { name: "Checked by the operating system" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Name, not verified/)).toHaveTextContent("Edit\uFFFDor");
    expect(screen.getByText("Icon, not verified")).toBeInTheDocument();
  });

  it("renders nothing when this session has no card", () => {
    const { container } = render(
      <ComputerUseApprovalSession
        cards={[sample()]}
        activeSessionId="other"
        locale="en"
        transport={async () => {}}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
