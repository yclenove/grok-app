/** @vitest-environment jsdom */
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { ComputerTaskCardLive } from "./ComputerTaskCardLive";

vi.mock("@/lib/api/computerUse", () => ({
  computerStatus: vi.fn(async () => ({ runId: null, featureEnabled: false })),
  computerPause: vi.fn(), computerStop: vi.fn(),
}));
vi.mock("./useComputerTask", () => ({ useComputerTask: () => ({
  state: "live", owner: {
    subscriptionId: "synthetic-subscription", sessionId: "synthetic-task-chat",
    connectionId: "synthetic-connection", hostInstanceId: "synthetic-host",
  },
  view: {
    sessionId: "synthetic-task-chat", connectionId: "synthetic-connection",
    taskId: "synthetic-task", featureEnabled: false, grants: [], executions: [],
    pickerSurfaces: ["desktop"],
    request: {
      key: { connectionId: "synthetic-connection", taskId: "synthetic-task", requestId: "synthetic-request", revision: 1 },
      phase: "pending", purpose: "Synthetic: arrange my owned editor",
      scopeIntent: "Only the selected fixture windows", surfaces: ["desktop"],
      capabilities: ["observe", "input"], remainingMs: 60_000,
    },
  },
}) }));
afterEach(cleanup);

it("shows a real Host pending request before any execution run or feature enable", () => {
  render(<ComputerTaskCardLive locale="en" sessionId="synthetic-task-chat" />);
  expect(screen.getByText("Synthetic: arrange my owned editor")).toBeInTheDocument();
  expect(screen.getByText("Only the selected fixture windows")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Choose windows for this task" })).toBeEnabled();
});
