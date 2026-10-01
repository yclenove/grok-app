import { useEffect, useRef, useState } from "react";
import * as api from "@/lib/api/computerUse";
import { sideBrowserList } from "@/lib/api/system";
import type { ComputerSurface } from "@/lib/computer-use/surface";
import { COMPUTER_STATUS_STALE_MS } from "@/lib/computer-use/taskCard";

// This bounds only the UI's read-only discovery attempt. It is not proof that
// a native operation has stopped, and never releases action ownership.
const TARGET_DISCOVERY_TIMEOUT_MS = 10_000;

/** Session-keyed controller. Candidate selection never grants control. */
export function useComputerController(sessionId: string | null, runId: string | null, surface: ComputerSurface) {
  const [status, setStatus] = useState<api.ComputerStatus | null>(null);
  const [fresh, setFresh] = useState(true);
  const [targets, setTargets] = useState<api.ComputerTarget[]>([]);
  const [candidateId, setCandidateId] = useState("");
  const [targetRevision, setTargetRevision] = useState(0);
  const [loadingTargets, setLoadingTargets] = useState(false);
  const [targetError, setTargetError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [authorizing, setAuthorizing] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [retryingCleanup, setRetryingCleanup] = useState(false);
  const [cleanupRetryError, setCleanupRetryError] = useState<string | null>(null);
  const live = useRef(true);
  const statusRevision = useRef(0);
  const commandRevision = useRef(0);
  const commandBusy = useRef(false);
  const stopRequest = useRef<{ runId: string | null } | null>(null);
  const cleanupBusy = useRef(false);
  const pendingAuthorization = useRef<api.ComputerAuthorizationAttempt | null>(null);
  const requestedRun = useRef(runId);
  const activeRun = requestedRun.current ?? status?.runId ?? null;
  const cleanupPending = !!status?.mcpCatalog?.cleanupPending;
  const usesSystemPicker = surface === "desktop" && status?.desktopSelection === "portal";
  const controlsLocked = !status?.featureEnabled || !fresh || busy || stopping || authorizing || retryingCleanup
    || status?.stopState === "stop_requested" || !!status?.mcpCatalog?.pending || cleanupPending;

  function acceptStatus(value: api.ComputerStatus) {
    setStatus(value);
    setFresh(true);
    const request = stopRequest.current;
    if (request?.runId && value.runId === request.runId && value.stopState === "stopped") {
      // Only fresh exact-run native completion retires a lost Stop reply. An
      // empty idle status cannot confirm a session-scoped authorization fence.
      stopRequest.current = null;
      setStopping(false);
    }
  }

  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
      commandRevision.current += 1;
      const pending = pendingAuthorization.current;
      pendingAuthorization.current = null;
      if (pending) void api.computerCancelAuthorization(pending).catch(() => {});
    };
  }, []);

  useEffect(() => {
    if (!sessionId) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    let watchdog: ReturnType<typeof setTimeout>;
    async function poll() {
      const revision = ++statusRevision.current;
      watchdog = setTimeout(() => {
        if (!cancelled && revision === statusRevision.current) setFresh(false);
      }, COMPUTER_STATUS_STALE_MS);
      try {
        const value = await api.computerStatus({ sessionId, runId: requestedRun.current });
        if (!cancelled && revision === statusRevision.current) {
          acceptStatus(value);
        }
      } catch {
        if (!cancelled && revision === statusRevision.current) setFresh(false);
      } finally {
        clearTimeout(watchdog);
        if (!cancelled) timer = setTimeout(() => void poll(), 800);
      }
    }
    void poll();
    return () => { cancelled = true; clearTimeout(timer); clearTimeout(watchdog); };
  }, [sessionId, runId]);

  useEffect(() => {
    if (!sessionId || !status?.featureEnabled) return;
    if (usesSystemPicker) {
      setTargets([]);
      setCandidateId("");
      setLoadingTargets(false);
      setTargetError(null);
      return;
    }
    let retired = false;
    setLoadingTargets(true);
    setTargetError(null);
    function fail(cause: unknown) {
      if (retired) return;
      retired = true;
      clearTimeout(deadline);
      // Neither failure nor timeout proves the old candidates still exist.
      // Keep active-run control independent and require explicit reselection.
      setTargets([]);
      setCandidateId("");
      setTargetError(String(cause));
      setLoadingTargets(false);
    }
    const deadline = setTimeout(() => fail("target discovery timed out"), TARGET_DISCOVERY_TIMEOUT_MS);
    async function list() {
      if (surface === "app-webview") {
        return (await sideBrowserList()).map((page) => ({
          targetId: page.label, title: page.url || page.label,
          appName: "", kind: "webview", surface,
        }));
      }
      const rows = await api.computerListTargets({ sessionId, runId: requestedRun.current, surface });
      return rows.length === 0 && surface === "existing-tabs" ? api.computerListSharedTabs() : rows;
    }
    void list().then((rows) => {
      if (retired) return;
      retired = true;
      clearTimeout(deadline);
      setTargets(rows);
      setCandidateId((current) => rows.some((row) => row.targetId === current) ? current : "");
      setLoadingTargets(false);
    }).catch(fail);
    return () => { retired = true; clearTimeout(deadline); };
  }, [sessionId, runId, status?.featureEnabled, targetRevision, surface, usesSystemPicker]);

  async function refreshStatus(requestedRunId = requestedRun.current) {
    const revision = ++statusRevision.current;
    const watchdog = setTimeout(() => {
      if (live.current && revision === statusRevision.current) setFresh(false);
    }, COMPUTER_STATUS_STALE_MS);
    try {
      const value = await api.computerStatus({ sessionId, runId: requestedRunId });
      if (live.current && revision === statusRevision.current) acceptStatus(value);
    } catch (cause) {
      if (live.current && revision === statusRevision.current) setFresh(false);
      throw cause;
    } finally {
      clearTimeout(watchdog);
    }
  }

  async function authorize() {
    if (!sessionId || controlsLocked || commandBusy.current || loadingTargets || targetError) return;
    if (!usesSystemPicker && (!candidateId || !targets.some((target) => target.targetId === candidateId))) return;
    commandBusy.current = true;
    setBusy(true);
    setAuthorizing(true);
    setError(null);
    const revision = ++commandRevision.current;
    const randomId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${revision}`;
    const attemptId = `cu:${randomId}:${revision}`;
    pendingAuthorization.current = { sessionId, attemptId, selectorRevision: revision };
    statusRevision.current += 1;
    try {
      const result = await api.computerAuthorizeSurface({
        sessionId, runId: activeRun, attemptId, selectorRevision: revision, surface,
        targetId: usesSystemPicker ? "" : candidateId,
      });
      if (result.attemptId !== attemptId || result.selectorRevision !== revision) {
        throw new Error("stale Computer Use authorization response");
      }
      if (live.current && revision === commandRevision.current) {
        requestedRun.current = result.runId;
        await refreshStatus(result.runId);
        return live.current && revision === commandRevision.current;
      }
    } catch (cause) {
      if (live.current && revision === commandRevision.current) setError(String(cause));
    } finally {
      if (pendingAuthorization.current?.attemptId === attemptId) pendingAuthorization.current = null;
      if (revision === commandRevision.current) commandBusy.current = false;
      if (live.current && revision === commandRevision.current) { setBusy(false); setAuthorizing(false); }
    }
  }

  async function runCommand(action: "pause" | "resume" | "takeover" | "unbind") {
    if (controlsLocked || commandBusy.current || !sessionId || !activeRun) return;
    commandBusy.current = true;
    const revision = ++commandRevision.current;
    setBusy(true);
    setError(null);
    try {
      const commands = {
        pause: api.computerPause, resume: api.computerResume,
        takeover: api.computerTakeover, unbind: api.computerUnbindWebview,
      };
      await commands[action]({ sessionId, runId: activeRun });
      if (!live.current || revision !== commandRevision.current) return;
      if (action === "unbind") setCandidateId("");
      await refreshStatus();
    } catch (cause) {
      if (live.current && revision === commandRevision.current) setError(String(cause));
    } finally {
      if (revision === commandRevision.current) commandBusy.current = false;
      if (live.current && revision === commandRevision.current) setBusy(false);
    }
  }

  async function stop() {
    if (!sessionId || stopRequest.current) return;
    const request = { runId: activeRun };
    stopRequest.current = request;
    const pending = pendingAuthorization.current;
    pendingAuthorization.current = null;
    commandRevision.current += 1;
    commandBusy.current = false;
    cleanupBusy.current = false;
    statusRevision.current += 1;
    setBusy(false);
    setAuthorizing(false);
    setRetryingCleanup(false);
    setStopping(true);
    setFresh(false);
    setError(null);
    try {
      const stopped = api.computerStop({ sessionId, runId: request.runId });
      // Host Stop fences every authorization for this exact session before its
      // acknowledgement. Attempt-specific cleanup must not hold the Stop latch;
      // a lost cancellation reply is not proof that native cleanup completed.
      if (pending) void api.computerCancelAuthorization(pending).catch(() => {});
      await stopped;
    } catch (cause) {
      if (live.current && stopRequest.current === request) setError(String(cause));
    } finally {
      // A readback may already have retired this exact request. Its late reply
      // must not invalidate newer status or interfere with subsequent cleanup.
      if (live.current && stopRequest.current === request) {
        stopRequest.current = null;
        setFresh(false);
        setStopping(false);
        try { await refreshStatus(); } catch (cause) {
          if (live.current) setError((value) => value ?? String(cause));
        }
      }
    }
  }

  async function retryCleanup() {
    if (!sessionId || cleanupBusy.current || stopping || !cleanupPending) return;
    cleanupBusy.current = true;
    const revision = ++commandRevision.current;
    statusRevision.current += 1;
    setRetryingCleanup(true);
    setCleanupRetryError(null);
    setError(null);
    try {
      const mcpCatalog = await api.computerRetryCleanup({ sessionId });
      if (!live.current || revision !== commandRevision.current) return;
      setStatus((current) => current ? { ...current, mcpCatalog } : current);
      await refreshStatus();
    } catch (cause) {
      if (live.current && revision === commandRevision.current) setCleanupRetryError(String(cause).replace(/^Error:\s*/, ""));
    } finally {
      if (revision === commandRevision.current) cleanupBusy.current = false;
      if (live.current && revision === commandRevision.current) setRetryingCleanup(false);
    }
  }

  return {
    status, fresh, targets, candidateId, setCandidateId, loadingTargets, targetError, usesSystemPicker,
    error, busy, authorizing, stopping, retryingCleanup, cleanupPending,
    cleanupRetryError, activeRun, controlsLocked, authorize, runCommand, stop, retryCleanup,
    refreshTargets: () => setTargetRevision((value) => value + 1),
  };
}
