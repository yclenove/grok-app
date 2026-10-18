/** @vitest-environment jsdom */
import { afterEach, describe, expect, it, vi } from "vitest";
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ComputerUseBrowser } from "./ComputerUseBrowser";
import { escapeUntrusted, openBrowserFixture } from "@/lib/computer-use/browser";

afterEach(cleanup);

describe("ComputerUseBrowser", () => {
  it("stays_closed_and_not_run_so_the_address_bar_cannot_submit", () => {
    render(<ComputerUseBrowser locale="en" />);
    expect(screen.getByTestId("cu-browser")).toHaveAttribute("data-proof", "not_run");
    expect(screen.getByTestId("cu-browser")).toHaveAttribute("data-availability", "closed");
    expect(screen.getByTestId("cu-browser-closed")).toHaveTextContent("Built-in browser is off");
    expect(screen.getByTestId("cu-browser-address")).toBeDisabled();
    expect(screen.getByTestId("cu-browser-go")).toBeDisabled();
    expect(screen.getByTestId("cu-browser-back")).toBeDisabled();
  });

  it("keeps_the_committed_origin_when_a_fixture_submit_is_blocked", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn(() => "blocked" as const);
    render(
      <ComputerUseBrowser
        locale="en"
        initial={openBrowserFixture("https://example.com")}
        onNavigate={onNavigate}
      />,
    );
    expect(screen.getByTestId("cu-browser-origin")).toHaveTextContent("https://example.com");
    const address = screen.getByTestId("cu-browser-address");
    await user.clear(address);
    await user.type(address, "http://127.0.0.1/secret");
    await user.click(screen.getByTestId("cu-browser-go"));
    expect(onNavigate).toHaveBeenCalledWith("http://127.0.0.1/secret");
    expect(screen.getByTestId("cu-browser-origin")).toHaveTextContent("https://example.com");
    expect(screen.getByTestId("cu-browser-blocked")).toHaveTextContent("That address was blocked.");
  });

  it("shows_a_page_title_as_untrusted_text_not_the_address", () => {
    const title = `Example\u202ehttps://evil.test`;
    const view = openBrowserFixture("https://example.com");
    view.pageTitle = title;
    render(<ComputerUseBrowser locale="en" initial={view} />);
    expect(screen.getByTestId("cu-browser-origin")).toHaveTextContent("https://example.com");
    expect(screen.getByTestId("cu-browser-origin")).not.toHaveTextContent("evil.test");
    expect(screen.getByTestId("cu-browser-title")).toHaveTextContent(escapeUntrusted(title));
    expect(screen.getByTestId("cu-browser-title").textContent).not.toContain("\u202e");
  });

  it("keeps_the_previous_address_when_open_fails", async () => {
    const user = userEvent.setup();
    render(
      <ComputerUseBrowser
        locale="en"
        initial={openBrowserFixture("https://example.com")}
        onNavigate={() => "failed"}
      />,
    );
    await user.click(screen.getByTestId("cu-browser-reload"));
    expect(screen.getByTestId("cu-browser-blocked")).toBeInTheDocument();
    expect(screen.getByTestId("cu-browser-origin")).toHaveTextContent("https://example.com");
    await user.click(screen.getByTestId("cu-browser-go"));
    expect(screen.getByTestId("cu-browser-failed")).toHaveTextContent("previous address is unchanged");
    expect(screen.getByTestId("cu-browser-origin")).toHaveTextContent("https://example.com");
  });
});
