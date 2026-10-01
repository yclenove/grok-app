import { clampAsideWidth, type AsideClampOpts } from "./layout";

export type AsideResizeRequest =
  | { kind: "delta"; pixels: number }
  | { kind: "minimum" }
  | { kind: "maximum" };

export type AsideResizeControl = {
  value: number;
  min: number;
  max: number;
  begin: () => void;
  change: (request: AsideResizeRequest) => void;
};

/** Move the left edge of the right-hand pane in the arrow's direction. */
export function asideResizeKey(key: string, largeStep: boolean): AsideResizeRequest | null {
  const step = largeStep ? 50 : 10;
  switch (key) {
    case "ArrowLeft": return { kind: "delta", pixels: step };
    case "ArrowRight": return { kind: "delta", pixels: -step };
    case "Home": return { kind: "minimum" };
    case "End": return { kind: "maximum" };
    default: return null;
  }
}

export function resolveAsideResize(
  current: number, request: AsideResizeRequest, opts: AsideClampOpts,
): number {
  const desired = request.kind === "minimum" ? 0
    : request.kind === "maximum" ? Number.MAX_SAFE_INTEGER
    : current + request.pixels;
  return clampAsideWidth(desired, opts);
}
