import { useState } from "react";
import { MarkdownBody } from "@/components/MarkdownBody";
import type { ImportedImages } from "@/hooks/useImportedImages";
import { importedEventDisplay } from "@/lib/sessionImport/eventView";
import { ImportedImage } from "./ImportedImage";
import { ImportedTextReader } from "./ImportedTextReader";
import type { SessionImportTFn } from "./sessionImportHostLabels";

/** Shared ordered body; content-selection checkboxes live outside this surface. */
export function ImportedEventBody({ event, images, tr }: { event: unknown; images?: ImportedImages; tr: SessionImportTFn }) {
  const view = importedEventDisplay(event);
  const [fullText, setFullText] = useState<{ event: unknown; text: string } | null>(null);
  return <>
    {view.parts.map((part, index) => {
      if (part.kind === "asset") return <ImportedImage key={index} assetId={part.assetId} alt={part.alt} images={images} tr={tr} />;
      if (part.kind === "tool-input-payload") return <p key={index} className="session-import__notice">
        {tr("sessionImport.toolInputPayload", { id: part.blobId || "—" })}</p>;
      if (part.kind === "unsupported") return <p key={index} className="session-import__notice" data-issue="unsupported">
        {tr("sessionImport.unsupportedBlock", { type: part.sourceType })}</p>;
      if (part.kind === "truncated") return <p key={index} className="session-import__notice" data-issue="truncated">
        {tr("sessionImport.truncatedOutput")}</p>;
      if (part.kind === "incomplete") return <p key={index} className="session-import__notice" data-issue="incomplete">
        {tr("sessionImport.mediaIncomplete")}</p>;
      return <div key={index}>
        {part.kind === "system" ? <p className="session-import__notice" data-issue="system">{tr("sessionImport.systemNotice")}</p> : null}
        {view.kind === "message" && view.role === "assistant" ? <MarkdownBody inert>{part.text}</MarkdownBody>
          : <pre className={view.role === "user" ? "session-import__user-body" : undefined}>{part.text}</pre>}
        {part.fullText ? <button type="button" className="btn btn--sm btn--ghost"
          onClick={(e) => { e.stopPropagation(); setFullText({ event, text: part.fullText! }); }}>{tr("sessionImport.readFullText")}</button> : null}
      </div>;
    })}
    {view.flags.displayClipped && !view.parts.some((part) => (part.kind === "text" || part.kind === "system") && part.fullText)
      ? <p className="session-import__notice">{tr("sessionImport.mediaPreviewLimited")}</p> : null}
    {fullText && fullText.event === event ? <ImportedTextReader text={fullText.text} tr={tr} onClose={() => setFullText(null)} /> : null}
  </>;
}
