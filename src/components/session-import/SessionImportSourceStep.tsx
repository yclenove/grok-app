import { useLayoutEffect, useRef, useState } from "react";
import { Select } from "@/components/Select";
import { SessionImportSourceLocationsControl } from "./SessionImportSourceLocationsControl";
import { SessionImportDefaultLocationsControl } from "./SessionImportDefaultLocationsControl";
import { isDialogFocusAllowed } from "@/lib/a11yFocus";
import { shortPathLabel } from "@/lib/extensionsUi";
import { ownPickerTrigger, releaseDisconnectedPickerTrigger, restorePickerTrigger } from "@/lib/sessionImport/pickerFocus";
import type { MessageKey, Vars } from "@/i18n";
import type {
  DiscoveredRoot,
  ImportProvider,
  SourceRegistration,
} from "@/lib/api/sessionMigration";
import {
  choiceFromDefaultRoot,
  choiceFromRegistered,
  kindLabelKey,
  sameSourceChoice,
  type SourceChoice,
} from "@/lib/sessionImport/sourceChoice";

type TFn = (key: MessageKey, vars?: Vars) => string;

const CUSTOM_FOLDER_PROVIDERS: ImportProvider[] = [
  "codex",
  "claude-code",
  "cursor",
  "generic-file",
];

function providerKey(provider: string): MessageKey {
  if (provider === "codex") return "sessionImport.source.codex";
  if (provider === "claude-code") return "sessionImport.source.claude";
  if (provider === "cursor") return "sessionImport.source.cursor";
  return "sessionImport.source.file";
}

function rootStatus(root: DiscoveredRoot, tr: TFn): string {
  if (!root.exists) return tr("sessionImport.notInstalled");
  if (!root.readable) return tr("sessionImport.missingDir");
  return "";
}

function SourceLocation({ path }: { path: string }) {
  return (
    <span className="session-import__source-path" title={path}>
      <span aria-hidden="true">{shortPathLabel(path, 44)}</span>
      <span className="sr-only">{path}</span>
    </span>
  );
}

export function SessionImportSourceActions(props: {
  tr: TFn;
  choice: SourceChoice | null;
  busy: boolean;
  scanStarting: boolean;
  onChoose: (choice: SourceChoice | null) => void;
  onScan: () => void;
}) {
  const { tr, choice, busy } = props;
  return (
    <div className="session-import__toolbar session-import__footer session-import__source-actions">
      {choice ? (
        <button type="button" className="btn" disabled={busy} onClick={() => props.onChoose(null)}>
          {tr("sessionImport.clearSource")}
        </button>
      ) : null}
      <button type="button" className="btn btn--primary" disabled={busy} onClick={props.onScan}>
        {props.scanStarting
          ? tr("sessionImport.scanning")
          : choice
            ? tr("sessionImport.scan")
            : tr("sessionImport.scanAll")}
      </button>
    </div>
  );
}

export function SessionImportSourceStep(props: {
  tr: TFn;
  roots: DiscoveredRoot[];
  registrations: SourceRegistration[];
  choice: SourceChoice | null;
  busy: boolean;
  onChoose: (choice: SourceChoice | null) => void;
  onPickFolder: (provider: ImportProvider) => void | Promise<unknown>;
  onPickFile: () => void | Promise<unknown>;
  onPickWorkspaceFolder?: () => void | Promise<unknown>;
  onWorkspaces?: () => void;
  onRelocate: (reg: SourceRegistration) => void | Promise<unknown>;
  onScan: () => void;
  fileFlight?: Promise<unknown> | null;
}) {
  const { tr, choice, busy } = props;
  const [pendingProvider, setFolderProvider] = useState<ImportProvider>("codex");
  const sourceIntent = JSON.stringify(choice);
  const fileButtonRef = useRef<HTMLButtonElement>(null);
  const adoptedFileFlight = useRef<Promise<unknown> | null>(null);
  useLayoutEffect(() => {
    restorePickerTrigger();
    return releaseDisconnectedPickerTrigger;
  });
  useLayoutEffect(() => {
    const flight = props.fileFlight;
    const button = fileButtonRef.current;
    if (!flight || !button || adoptedFileFlight.current === flight) return;
    adoptedFileFlight.current = flight;
    void ownPickerTrigger(button, () => flight);
  }, [props.fileFlight]);
  const folderProvider =
    choice?.kind === "custom-folder" &&
    CUSTOM_FOLDER_PROVIDERS.includes(choice.provider as ImportProvider)
      ? (choice.provider as ImportProvider)
      : pendingProvider;
  return (
    <div
      className="session-import__source"
      onKeyDown={(e) => {
        // Scan only for an unclaimed plain Enter in the active dialog.
        if (e.target !== e.currentTarget || e.key !== "Enter" || busy ||
            e.defaultPrevented || e.nativeEvent.isComposing || e.nativeEvent.keyCode === 229 ||
            e.ctrlKey || e.altKey || e.metaKey || e.shiftKey ||
            !isDialogFocusAllowed(e.currentTarget)) return;
        e.preventDefault();
        props.onScan();
      }}
    >
      <p className="session-import__source-intro">{tr("sessionImport.entryHint")}</p>
      <section className="session-import__group session-import__custom-source">
        <h3 className="session-import__group-title">{tr("sessionImport.customRoot")}</h3>
        <div className="session-import__toolbar">
          <Select
            aria-label={tr("sessionImport.customRoot")}
            className="session-import__source-provider"
            disabled={busy}
            value={folderProvider}
            onChange={(value) => {
              const provider = CUSTOM_FOLDER_PROVIDERS.find((item) => item === value);
              if (!provider) return;
              setFolderProvider(provider);
              if (choice?.kind === "custom-folder") {
                props.onChoose({ ...choice, provider });
              }
            }}
            options={CUSTOM_FOLDER_PROVIDERS.map((provider) => ({
              value: provider,
              label: tr(providerKey(provider)),
            }))}
          />
          <button type="button" className="btn" disabled={busy} onClick={(event) => {
            void ownPickerTrigger(event.currentTarget, () => props.onPickFolder(folderProvider));
          }}>
            {tr("sessionImport.chooseRoot")}
          </button>
          <button ref={fileButtonRef} type="button" className="btn" disabled={busy} onClick={(event) => {
            void ownPickerTrigger(event.currentTarget, () => props.onPickFile());
          }}>
            {tr("sessionImport.fileMode")}
          </button>
        </div>
        {props.onPickWorkspaceFolder || props.onWorkspaces ? (
          <div className="session-import__toolbar session-import__workspace-actions">
            {props.onPickWorkspaceFolder ? (
              <button type="button" className="btn" disabled={busy} onClick={(event) => {
                void ownPickerTrigger(event.currentTarget, () => props.onPickWorkspaceFolder?.());
              }}>
                {tr("sessionImport.workspace.folder")}
              </button>
            ) : null}
            {props.onWorkspaces ? (
              <button type="button" className="btn" disabled={busy} onClick={props.onWorkspaces}>
                {tr("sessionImport.workspace.savedList")}
              </button>
            ) : null}
          </div>
        ) : null}
      </section>
      {choice ? (
        <p className="session-import__source-choice" data-source-kind={choice.kind}
          data-kind={choice.kind === "default" || choice.kind === "registered" ? undefined : choice.kind}>
          <span className="session-import__badge">{tr(kindLabelKey(choice.kind))}</span>
          <span>{tr(providerKey(choice.provider))}</span>
          <span className="session-import__locator">{choice.path}</span>
          {choice.locations ? <span>{tr("sessionImport.locations.selected", { n: choice.locations.length })}</span> : null}
        </p>
      ) : null}
      <div className="session-import__source-grid">
      <section className="session-import__group">
        <h3 className="session-import__group-title">{tr("sessionImport.defaults")}</h3>
        {props.roots.length === 0 ? <p>{tr("sessionImport.noSources")}</p> : null}
        <ul className="session-import__roots" aria-label={tr("sessionImport.defaults")}>
          {props.roots.map((root) => {
            const next = choiceFromDefaultRoot(root.provider, root.path);
            const status = rootStatus(root, tr);
            return (
              <li key={`${root.provider}-${root.kind}-${root.path}`} className="session-import__root-row">
                <button
                  type="button"
                  className="btn"
                  data-kind="default"
                  data-root-kind={root.kind}
                  data-selected={sameSourceChoice(choice, next) ? "1" : "0"}
                  aria-pressed={sameSourceChoice(choice, next)}
                  disabled={busy}
                  onClick={() => props.onChoose(next)}
                >
                  {tr(providerKey(root.provider))}
                  <span className="session-import__badge" data-kind="ok">
                    {tr("sessionImport.kind.default")}
                  </span>
                  <SourceLocation path={root.path} />
                  {status ? <span className="session-import__notice">{status}</span> : null}
                </button>
                {root.provider === "cursor" && root.exists && root.readable ?
                  <SessionImportDefaultLocationsControl root={root} disabled={busy} tr={tr} onChoose={props.onChoose} scopeKey={sourceIntent} /> : null}
              </li>
            );
          })}
        </ul>
      </section>
      <section className="session-import__group">
        <h3 className="session-import__group-title">{tr("sessionImport.registered")}</h3>
        {props.registrations.length === 0 ? (
          <p>{tr("sessionImport.noRegistered")}</p>
        ) : (
          <ul className="session-import__roots" aria-label={tr("sessionImport.registered")}>
            {props.registrations.map((reg) => {
              const next = choiceFromRegistered(reg);
              return (
                <li key={reg.sourceId} className="session-import__root-row">
                  <button
                    type="button"
                    className="btn"
                    data-kind="registered"
                    data-selected={sameSourceChoice(choice, next) ? "1" : "0"}
                    aria-pressed={sameSourceChoice(choice, next)}
                    disabled={busy}
                    onClick={() => props.onChoose(next)}
                  >
                    {tr(providerKey(reg.provider))}
                    <span className="session-import__badge">
                      {tr("sessionImport.kind.registered")}
                    </span>
                    <SourceLocation path={reg.rootPath} />
                  </button>
                  <button
                    type="button"
                    className="btn btn--sm"
                    data-kind="relocate"
                    disabled={busy}
                    onClick={(event) => {
                      void ownPickerTrigger(event.currentTarget, () => props.onRelocate(reg));
                    }}
                  >
                    {tr("sessionImport.relocateSource")}
                  </button>
                  {reg.provider === "cursor" && (!reg.location || reg.location.host === "local") ?
                    <SessionImportSourceLocationsControl source={reg} disabled={busy} tr={tr} onChoose={props.onChoose} scopeKey={sourceIntent} /> : null}
                </li>
              );
            })}
          </ul>
        )}
      </section>
      </div>
    </div>
  );
}
