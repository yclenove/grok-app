// Fixed private owned-fixture expectations, never a production action dispatcher.
import assert from "node:assert/strict";

const text = "你好，世界🙂";
const specs = Object.freeze({
  click: { action: "click", name: "Fixture action", parameters: {},
    status: "applied", detail: "semantic_click_dispatched" },
  "set-value": { action: "set_value", name: "Fixture input", parameters: { text },
    status: "verified", detail: "field_value_changed" },
  "type-text": { action: "type_text", name: "Fixture input", parameters: { text },
    status: "verified", detail: "field_value_changed" },
  scroll: { action: "scroll", name: "Fixture scroll", parameters: { delta: 60 },
    status: "verified", detail: "scroll_position_changed" },
  wait: { action: "wait", name: "Fixture action", parameters: { nameEquals: "Fixture action" },
    status: "verified", detail: "condition_matched" },
});
const suffixes = Object.freeze(["pre-pause", "post-pause", "pre-chat-stop", "post-chat-stop"]);

export const lateActionScenarios = Object.freeze(Object.keys(specs).flatMap(action =>
  suffixes.map(suffix => `active-late-${action}-${suffix}`)));

export function lateActionSpec(scenario) {
  assert(lateActionScenarios.includes(scenario), "unsupported private late action scenario");
  const [slug, spec] = Object.entries(specs).find(([slug]) => scenario.startsWith(`active-late-${slug}-`));
  const suffix = scenario.slice(`active-late-${slug}-`.length);
  return { ...spec, post: suffix.startsWith("post-"),
    interruption: suffix.endsWith("chat-stop") ? "chat-stop" : "pause" };
}

export async function originalPageEffects(page) {
  return page.evaluate(() => ({ clicks: globalThis.clicks,
    text: document.querySelector("#input").value,
    scrollTop: document.querySelector("#scroll").scrollTop,
    textEvents: { ...globalThis.textEvents } }));
}

export function expectedPageEffects(spec, applied) {
  const textAction = applied && ["set_value", "type_text"].includes(spec.action);
  return { clicks: Number(applied && spec.action === "click"),
    text: textAction ? spec.parameters.text : "",
    scrollTop: applied && spec.action === "scroll" ? spec.parameters.delta : 0,
    textEvents: { beforeinput: Number(textAction), input: Number(textAction),
      change: Number(applied && spec.action === "set_value"),
      inputType: textAction ? spec.action === "set_value" ? "insertReplacementText" : "insertText" : null } };
}
