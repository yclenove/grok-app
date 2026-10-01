// Test lifecycle helper only. Never replay actions or infer physical success
// from an expired request. Preserve the original total HTTP cleanup budget.
export async function confirmWorkerShutdown(request, { budgetMs = 20000, onPending = () => {} } = {}) {
  const deadline = performance.now() + budgetMs;
  const remaining = () => Math.max(1, Math.ceil(deadline - performance.now()));
  const pending = reply => reply?.status === 409 && reply.body?.error?.code === "run_cleanup_pending"
    && reply.body.error.completion === "unknown";
  let reply = await request("/shutdown", remaining());
  if (!pending(reply)) return reply;
  onPending();
  while (performance.now() < deadline) {
    const health = await request("/health", remaining());
    if (health.status !== 200 || !Array.isArray(health.body?.openProfiles)) return reply;
    if (health.body.openProfiles.length === 0) {
      // Empty inventory alone is not completion: the production shutdown must
      // still confirm that admitted launches and all physical work are idle.
      reply = await request("/shutdown", remaining());
      if (!pending(reply)) return reply;
    }
    if (performance.now() < deadline) await new Promise(resolve => setTimeout(resolve, Math.min(100, remaining())));
  }
  return reply;
}

// The same observation contract applies to a single stopped run. An idle
// snapshot alone is not success, and an unrelated revision is never accepted.
export async function confirmRunCancellation(request, identity, { budgetMs = 20000, onPending = () => {} } = {}) {
  const deadline = performance.now() + budgetMs;
  const remaining = () => Math.max(1, Math.ceil(deadline - performance.now()));
  const pending = reply => reply?.status === 409 && reply.body?.error?.code === "run_cleanup_pending"
    && reply.body.error.completion === "unknown";
  let reply = await request("/cancel-run", { owner: identity.owner }, remaining());
  if (!pending(reply)) return reply;
  onPending();
  while (performance.now() < deadline) {
    const status = await request("/run-status", identity, remaining());
    if (status.status !== 200 || status.body?.runRevision !== identity.runRevision ||
      status.body?.phase !== "stopped" || typeof status.body?.idle !== "boolean" ||
      !Number.isSafeInteger(status.body?.activeOperations) || status.body.activeOperations < 0) return reply;
    if (status.body.idle && status.body.activeOperations === 0) {
      reply = await request("/cancel-run", { owner: identity.owner }, remaining());
      if (!pending(reply)) return reply;
    }
    if (performance.now() < deadline) await new Promise(resolve => setTimeout(resolve, Math.min(100, remaining())));
  }
  return reply;
}
