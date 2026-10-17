/**
 * @vitest-environment jsdom
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { ComputerUseActionConfirmSession } from "./ComputerUseActionConfirmCard";
import {
  type ConfirmCard,
  type ConfirmClaim,
  type ConfirmEnd,
  type ConfirmPhase,
} from "@/lib/computer-use/confirmState";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const png = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 9];

function sample(over: Partial<ConfirmCard> = {}): ConfirmCard {
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
    summary: "Click Run",
    reason: "",
    snapshotId: "snap-1",
    actionDigest: "ab".repeat(32),
    crop: { kind: "missing" },
    timeoutSecs: 120,
    openedMs: Date.now(),
    hasIcon: false,
    ...over,
  };
}

function Host(props: {
  card: ConfirmCard;
  sessionId?: string;
  transport: (claim: ConfirmClaim) => Promise<ConfirmEnd>;
}) {
  const [phases, setPhases] = useState<Record<number, ConfirmPhase>>({});
  return (
    <ComputerUseActionConfirmSession
      cards={[props.card]}
      phases={phases}
      activeSessionId={props.sessionId ?? props.card.sessionId}
      locale="en"
      transport={props.transport}
      onPhase={(requestId, phase) => setPhases((current) => ({ ...current, [requestId]: phase }))}
    />
  );
}

describe("ComputerUseActionConfirmCard", () => {
  it("offers deny before allow and does not focus deny", () => {
    render(<Host card={sample()} transport={async () => "denied"} />);
    const names = screen.getAllByRole("button").map((button) => button.textContent);
    expect(names).toEqual(["Deny", "Allow action"]);
    expect(screen.getByRole("button", { name: "Deny" })).not.toHaveFocus();
    expect(screen.getByRole("dialog")).toHaveAttribute("aria-modal", "false");
    expect(screen.getByText(/Deny is the default/)).toBeInTheDocument();
  });

  it("sends deny by default and disables a second submit", async () => {
    let resolve: ((end: ConfirmEnd) => void) | undefined;
    const transport = vi.fn(
      (_claim: ConfirmClaim) =>
        new Promise<ConfirmEnd>((done) => {
          resolve = done;
        }),
    );
    render(<Host card={sample()} transport={transport} />);
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    fireEvent.click(screen.getByRole("button", { name: "Allow action" }));
    expect(transport).toHaveBeenCalledTimes(1);
    expect(transport.mock.calls[0]?.[0].act).toBe("deny");
    expect(transport.mock.calls[0]?.[0].snapshotId).toBe("snap-1");
    expect(screen.getByRole("button", { name: "Deny" })).toBeDisabled();
    await act(async () => {
      resolve?.("denied");
    });
    expect(screen.getByRole("status")).toHaveTextContent("Denied. Nothing was run.");
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(transport).toHaveBeenCalledTimes(1);
  });

  it("shows allowed only when the desk says the binding still matches", async () => {
    const transport = vi.fn(async () => "stale" as const);
    render(<Host card={sample()} transport={transport} />);
    fireEvent.click(screen.getByRole("button", { name: "Allow action" }));
    expect(await screen.findByRole("status")).toHaveTextContent("This confirmation is void.");
    expect(screen.queryByText("Allowed. The action is checked again before it runs.")).not.toBeInTheDocument();
  });

  it("does not reset the countdown after leaving and returning", () => {
    vi.useFakeTimers();
    const started = 1_000_000;
    vi.setSystemTime(started);
    const transport = vi.fn(async (_claim: ConfirmClaim) => "timed_out" as const);
    const card = sample({ openedMs: started, timeoutSecs: 5, sessionId: "alpha" });
    const { rerender } = render(<Host card={card} transport={transport} />);
    expect(screen.getByText("Auto-deny in 5s")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.getByText("Auto-deny in 3s")).toBeInTheDocument();
    rerender(<Host card={card} sessionId="beta" transport={transport} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(transport).not.toHaveBeenCalled();
    rerender(<Host card={card} sessionId="alpha" transport={transport} />);
    expect(screen.getByText("Auto-deny in 1s")).toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(transport).toHaveBeenCalledTimes(1);
    expect(transport.mock.calls[0]?.[0].act).toBe("timeout");
  });

  it("leaves chat Enter on the composer and denies on escape only while mounted", () => {
    const transport = vi.fn(async (_claim: ConfirmClaim) => "denied" as const);
    const { rerender } = render(
      <>
        <textarea data-testid="chat" />
        <Host card={sample()} transport={transport} />
      </>,
    );
    const chat = screen.getByTestId("chat");
    chat.focus();
    fireEvent.keyDown(chat, { key: "Enter" });
    expect(document.activeElement).toBe(chat);
    expect(transport).not.toHaveBeenCalled();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(transport.mock.calls[0]?.[0].act).toBe("deny");
    rerender(
      <>
        <textarea data-testid="chat" />
        <Host card={sample()} sessionId="other" transport={transport} />
      </>,
    );
    fireEvent.keyDown(document, { key: "Escape" });
    expect(transport).toHaveBeenCalledTimes(1);
  });

  it("labels engine text as the action and keeps an empty model reason plain", () => {
    render(
      <Host
        card={sample({ summary: "go\u202Ewest", name: "Edit\u202Aor" })}
        transport={async () => "denied"}
      />,
    );
    const summary = screen.getByTestId("cu-summary");
    expect(summary.textContent).toContain("Action");
    expect(summary.textContent).toContain("go\uFFFDwest");
    expect(summary.textContent).not.toContain("Model-provided");
    expect(summary.textContent).not.toContain("\u202E");
    const reason = screen.getByTestId("cu-reason");
    expect(reason.textContent).toContain("Model-provided");
    expect(reason.textContent).toContain("The model did not give a reason.");
    expect(screen.getByText(/Name, not verified|Edit/)).toBeTruthy();
  });

  it("shows a gap instead of an unredacted or unlocated crop", () => {
    const { rerender } = render(<Host card={sample()} transport={async () => "denied"} />);
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByTestId("cu-crop")).toHaveTextContent("unredacted screen is not shown");
    expect(screen.getByTestId("cu-crop").textContent).toMatch(/No redacted preview/);
    rerender(
      <Host
        card={sample({
          crop: { kind: "redacted", mime: "image/png", width: 8, height: 8, bytes: [1, 2, 3, 4, 5, 6, 7, 8] },
        })}
        transport={async () => "denied"}
      />,
    );
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByTestId("cu-crop")).toHaveTextContent("could not be located");
    expect(screen.getByTestId("cu-crop").textContent).not.toContain("1,2,3");
    rerender(
      <Host
        card={sample({
          crop: { kind: "redacted", mime: "image/png", width: 8, height: 8, bytes: png },
        })}
        transport={async () => "denied"}
      />,
    );
    const image = screen.getByRole("img");
    expect(image).toHaveAttribute("alt", "Redacted preview of the target");
    expect(image.getAttribute("src")).toMatch(/^data:image\/png;base64,/);
  });

  it("renders nothing when this session has no card", () => {
    const { container } = render(
      <Host card={sample()} sessionId="other" transport={async () => "denied"} />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});
