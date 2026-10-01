import { copyCompletionProof, exactKeys, uuid } from "./completion-scope.mjs";
import { parseEndpoint } from "./loopback-endpoint.mjs";

export function copyHostLifetime(value, instanceId = value?.instanceId) {
  if (!exactKeys(value, ["instanceId", "platform", "scope", "pid", "birth"])
    || !uuid(value.instanceId) || value.instanceId !== instanceId
    || !["windows", "macos", "linux"].includes(value.platform)
    || !Number.isInteger(value.pid) || value.pid <= 0 || value.pid > 2147483647
    || ![value.scope, value.birth].every(stamp => typeof stamp === "string" && /^[a-f0-9]{1,128}$/i.test(stamp))) {
    throw new Error("hostLifetimeUnavailable");
  }
  return Object.freeze({ ...value });
}

// This witness is captured only from the original Host after full completion-proof authentication.
// The trusted durable cleanup journal stores it; it never enters a page, model
// message, or user-visible diagnostic. It cannot grant input/read authority.
export class HostLifetimeClient {
  constructor({ client, fetch = (...args) => globalThis.fetch(...args) }) { this.client = client; this.fetch = fetch; }

  async capture(endpoint, proof) {
    const copied = copyCompletionProof(proof);
    let reader;
    const controller = new AbortController(); const timeout = setTimeout(() => controller.abort(), 5000);
    try {
      const response = await this.fetch(parseEndpoint(endpoint) + "/cu/extension-completion/host-lifetime", {
        method: "POST", credentials: "omit", cache: "no-store", redirect: "error",
        headers: { "content-type": "application/json" }, body: JSON.stringify(copied), signal: controller.signal,
      });
      if (!response.ok) throw new Error("hostLifetimeUnavailable");
      reader = response.body.getReader(); const chunks = []; let size = 0;
      for (;;) {
        const { value, done } = await reader.read();
        if (controller.signal.aborted) throw new Error("hostLifetimeUnavailable");
        if (done) break;
        size += value.byteLength;
        if (size > 1024) throw new Error("hostLifetimeUnavailable");
        chunks.push(value);
      }
      const bytes = new Uint8Array(size); let offset = 0;
      for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.byteLength; }
      const reply = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
      if (!exactKeys(reply, ["ok", "lifetime"]) || reply.ok !== true) throw new Error("hostLifetimeUnavailable");
      return copyHostLifetime(reply.lifetime, copied.binding.connection.instanceId);
    } catch { throw new Error("hostLifetimeUnavailable"); }
    finally { clearTimeout(timeout); await reader?.cancel().catch(() => {}); }
  }

  retired(lifetime) { return this.client.retiredHost(copyHostLifetime(lifetime)); }
}
