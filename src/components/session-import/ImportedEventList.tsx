import type { MessageKey, Vars } from "@/i18n";
import { ImportCopyButton } from "./ImportCopyButton";
import { eventPlainText, importedEventDisplay } from "@/lib/sessionImport/eventView";
import type { ImportedImages } from "@/hooks/useImportedImages";
import { ImportedEventBody } from "./ImportedEventBody";

type TFn = (key: MessageKey, vars?: Vars) => string;

export function ImportedEventList({
  events,
  highlightId,
  tr,
  onContinueFrom,
  idPrefix = "import-ev",
  images,
}: {
  events: unknown[];
  highlightId?: string | null;
  tr: TFn;
  onContinueFrom?: (eventId: string) => void;
  idPrefix?: string;
  images?: ImportedImages;
}) {
  return (
    <>
    <ol className="session-import__events">
      {events.map((raw, i) => {
        const view = importedEventDisplay(raw);
        return (
          <li
            key={view.id ?? i}
            id={view.id ? `${idPrefix}-${view.id}` : undefined}
            tabIndex={view.id ? -1 : undefined}
            data-highlight={highlightId && view.id === highlightId ? "1" : "0"}
            data-kind={view.kind}
            data-role={view.role}
            data-system={view.flags.system ? "1" : "0"}
          >
            {view.role !== "user" && view.role !== "assistant" ? (
              <strong>{view.heading}</strong>
            ) : null}
            <div className="session-import__event-actions">
              {eventPlainText(view) ? (
                <ImportCopyButton text={eventPlainText(view)} tr={tr} />
              ) : null}
              {onContinueFrom &&
              view.id &&
              view.kind !== "unsupported" &&
              view.kind !== "metadata" &&
              !view.flags.system ? (
                <button
                  type="button"
                  className="btn btn--sm btn--ghost"
                  onClick={() => onContinueFrom(view.id!)}
                >
                  {tr("sessionImport.continueFromHere")}
                </button>
              ) : null}
            </div>
            <ImportedEventBody event={raw} tr={tr} images={images} />
          </li>
        );
      })}
    </ol>
    </>
  );
}
