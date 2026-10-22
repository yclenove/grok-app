import { useEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { installDialogFocus, listFocusable } from "@/lib/a11yFocus";
import type { ImportStep } from "@/hooks/useSessionImport";
import type { SessionImportTFn } from "./sessionImportHostLabels";

export const SESSION_IMPORT_STEPS = [
  "source",
  "content",
  "prepare",
  "mapping",
  "import",
  "result",
] as const satisfies readonly ImportStep[];

const STEP_KEY = {
  source: "sessionImport.step.source",
  content: "sessionImport.step.content",
  prepare: "sessionImport.step.prepare",
  mapping: "sessionImport.step.mapping",
  import: "sessionImport.step.import",
  result: "sessionImport.step.result",
} as const;

function enabledItems(root: ParentNode | null | undefined): HTMLElement[] {
  return listFocusable(root).filter((el) => el.getAttribute("role") === "menuitem");
}

/**
 * Domain popup. A newer dialog scope owns Tab and Escape, so the wizard
 * trap cannot keep focus on the trigger or close itself while this menu is open.
 */
function SessionImportStepMenu({
  tr,
  edge,
  triggerRef,
  portalTheme,
  busy,
  stepOpenable,
  onGo,
  onClose,
}: {
  tr: SessionImportTFn;
  edge: "first" | "last";
  triggerRef: RefObject<HTMLButtonElement | null>;
  portalTheme?: string;
  busy: boolean;
  stepOpenable: (step: ImportStep) => boolean;
  onGo: (step: ImportStep) => void;
  onClose: () => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const onGoRef = useRef(onGo);
  onGoRef.current = onGo;

  useEffect(() => {
    return installDialogFocus(() => menuRef.current, {
      initialFocus: () => {
        const items = enabledItems(menuRef.current);
        return edge === "last" ? items[items.length - 1] : items[0];
      },
      onEscape: () => onCloseRef.current(),
      restoreFocus: () => {
        const button = triggerRef.current;
        if (button && button.isConnected && !button.disabled) {
          button.focus();
          return false;
        }
        return true;
      },
    });
  }, [edge, triggerRef]);

  useEffect(() => {
    const trigger = triggerRef.current;
    const onDoc = (event: MouseEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (menuRef.current?.contains(target) || trigger?.contains(target)) return;
      onCloseRef.current();
    };
    const timer = window.setTimeout(() => {
      document.addEventListener("mousedown", onDoc, true);
    }, 0);
    return () => {
      window.clearTimeout(timer);
      document.removeEventListener("mousedown", onDoc, true);
    };
  }, [triggerRef]);

  useEffect(() => {
    const place = () => {
      const trigger = triggerRef.current;
      const menu = menuRef.current;
      if (!trigger || !menu) return;
      const box = trigger.getBoundingClientRect();
      const width = menu.offsetWidth || 0;
      const height = menu.offsetHeight || 0;
      const left = Math.max(8, Math.min(box.left, window.innerWidth - width - 8));
      const top = Math.max(8, Math.min(box.bottom, window.innerHeight - height - 8));
      menu.style.left = `${left}px`;
      menu.style.top = `${top}px`;
    };
    place();
    window.addEventListener("resize", place);
    return () => window.removeEventListener("resize", place);
  }, [triggerRef]);

  if (typeof document === "undefined") return null;
  return createPortal(
    <div
      ref={menuRef}
      className="menu-panel context-menu session-import__step-menu"
      role="menu"
      aria-label={tr("sessionImport.stepMenu")}
      data-theme={portalTheme}
      onKeyDown={(event) => {
        const native = event.nativeEvent;
        if (native.isComposing || native.keyCode === 229) return;
        const items = enabledItems(menuRef.current);
        const current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
        const index = current ? items.indexOf(current) : -1;
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const next = event.key === "ArrowDown"
            ? items[Math.min(items.length - 1, index < 0 ? 0 : index + 1)]
            : items[Math.max(0, index < 0 ? items.length - 1 : index - 1)];
          next?.focus();
        } else if (event.key === "Home" || event.key === "End") {
          event.preventDefault();
          (event.key === "Home" ? items[0] : items[items.length - 1])?.focus();
        }
      }}
    >
      {SESSION_IMPORT_STEPS.map((item, itemIndex) => {
        const openable = stepOpenable(item) && !busy;
        return (
          <button
            key={item}
            type="button"
            className="context-menu__item"
            role="menuitem"
            disabled={!openable}
            onClick={() => {
              if (!openable) return;
              onGoRef.current(item);
              onCloseRef.current();
            }}
          >
            {`${itemIndex + 1}. ${tr(STEP_KEY[item])}`}
          </button>
        );
      })}
    </div>,
    document.body,
  );
}

export function SessionImportStepNav({
  tr,
  step,
  busy,
  layout,
  stepOpenable,
  onGo,
  portalTheme,
}: {
  tr: SessionImportTFn;
  step: ImportStep;
  busy: boolean;
  layout: "wide" | "narrow";
  stepOpenable: (step: ImportStep) => boolean;
  onGo: (step: ImportStep) => void;
  /** Gallery portals are outside the themed frame. Production inherits the document theme. */
  portalTheme?: string;
}) {
  const [menu, setMenu] = useState<{ edge: "first" | "last" } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const index = Math.max(0, SESSION_IMPORT_STEPS.indexOf(step));
  useEffect(() => {
    if (layout !== "narrow") setMenu(null);
  }, [layout]);
  const openMenu = (edge: "first" | "last") => setMenu({ edge });
  return (
    <div className="session-import__step-nav" data-layout={layout}>
      {layout === "narrow" ? (
        <div className="session-import__step-compact">
          <button
            ref={triggerRef}
            type="button"
            className="btn btn--sm"
            aria-haspopup="menu"
            aria-expanded={menu ? true : false}
            aria-current="step"
            aria-label={tr("sessionImport.stepMenu")}
            onClick={() => setMenu((current) => current ? null : { edge: "first" })}
            onKeyDown={(event) => {
              const native = event.nativeEvent;
              if (native.isComposing || native.keyCode === 229) return;
              if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
              event.preventDefault();
              openMenu(event.key === "ArrowUp" ? "last" : "first");
            }}
          >
            {tr("sessionImport.stepProgress", {
              current: index + 1,
              total: SESSION_IMPORT_STEPS.length,
            })}
          </button>
        </div>
      ) : (
        <ol className="session-import__steps">
          {SESSION_IMPORT_STEPS.map((item, itemIndex) => (
            <li
              key={item}
              data-active={step === item ? "1" : "0"}
              aria-current={step === item ? "step" : undefined}
            >
              <button
                type="button"
                disabled={!stepOpenable(item) || busy}
                onClick={() => onGo(item)}
              >
                <span className="session-import__step-number" aria-hidden="true">{itemIndex + 1}</span>
                {tr(STEP_KEY[item])}
              </button>
            </li>
          ))}
        </ol>
      )}
      {menu && layout === "narrow" ? (
        <SessionImportStepMenu
          tr={tr}
          edge={menu.edge}
          triggerRef={triggerRef}
          portalTheme={portalTheme}
          busy={busy}
          stepOpenable={stepOpenable}
          onGo={onGo}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </div>
  );
}
