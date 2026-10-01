import { assertObservationIdentity } from "./observation-state.mjs";
import { WORKER_COMPLETION_UNKNOWN, workerException } from "./worker-errors.mjs";

export function dragDestination(body) {
  for (const key of ["toSelector", "selector", "to", "drop"]) {
    if (Object.hasOwn(body, key)) {
      throw workerException(400, "selector_forbidden", "drag requires an observed destination");
    }
  }
  const refs = ["destElementRef", "dropElementRef"].filter(key => Object.hasOwn(body, key));
  const pixels = ["toX", "toY"].filter(key => Object.hasOwn(body, key));
  if (Object.hasOwn(body, "x1") || Object.hasOwn(body, "y1") ||
      refs.length > 1 || (refs.length && pixels.length)) {
    throw workerException(400, "ambiguous_drag_destination", "use exactly one drag destination");
  }
  if (refs.length) {
    const ref = body[refs[0]];
    if (typeof ref !== "string" || !ref.trim()) {
      throw workerException(400, "drag_destination_required", "destination elementRef required");
    }
    return { elementRef: ref };
  }
  if (pixels.length !== 2 || !Number.isFinite(body.toX) || !Number.isFinite(body.toY) ||
      body.toX < 0 || body.toY < 0) {
    throw workerException(400, "drag_destination_required", "finite nonnegative toX and toY required");
  }
  return { x: body.toX, y: body.toY };
}

export function observedDragPoint(observation, body, point) {
  const { image } = assertObservationIdentity(observation, body);
  if (!image) {
    throw workerException(400, "visual_observation_required", "pixel destination requires a visual observation");
  }
  if (point.x >= image.width || point.y >= image.height) {
    throw workerException(400, "drag_destination_out_of_bounds", "destination is outside the observed image");
  }
  return { ...point, width: image.width, height: image.height };
}

function center(box) {
  if (!box || ![box.x, box.y, box.width, box.height].every(Number.isFinite) ||
      box.width <= 0 || box.height <= 0) {
    throw workerException(409, "drag_target_invalid", "drag target has no visible geometry");
  }
  return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
}

// The original Page and ElementHandles are retained throughout. No selectors,
// active-page lookup, global mouse release or high-level drag auto-retry.
export async function dragObservedTargets({ page, source, destination, point, signal,
  checkIdentity, checkTargets }) {
  let inputStarted = false;
  let buttonMayBeDown = false;
  function check() {
    if (signal?.aborted) throw new Error("cancelled");
    checkIdentity();
    if (page.isClosed()) throw new Error("original page closed");
    if (point) {
      const viewport = page.viewportSize();
      if (!viewport || viewport.width !== point.width || viewport.height !== point.height) {
        throw workerException(409, "drag_viewport_changed", "observe the resized viewport again");
      }
    }
  }
  try {
    check();
    await checkTargets();
    const from = center(await source.boundingBox());
    const to = point ?? center(await destination.boundingBox());
    const viewport = page.viewportSize();
    if (!viewport || [from, to].some(p => p.x < 0 || p.y < 0 ||
        p.x >= viewport.width || p.y >= viewport.height)) {
      throw workerException(409, "drag_target_invalid", "drag endpoints must be in the current viewport");
    }
    check();
    inputStarted = true;
    await page.mouse.move(from.x, from.y);
    check();
    await checkTargets();
    check();
    // Set before awaiting: a lost response does not prove down never happened.
    buttonMayBeDown = true;
    await page.mouse.down({ button: "left" });
    for (let step = 1; step <= 8; step += 1) {
      check();
      await checkTargets();
      check();
      await page.mouse.move(from.x + (to.x - from.x) * step / 8,
        from.y + (to.y - from.y) * step / 8);
    }
    await checkTargets();
    if (destination) {
      const current = center(await destination.boundingBox());
      if (current.x !== to.x || current.y !== to.y) throw new Error("drag destination moved");
    }
    check();
    await page.mouse.up({ button: "left" });
    buttonMayBeDown = false;
    check();
  } catch (error) {
    if (buttonMayBeDown && !page.isClosed()) {
      // An up at the destination could commit a cancelled drop, or hit a new
      // document. Destroy only this owned managed page instead; await reality.
      try {
        await page.close({ runBeforeUnload: false });
        if (!page.isClosed()) throw new Error("original page did not close");
      } catch {
        throw workerException(409, "input_cleanup_pending", "owned drag page cleanup is unconfirmed",
          WORKER_COMPLETION_UNKNOWN);
      }
    }
    if (inputStarted) {
      throw workerException(409, "drag_outcome_unknown", "drag did not complete; do not replay",
        WORKER_COMPLETION_UNKNOWN);
    }
    throw error;
  }
}
