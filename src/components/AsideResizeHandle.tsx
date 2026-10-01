import { asideResizeKey, type AsideResizeControl } from "@/lib/asideResize";

export function AsideResizeHandle({ control, label, busy }: {
  control: AsideResizeControl;
  label: string;
  busy: boolean;
}) {
  return (
    <div
      className="aside-resizer"
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      aria-controls="workbench-aside"
      aria-label={label}
      aria-valuemin={control.min}
      aria-valuemax={control.max}
      aria-valuenow={control.value}
      aria-keyshortcuts="ArrowLeft ArrowRight Shift+ArrowLeft Shift+ArrowRight Home End"
      onKeyDown={(event) => {
        if (busy || event.altKey || event.ctrlKey || event.metaKey) return;
        const request = asideResizeKey(event.key, event.shiftKey);
        if (!request) return;
        event.preventDefault();
        control.change(request);
      }}
      onPointerDown={(event) => {
        if (busy || event.button !== 0) return;
        event.preventDefault();
        event.currentTarget.focus({ preventScroll: true });
        control.begin();
      }}
    />
  );
}
