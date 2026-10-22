export type SessionGateRestoreLease = {
  isCurrent: () => boolean;
  finish: () => void;
};

/** A Map whose mutations retire outstanding restores, including set/delete ABA.
 * Only active restores hold tokens; empty-cache invalidations leave no tombstones. */
export class SessionGateCache<T> extends Map<string, T> {
  #restores = new Map<string, object>();

  constructor(entries?: Iterable<readonly [string, T]> | null) {
    // Map(entries) calls the overridden set before subclass fields exist.
    super();
    if (entries) for (const [key, value] of entries) super.set(key, value);
  }

  override set(sessionId: string, value: T): this {
    this.#restores.delete(sessionId);
    return super.set(sessionId, value);
  }

  override delete(sessionId: string): boolean {
    this.#restores.delete(sessionId);
    return super.delete(sessionId);
  }

  override clear(): void {
    this.#restores.clear();
    super.clear();
  }

  /** The last restore for a chat wins even when it returns null or is disabled. */
  beginRestore(sessionId: string): SessionGateRestoreLease {
    const token = {};
    this.#restores.set(sessionId, token);
    const isCurrent = () => this.#restores.get(sessionId) === token;
    return {
      isCurrent,
      finish: () => { if (isCurrent()) this.#restores.delete(sessionId); },
    };
  }
}
