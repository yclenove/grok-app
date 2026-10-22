import { useRef } from "react";
import { flushSync } from "react-dom";
import type { createT } from "@/i18n";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";

export function ImportedReaderSearch(props: {
  tr: ReturnType<typeof createT>;
  busy: boolean;
  query: string;
  hasHits: boolean;
  onQueryChange: (value: string) => void;
  onSearch: () => void | Promise<void>;
  onClear: () => void;
}) {
  const { tr } = props;
  const composing = useRef(false);
  const blockedEnter = useRef(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const resetInputCycle = () => {
    composing.current = false;
    blockedEnter.current = false;
  };
  // Clear removes its own focused button in this commit. Return to this
  // mounted field before the key cycle continues. A covered button, a focus
  // move, or an unmounted reader leaves no claim to revive later.
  const activateClear = (target: HTMLButtonElement) => {
    if (!isDialogFocusAllowed(target)) return;
    const owned = target.ownerDocument.activeElement === target;
    flushSync(() => { props.onClear(); });
    const input = inputRef.current;
    if (!owned || !input?.isConnected || input.disabled || !isDialogFocusAllowed(input)) return;
    const doc = input.ownerDocument;
    const active = doc.activeElement;
    const lost = !active || active === doc.body || active === doc.documentElement || !active.isConnected;
    const stillClear = active instanceof HTMLElement && active.dataset.readerClear === "1";
    if (!lost && !stillClear) return;
    input.focus({ preventScroll: true });
  };
  return (
    <form
      className="session-import__toolbar session-import__reader-search"
      onKeyDown={(e) => {
        // Observe the key cycle; never consume an OS candidate-selection key.
        blockedEnter.current = e.key === "Enter" && (
          e.defaultPrevented || e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229 ||
          e.ctrlKey || e.altKey || e.metaKey || e.shiftKey
        );
      }}
      onKeyUp={(e) => { if (e.key === "Enter") blockedEnter.current = false; }}
      onSubmit={(e) => {
        const cancelled = e.defaultPrevented;
        e.preventDefault();
        if (cancelled || props.busy || composing.current || blockedEnter.current ||
            !isDialogFocusAllowed(e.currentTarget)) return;
        void props.onSearch();
      }}
    >
      <input
        ref={inputRef}
        type="search"
        className="input"
        placeholder={tr("sessionImport.searchPlaceholder")}
        value={props.query}
        onChange={(e) => props.onQueryChange(e.target.value)}
        onCompositionStart={() => { composing.current = true; }}
        onCompositionEnd={() => { composing.current = false; }}
        onBlur={resetInputCycle}
        aria-label={tr("sessionImport.searchPlaceholder")}
      />
      <button type="submit" className="btn btn--ghost" disabled={props.busy}>
        {tr("sessionImport.search")}
      </button>
      {props.query || props.hasHits ? (
        <button
          type="button"
          className="btn btn--ghost"
          data-reader-clear="1"
          onKeyDown={(event) => {
            const plain = (event.key === "Enter" || event.key === " ") &&
              !event.defaultPrevented && !event.nativeEvent.isComposing && event.nativeEvent.keyCode !== 229 &&
              !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
            if (!plain || !isDialogFocusAllowed(event.currentTarget)) return;
            // The button's default Enter/Space click would run after this, and
            // Space would type into the field once focus has moved.
            event.preventDefault();
            event.stopPropagation();
            activateClear(event.currentTarget);
          }}
          onClick={(event) => { activateClear(event.currentTarget); }}
        >
          {tr("sessionImport.clearSearch")}
        </button>
      ) : null}
    </form>
  );
}
