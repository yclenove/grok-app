/** @vitest-environment jsdom */
import { describe, expect, it } from "vitest";
import {
  importedHistoryHash,
  openImportedHistory,
  parseImportedDeepLink,
  rememberImportedHistoryAnchor,
} from "./open";

describe("parseImportedDeepLink", () => {
  it("opens revision and leaf from the imported hash", () => {
    expect(
      parseImportedDeepLink(
        "#/imported/imp:00000000-0000-4000-8000-000000000001/rev/rev-a/leaf/a3",
      ),
    ).toEqual({
      importId: "imp:00000000-0000-4000-8000-000000000001",
      revision: "rev-a",
      leaf: "a3",
    });
  });

  it("round trips an encoded event anchor without treating it as authorization", () => {
    const detail = {
      importId: "imp:00000000-0000-4000-8000-000000000001",
      revision: "rev-a",
      leaf: "a3",
      anchorEventId: "event/with spaces",
    };
    const hash = importedHistoryHash(detail);
    expect(hash).toContain("/event/event%2Fwith%20spaces");
    expect(parseImportedDeepLink(hash)).toEqual(detail);
  });

  it("rejects native session hashes", () => {
    expect(parseImportedDeepLink("#/session/abc")).toBeNull();
  });

  it("fails closed for malformed percent encoding", () => {
    expect(() =>
      parseImportedDeepLink("#/imported/imp:00000000-0000-4000-8000-000000000001/event/%E0%A4%A"),
    ).not.toThrow();
    expect(
      parseImportedDeepLink("#/imported/imp:00000000-0000-4000-8000-000000000001/event/%E0%A4%A"),
    ).toBeNull();
  });
});

describe("rememberImportedHistoryAnchor", () => {
  it("adds a back-forward location without reopening the reader", () => {
    window.history.replaceState({}, "", "/");
    const seen: unknown[] = [];
    const onOpen = (event: Event) => seen.push((event as CustomEvent).detail);
    window.addEventListener("grok:open-imported-history", onOpen);
    rememberImportedHistoryAnchor({
      importId: "imp:00000000-0000-4000-8000-000000000001",
      revision: "rev-a",
      leaf: "a3",
      anchorEventId: "e5000",
    });
    expect(window.location.hash).toContain("/event/e5000");
    expect(seen).toEqual([]);
    window.removeEventListener("grok:open-imported-history", onOpen);
  });
});

describe("openImportedHistory", () => {
  it("writes a copyable hash and dispatches the imported event", () => {
    const pushed: string[] = [];
    const original = window.history.pushState.bind(window.history);
    window.history.pushState = ((data: unknown, unused: string, url?: string | URL | null) => {
      pushed.push(String(url ?? ""));
      return original(data, unused, url);
    }) as History["pushState"];
    const seen: unknown[] = [];
    const onOpen = (ev: Event) => {
      seen.push((ev as CustomEvent).detail);
    };
    window.addEventListener("grok:open-imported-history", onOpen);
    const detail = {
      importId: "imp:00000000-0000-4000-8000-000000000001",
      revision: "rev-a",
      leaf: "a3",
    };
    openImportedHistory(detail);
    expect(importedHistoryHash(detail)).toBe(
      "#/imported/imp%3A00000000-0000-4000-8000-000000000001/rev/rev-a/leaf/a3",
    );
    expect(pushed.some((url) => url.includes("/imported/imp%3A00000000-0000-4000-8000-000000000001"))).toBe(
      true,
    );
    expect(seen).toEqual([detail]);
    openImportedHistory(detail);
    expect(pushed.filter((url) => url.includes("/imported/")).length).toBe(1);
    window.removeEventListener("grok:open-imported-history", onOpen);
    window.history.pushState = original;
  });
});
