import { useCallback, useRef, useState } from "react";
import {
  sessionImportPreview,
  sessionImportPreviewEvents,
} from "@/lib/api/sessionMigration";
import {
  peekSummaryFromAdapter,
  type PeekSummary,
} from "@/lib/sessionImport/peekSummary";

export type PeekEvent = {
  eventId: string;
  kind: string;
  role?: string | null;
  text: string;
  truncated?: boolean;
};

export function useCandidatePeek(onError: (message: string) => void) {
  const [title, setTitle] = useState<string | null>(null);
  const [events, setEvents] = useState<PeekEvent[]>([]);
  const [busy, setBusy] = useState(false);
  const [candidateId, setCandidateId] = useState<string | null>(null);
  const [previewId, setPreviewId] = useState<string | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [summary, setSummary] = useState<PeekSummary | null>(null);
  const generation = useRef(0);

  const peek = useCallback(
    async (id: string, sourceRevision: string | null | undefined, label: string) => {
      const request = ++generation.current;
      setBusy(true);
      setCandidateId(id);
      setTitle(label);
      setEvents([]);
      setPreviewId(null);
      setNextCursor(null);
      setSummary(null);
      try {
        const preview = await sessionImportPreview(id, sourceRevision);
        if (request !== generation.current) return;
        const page = await sessionImportPreviewEvents(preview.previewId, null, 20);
        if (request !== generation.current) return;
        setPreviewId(preview.previewId);
        setEvents(page.events);
        setNextCursor(page.nextCursor == null ? null : String(page.nextCursor));
        setSummary(peekSummaryFromAdapter(preview.adapter));
      } catch (e) {
        if (request !== generation.current) return;
        setEvents([]);
        setSummary(null);
        onError(String(e));
      } finally {
        if (request === generation.current) setBusy(false);
      }
    },
    [onError],
  );

  const loadMore = useCallback(async () => {
    if (!previewId || nextCursor == null) return;
    const request = generation.current;
    setBusy(true);
    try {
      const page = await sessionImportPreviewEvents(previewId, nextCursor, 20);
      if (request !== generation.current) return;
      setEvents((prev) => [...prev, ...page.events]);
      setNextCursor(page.nextCursor == null ? null : String(page.nextCursor));
      return page;
    } catch (e) {
      if (request !== generation.current) return;
      onError(String(e));
    } finally {
      if (request === generation.current) setBusy(false);
    }
    return undefined;
  }, [previewId, nextCursor, onError]);

  const clear = useCallback(() => {
    generation.current += 1;
    setTitle(null);
    setEvents([]);
    setCandidateId(null);
    setPreviewId(null);
    setNextCursor(null);
    setSummary(null);
  }, []);

  return {
    title,
    events,
    busy,
    candidateId,
    previewId,
    nextCursor,
    summary,
    peek,
    loadMore,
    clear,
  };
}
