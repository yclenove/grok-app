import type { ReactNode } from "react";
import { createPortal } from "react-dom";

/** Keep step-owned actions inside the same dialog, outside its scrollable body. */
export function SessionImportActionPortal({ target, children }: {
  target?: HTMLElement | null;
  children: ReactNode;
}) {
  return target ? createPortal(children, target) : children;
}
