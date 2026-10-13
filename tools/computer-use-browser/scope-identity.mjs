// Host-only instance metadata. Never invoke pageResult/capture/title/url here:
// ordinary navigation changes document authority, not the approved tab instance.
import { workerException } from "./worker-errors.mjs";

export function managedScopeIdentity(got, pageId, workerInstanceId) {
  if (typeof pageId !== "string" || !pageId || pageId.length > 128) {
    throw workerException(400, "invalid_page_id", "valid pageId required");
  }
  const state = got.slot.pages.get(pageId);
  if (!state || !state.published || state.closed || state.identityExhausted ||
      state.page.isClosed() || !got.slot.context.pages().includes(state.page)) {
    throw workerException(404, "page_closed", "page is closed or unknown");
  }
  got.slot.closure.check();
  return { ok: true, owner: got.owner, profile: got.profile, workerInstanceId,
    contextInstanceId: got.slot.contextInstanceId, pageId: state.id };
}
