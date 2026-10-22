import type { ButtonHTMLAttributes } from "react";
import { Tip } from "@/components/ui/tooltip";

/** Compact controls retain a translated name, shared tooltip and visible focus. */
export function ImportIconButton({
  label, className = "", children, ...props
}: Omit<ButtonHTMLAttributes<HTMLButtonElement>, "title" | "aria-label"> & { label: string }) {
  return (
    <Tip label={label}>
      <button {...props} type="button" aria-label={label}
        className={`btn btn--ghost btn--sm tree-imported__icon ${className}`}>
        {children}
      </button>
    </Tip>
  );
}
