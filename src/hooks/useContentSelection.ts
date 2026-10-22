import { useCallback, useReducer } from "react";
import {
  emptyContentSelection,
  reduceContentSelection,
  selectionFromState,
  type ContentSelectionAction,
  type PreviewEventRow,
} from "@/lib/sessionImport/contentSelection";
import type { AllowedSelection } from "@/lib/sessionImport/selection";

export function useContentSelection() {
  const [state, dispatch] = useReducer(reduceContentSelection, emptyContentSelection());

  const reset = useCallback(
    (payload: {
      previewId: string;
      leaf: string | null;
      leaves: string[];
      events: PreviewEventRow[];
      nextCursor: string | null;
      defaultSelection?: AllowedSelection | null;
      archiveCount?: number;
    }) => {
      dispatch({ type: "reset", ...payload });
    },
    [],
  );

  const appendPage = useCallback((action: Extract<ContentSelectionAction, { type: "appendPage" }>) => {
    dispatch(action);
  }, []);

  return {
    state,
    dispatch,
    reset,
    appendPage,
    selection: selectionFromState(state),
  };
}
