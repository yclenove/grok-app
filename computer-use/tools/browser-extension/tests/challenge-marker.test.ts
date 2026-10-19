// @vitest-environment jsdom
import { afterEach, describe, expect, it } from "vitest";
import {
  createPages, installRelay, marker, matches, pageVisibleShim, privateActionShim,
  readCarrier, writeCarrier, type Carrier, type Transport,
} from "./fixtures/challenge-marker";
import { ContentSession } from "../content";

// Public, deterministic fixture bytes. This is not a production challenge generator.
const nonce = "0123456789abcdef".repeat(4);
const cleanup: Array<() => void> = [];
afterEach(() => { for (const close of cleanup.splice(0).reverse()) close(); });
function pages() {
  const fixture = createPages(document);
  cleanup.push(fixture.close);
  return fixture;
}

describe("ADR 0014 M0 page-carrier counterexamples (not native AX evidence)", () => {
  const cases = (["C1", "C2"] as Carrier[]).flatMap(carrier =>
    (["postMessage", "same_origin_reference"] as Transport[]).map(transport => ({ carrier, transport })));

  it.each(cases)("page_relays_marker_unique_wrong_window_$carrier_$transport", async ({ carrier, transport }) => {
    const f = pages();
    expect(matches(f, carrier, nonce)).toEqual([]);
    const relay = installRelay(f, carrier, transport);
    cleanup.push(relay.close);
    // Only A receives the challenge. The attacker obtains it through page APIs.
    writeCarrier(f.A.document, carrier, marker(nonce));
    await relay.delivered;
    expect(relay.reads).toEqual([marker(nonce)]);
    expect(readCarrier(f.A.document, carrier)).toBe("Continue");
    for (let sample = 0; sample < 3; sample++) {
      expect(matches(f, carrier, nonce)).toEqual(["B"]);
      await Promise.resolve();
    }
    // Independent document identity, not a copied title/URL or marker-derived ID.
    const policyCandidate = matches(f, carrier, nonce)[0];
    expect(f.identity.get(f.sharedDocument)).toBe("A");
    expect(policyCandidate).toBe("B");
    expect(policyCandidate).not.toBe(f.identity.get(f.sharedDocument));
    expect(f.actions).toEqual({ A: 0, B: 0 });
  });

  it.each(["C1", "C2"] as Carrier[])("candidate_oracle_%s_distinguishes_original_duplicate_and_absent", carrier => {
    const f = pages();
    writeCarrier(f.A.document, carrier, marker(nonce));
    expect(matches(f, carrier, nonce)).toEqual(["A"]);
    writeCarrier(f.B.document, carrier, marker(nonce));
    expect(matches(f, carrier, nonce)).toEqual(["A", "B"]);
    writeCarrier(f.A.document, carrier, "");
    writeCarrier(f.B.document, carrier, "");
    expect(matches(f, carrier, nonce)).toEqual([]);
  });
});

describe("C4 test-only private action boundary (not a Chrome implementation)", () => {
  it("c4_nonce_stays_out_of_page_and_content_visible_objects", async () => {
    const f = pages();
    const action = privateActionShim();
    const worlds = [pageVisibleShim(f.A), pageVisibleShim(f.B)];
    worlds.forEach(world => cleanup.push(world.close));
    const content = new ContentSession(f.A);
    cleanup.push(() => content.revoke());
    const activation = { kind: "activate", shareId: "fixture-share", url: f.A.document.URL };
    worlds[0].contentIngress(activation);
    content.handle(activation);
    expect(worlds.every(world => world.leaks(nonce).length === 0)).toBe(true);

    // A browser-owned action state is deliberately NOT installed on either Window.
    await action.setTitle({ tabId: 1, title: `Stop sharing this tab${marker(nonce)}` });
    expect(await action.getTitle({ tabId: 1 })).toContain(nonce);
    expect(await action.getTitle({ tabId: 2 })).not.toContain(nonce);
    await Promise.resolve();
    for (const world of worlds) {
      expect("chrome" in world.api).toBe(false);
      expect(world.leaks(nonce)).toEqual([]);
    }
    expect(matches(f, "C1", nonce)).toEqual([]);
    expect(matches(f, "C2", nonce)).toEqual([]);
    await action.setTitle({ tabId: 1, title: "Stop sharing this tab" });
    expect(await action.getTitle({ tabId: 1 })).not.toContain(nonce);
    expect(worlds.every(world => world.leaks(nonce).length === 0)).toBe(true);
  });

  it.each(["title", "aria_label", "page_message", "content_message"])("visibility_oracle_%s_detects_deliberate_nonce_leak", async boundary => {
    const f = pages();
    const world = pageVisibleShim(f.A);
    cleanup.push(world.close);
    expect(world.leaks(nonce)).toEqual([]);
    if (boundary === "title") f.A.document.title = marker(nonce);
    if (boundary === "aria_label") f.A.document.body.setAttribute("aria-label", marker(nonce));
    if (boundary === "page_message") await world.postAndWait({ nonce });
    if (boundary === "content_message") world.contentIngress({ kind: "activate", nonce });
    await Promise.resolve();
    expect(world.leaks(nonce).length).toBeGreaterThan(0);
    // Removing a DOM leak cannot erase an observer's already captured copy.
    f.A.document.title = "Continue";
    f.A.document.body.removeAttribute("aria-label");
    expect(world.leaks(nonce).length).toBeGreaterThan(0);
  });
});
