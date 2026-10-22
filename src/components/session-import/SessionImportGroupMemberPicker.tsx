import { useId, useRef } from "react";
import type { SourceGroup } from "@/lib/api/sessionMigrationGroups";
import type { ImportGroupScope } from "@/lib/sessionImport/groupSelectionState";
import type { SessionImportTFn } from "./sessionImportHostLabels";
import { SessionImportGroupMemberMenu } from "./SessionImportGroupMemberMenu";

export function SessionImportGroupMemberPicker(props: {
  scope: ImportGroupScope;
  group: SourceGroup;
  candidateId: string;
  open: boolean;
  disabled: boolean;
  busy: boolean;
  tr: SessionImportTFn;
  onOpen: () => void;
  onClose: () => void;
  onChoose: (groupId: string, candidateId: string) => Promise<boolean>;
  onRefresh: () => void | Promise<unknown>;
}) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();
  const key = JSON.stringify([props.scope.scanId, props.scope.epoch, props.scope.generation, props.group.groupId]);
  const open = () => {
    if (props.disabled) return;
    triggerRef.current?.focus();
    props.onOpen();
  };
  return <>
    <button ref={triggerRef} type="button" className="btn btn--sm btn--ghost"
      aria-haspopup="menu" aria-expanded={props.open} aria-controls={props.open ? menuId : undefined}
      disabled={props.disabled}
      onClick={() => props.open ? props.onClose() : open()}
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229) {
          if (event.key === "Enter" || event.key === " ") event.preventDefault();
          return;
        }
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault(); event.stopPropagation(); open();
        }
      }}>
      {props.tr("sessionImport.versions.button", { n: props.group.memberCount })}
    </button>
    {props.open ? <SessionImportGroupMemberMenu key={key} {...props} triggerRef={triggerRef} menuId={menuId} /> : null}
  </>;
}
