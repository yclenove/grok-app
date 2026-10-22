import { useEffect, useRef, useState } from "react";
import { sessionReceiptReview, sessionReceiptSnapshot, type ReceiptDecision, type ReceiptReviewAck, type ReceiptRow } from "@/lib/api/sessionReceipts";
export function useSessionReceipts() {
  const alive = useRef(true), generation = useRef(0), readId = useRef(0), sending = useRef(false), shown = useRef(false);
  const [open, setOpen] = useState(false), [items, setItems] = useState<ReceiptRow[]>([]);
  const [selected, setSelected] = useState<ReceiptRow | null>(null), [ack, setAck] = useState<ReceiptReviewAck | null>(null);
  const [loading, setLoading] = useState(false), [verified, setVerified] = useState(false), [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null), [prompt, setPrompt] = useState("");
  const [reviewed, setReviewed] = useState(false), [confirmed, setConfirmed] = useState(false);
  useEffect(() => { alive.current = true; return () => { alive.current = false; generation.current++; readId.current++; }; }, []);
  async function refresh() {
    const id = ++readId.current;
    setLoading(true); setVerified(false); setError(null);
    try { const next = await sessionReceiptSnapshot(); if (alive.current && id === readId.current) { setItems(next.items); setVerified(true); } }
    catch { if (alive.current && id === readId.current) setError("SESSION_RECEIPT_UNAVAILABLE"); }
    finally { if (alive.current && id === readId.current) setLoading(false); }
  }
  function reset() { setPrompt(""); setReviewed(false); setConfirmed(false); setAck(null); setError(null); }
  function show() { shown.current = true; generation.current++; reset(); setSelected(null); setOpen(true); void refresh(); }
  function close() { shown.current = false; generation.current++; readId.current++; setOpen(false); reset(); setSelected(null); setLoading(false); setVerified(false); }
  function select(row: ReceiptRow | null) { generation.current++; reset(); setSelected(row); }
  const current = selected && items.find(r => r.sessionId === selected.sessionId && r.key === selected.key && r.revision === selected.revision);
  const resolution = ack ?? current?.review ?? null;
  const canReview = !!current?.canReview && verified && !loading && !busy && !resolution;
  async function submit(decision: ReceiptDecision) {
    if (!current || !canReview || !reviewed || !confirmed || sending.current || (decision === "retry" && !prompt.trim())) return;
    const gen = generation.current; sending.current = true; setBusy(true); setError(null);
    try {
      const result = await sessionReceiptReview({ sessionId: current.sessionId, key: current.key, revision: current.revision, prompt, decision, reviewed: true, confirmed: true });
      if (alive.current && gen === generation.current) { setAck(result); setReviewed(false); setConfirmed(false); setPrompt(""); }
    } catch (e) {
      if (alive.current && gen === generation.current) setError(e instanceof Error ? e.message : typeof e === "string" ? e : "SESSION_RECEIPT_UNAVAILABLE");
    } finally {
      sending.current = false;
      if (alive.current) { setBusy(false); if (shown.current) void refreshAfterSend(); }
    }
  }
  async function refreshAfterSend() {
    // Preserve a truthful mutation error / ACK; refreshing never retries an action.
    const id = ++readId.current; setLoading(true); setVerified(false);
    try { const next = await sessionReceiptSnapshot(); if (alive.current && id === readId.current) { setItems(next.items); setVerified(true); } }
    catch { if (alive.current && id === readId.current) setError(previous => previous ?? "SESSION_RECEIPT_UNAVAILABLE"); }
    finally { if (alive.current && id === readId.current) setLoading(false); }
  }
  return { open, show, close, items, selected, select, resolution, canReview, loading, verified, busy, error, prompt, setPrompt,
    reviewed, setReviewed, confirmed, setConfirmed, refresh, submit };
}
