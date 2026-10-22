import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { createPortal } from "react-dom";
import { IconChevronDown } from "@/components/icons";
import { Tip } from "@/components/ui/tooltip";
import { useFloatingMenu, type FloatingPlacement } from "@/lib/floatingMenu";
import { installDialogFocus, isDialogFocusAllowed, listFocusable, trapTabKey } from "@/lib/a11yFocus";

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

interface SelectProps {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  /** compact chip-style in composer */
  variant?: "default" | "chip";
  /** Menu open direction; defaults to auto so low triggers stay in the viewport. */
  placement?: FloatingPlacement;
  className?: string;
  /** Scope option presentation even though the menu lives in a body portal. */
  menuClassName?: string;
  disabled?: boolean;
  "aria-label"?: string;
  title?: string;
}

/** Custom dropdown — menu portaled to body (never clipped by overflow parents). */
export function Select({
  value,
  options,
  onChange,
  variant = "default",
  placement,
  className = "",
  menuClassName = "",
  disabled,
  "aria-label": ariaLabel,
  title,
}: SelectProps) {
  const [open, setOpen] = useState(false);
  const menuOpen = open && !disabled;
  const restoreFocusRef = useRef(true);
  const initialEdgeRef = useRef<"first" | "last">("first");
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLUListElement>(null);
  const listId = useId();
  const selected = options.find((o) => o.value === value) ?? options[0];
  const menuPlacement: FloatingPlacement = placement ?? "auto";

  const { pos, style } = useFloatingMenu({
    open: menuOpen,
    triggerRef,
    panelRef,
    roots: [rootRef],
    onClose: () => {
      // Do not undo the user's outside click with a late effect cleanup.
      restoreFocusRef.current = false;
      setOpen(false);
    },
    closeOnEscape: false,
    placement: menuPlacement,
    fitContent: true,
    // At least as wide as the trigger; grow when option labels are longer.
    matchTriggerWidth: true,
    estHeight: Math.min(280, 40 + options.length * 36),
  });

  const menuReady = menuOpen && !!pos;
  useEffect(() => {
    if (!menuReady) return;
    return installDialogFocus(() => panelRef.current, {
      initialFocus: () => {
        const panel = panelRef.current;
        const selectedOption = panel?.querySelector<HTMLElement>("[aria-selected='true'] button:not([disabled])");
        const enabled = listFocusable(panel);
        return selectedOption ?? (initialEdgeRef.current === "last" ? enabled.at(-1) : enabled[0]) ?? panel;
      },
      onEscape: () => setOpen(false),
      restoreFocus: () => restoreFocusRef.current,
      onTab: (event) => {
        // Options live in a body portal: resume the trigger's logical order,
        // not that portal's position at the end of the document.
        restoreFocusRef.current = false;
        event.stopPropagation();
        const trigger = triggerRef.current;
        trigger?.focus();
        const dialog = trigger?.closest("[role='dialog'], [role='alertdialog']");
        if (dialog) trapTabKey(event, dialog);
        setOpen(false);
      },
    });
  }, [menuReady]);

  useEffect(() => { if (disabled) setOpen(false); }, [disabled]);

  const openMenu = (edge: "first" | "last" = "first") => {
    if (disabled) return;
    restoreFocusRef.current = true;
    initialEdgeRef.current = edge;
    triggerRef.current?.focus();
    setOpen(true);
  };
  const isComposing = (event: KeyboardEvent) => {
    if (!event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229) return false;
    // A native button must not turn IME confirmation into a choice/click.
    if (event.key === "Enter" || event.key === " ") event.preventDefault();
    return true;
  };

  useEffect(() => {
    const panel = panelRef.current;
    if (!menuReady || !panel || !isDialogFocusAllowed(panel)) return;
    const active = document.activeElement;
    if (active === document.body || active === panel ||
      (active && panel.contains(active) && active.hasAttribute("disabled"))) {
      const first = listFocusable(panel)[0];
      (first ?? panel).focus();
    }
  }, [menuReady, options]);

  const menu =
    menuOpen && pos && typeof document !== "undefined"
      ? createPortal(
          <ul
            ref={panelRef}
            className={`menu-panel c-select__menu c-select__menu--portal ${menuClassName}`}
            role="listbox"
            tabIndex={-1}
            aria-label={ariaLabel}
            id={listId}
            style={style}
            /* Keep modal from treating option clicks as outside / drag */
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={(event) => {
              if (isComposing(event)) return;
              if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
              event.preventDefault();
              event.stopPropagation();
              const enabled = listFocusable(panelRef.current);
              if (!enabled.length) return;
              const current = enabled.indexOf(document.activeElement as HTMLElement);
              const next = event.key === "Home" ? 0 : event.key === "End" ? enabled.length - 1
                : event.key === "ArrowDown" ? (current + 1) % enabled.length
                  : (current - 1 + enabled.length) % enabled.length;
              const target = enabled[next];
              target?.focus();
              target?.scrollIntoView?.({ block: "nearest" });
            }}
          >
            {options.map((o) => (
              <li key={o.value} role="option" aria-selected={o.value === value} aria-disabled={o.disabled || undefined}>
                <button
                  type="button"
                  tabIndex={-1}
                  className={
                    "c-select__option" +
                    (o.value === value ? " is-selected" : "") +
                    (o.disabled ? " is-disabled" : "")
                  }
                  disabled={o.disabled}
                  onClick={() => {
                    if (o.disabled) return;
                    restoreFocusRef.current = true;
                    setOpen(false);
                    onChange(o.value);
                  }}
                >
                  {o.label}
                </button>
              </li>
            ))}
          </ul>,
          document.body,
        )
      : null;

  const trigger = (
    <button
      ref={triggerRef}
      type="button"
      className="c-select__trigger"
      disabled={disabled}
      aria-haspopup="listbox"
      aria-expanded={menuOpen}
      aria-controls={listId}
      aria-label={ariaLabel}
      onClick={() => { if (menuOpen) setOpen(false); else openMenu(); }}
      onKeyDown={(event) => {
        if (isComposing(event) || disabled) return;
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          event.stopPropagation();
          openMenu(event.key === "ArrowUp" ? "last" : "first");
        }
      }}
    >
      <span className="c-select__value">{selected?.label ?? value}</span>
      <span className="c-select__chev" aria-hidden>
        <IconChevronDown size={14} />
      </span>
    </button>
  );

  return (
    <div
      ref={rootRef}
      className={`c-select c-select--${variant} ${menuOpen ? "is-open" : ""} ${className}`}
    >
      {title ? <Tip label={title}>{trigger}</Tip> : trigger}
      {menu}
    </div>
  );
}
