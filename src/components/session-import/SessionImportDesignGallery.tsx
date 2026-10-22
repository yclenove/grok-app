import { useState } from "react";
import { createT, type Locale } from "@/i18n";
import { GlassModal } from "@/components/GlassModal";
import { MarkdownBody } from "@/components/MarkdownBody";
import { Select } from "@/components/Select";
import type { ImportStep } from "@/hooks/useSessionImport";
import { SessionImportPageStatus } from "./SessionImportPageStatus";
import { SessionImportReaderSplit, type ReaderPane } from "./SessionImportReaderSplit";
import { SessionImportStepNav } from "./SessionImportStepNav";

type Layout = "wide" | "narrow";
type Theme = "dark" | "light";

const OPEN: Record<ImportStep, boolean> = {
  source: true,
  content: true,
  prepare: false,
  mapping: false,
  import: false,
  result: false,
};

/**
 * Synthetic review artifact. It reuses product primitives and does not read
 * history, start an import, or call a provider.
 */
export function SessionImportDesignGallery() {
  return (
    <div data-design-gallery="synthetic" className="session-import-gallery">
      <GalleryFrame
        id="wide"
        title="Wide"
        locale="en"
        theme="dark"
        layout="wide"
        width={960}
      />
      <GalleryFrame
        id="minimum"
        title="Minimum content width"
        locale="en"
        theme="light"
        layout="wide"
        width={900}
      />
      <GalleryFrame
        id="narrow"
        title="Narrow dialog"
        locale="de"
        theme="dark"
        layout="narrow"
        width={560}
      />
      <GalleryFrame
        id="short"
        title="Short height"
        locale="de"
        theme="light"
        layout="wide"
        width={860}
        height={420}
      />
      <GalleryFrame
        id="long"
        title="Long translation"
        locale="de"
        theme="dark"
        layout="wide"
        width={960}
      />
    </div>
  );
}

function GalleryFrame(props: {
  id: string;
  title: string;
  locale: Locale;
  theme: Theme;
  layout: Layout;
  width: number;
  height?: number;
}) {
  const tr = createT(props.locale);
  const [step, setStep] = useState<ImportStep>("content");
  const [pane, setPane] = useState<ReaderPane>("events");
  const [project, setProject] = useState("");
  const [draft, setDraft] = useState("Synthetic continuation draft.");
  const [saved, setSaved] = useState(draft);
  const [review, setReview] = useState(false);
  const short = props.height != null;
  const body = (
    <>
      <p className="session-import-gallery__mock" role="note">{tr("sessionImport.gallery.synthetic")}</p>
      <SessionImportStepNav
        tr={tr}
        step={step}
        busy={false}
        layout={props.layout}
        stepOpenable={(item) => OPEN[item]}
        onGo={setStep}
        portalTheme={props.theme}
      />
      <SessionImportPageStatus tr={tr} step={step} />
      <SessionImportReaderSplit
        tr={tr}
        narrow={props.layout === "narrow"}
        pane={pane}
        onPane={setPane}
        matches={<p className="session-import__hit-snippet">synthetic match · page count only</p>}
        events={
          <ol className="session-import__events">
            <li data-role="user">
              <div className="session-import__user-body">Synthetic user note 🙂 锚点</div>
            </li>
            <li data-role="assistant">
              <MarkdownBody locale={props.locale} inert>{"Synthetic **assistant** note. Not sent."}</MarkdownBody>
            </li>
          </ol>
        }
      />
      <label className="session-import__page-line">
        {tr("sessionImport.mapUnassigned")}
        <Select
          aria-label={tr("sessionImport.mappingTarget", { source: "synthetic", target: project || "history" })}
          value={project}
          onChange={setProject}
          options={[
            { value: "", label: tr("sessionImport.mapUnassigned") },
            { value: "proj-synthetic", label: tr("sessionImport.mapExisting") },
          ]}
        />
      </label>
      <section aria-label={tr("sessionImport.handoffSource")}>
        <h3 className="session-import__pane-title">{tr("sessionImport.handoffSource")}</h3>
        <p className="session-import__page-line">import synthetic · revision r1 · leaf L1</p>
      </section>
      <section aria-label={tr("sessionImport.handoffTarget")}>
        <h3 className="session-import__pane-title">{tr("sessionImport.handoffTarget")}</h3>
        <p className="session-import__page-line" role="status">{saved === draft ? tr("sessionImport.continuationSaved") : tr("sessionImport.handoffEdit")}</p>
        <button type="button" className="btn btn--primary" onClick={() => setReview(true)}>
          {tr("sessionImport.continuationReview")}
        </button>
      </section>
    </>
  );
  return (
    <section
      data-gallery-frame={props.id}
      data-theme={props.theme}
      data-layout={props.layout}
      className="session-import-gallery__frame"
      style={{ width: props.width }}
      aria-label={props.title}
    >
      {short ? (
        <div className="session-import-gallery__short" style={{ height: props.height }}>
          <div className="session-import-gallery__scroll">{body}</div>
          <div className="session-import__toolbar session-import__footer" data-gallery-footer="1">
            <button type="button" className="btn" onClick={() => setStep("source")}>{tr("sessionImport.back")}</button>
            <button type="button" className="btn btn--primary" onClick={() => setStep("content")}>{tr("sessionImport.next")}</button>
          </div>
        </div>
      ) : (
        <div className="session-import-modal session-import-wizard">{body}</div>
      )}
      <GlassModal
        open={review}
        onClose={() => setReview(false)}
        title={tr("sessionImport.handoffTitle")}
        size="lg"
        wrapBody
        className="session-import-modal session-import-continuation"
        closeLabel={tr("sessionImport.close")}
        dataTheme={props.theme}
        footer={
          <>
            <button type="button" className="btn btn--ghost" onClick={() => { setDraft(saved); }}>{tr("sessionImport.continuationReload")}</button>
            <button type="button" className="btn btn--primary" onClick={() => { setSaved(draft); setReview(false); }}>{tr("common.save")}</button>
          </>
        }
      >
        <p className="session-import__read-only">{tr("sessionImport.handoffSendHint")}</p>
        <textarea
          className="session-import__draft"
          aria-label={tr("sessionImport.handoffEdit")}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          rows={6}
        />
      </GlassModal>
    </section>
  );
}
