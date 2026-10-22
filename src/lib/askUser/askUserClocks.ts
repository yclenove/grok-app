/**
 * Auto-cancel clocks for `_x.ai/ask_user_question`.
 *
 * Shared across mounts of the composer gate (and the settings demo modal).
 * Switching chats unmounts the UI; resuming the same request must not reset
 * the deadline.
 */
import { dropGateClock } from "../gateClock";

const askUserClocks = new Map<string, number>();
const pausedClocks = new Set<string>();

export function getAskUserClocks(): Map<string, number> {
  return askUserClocks;
}

/** Drop auto-cancel clocks for every AskUser request on `sessionId`. */
export function dropAskUserClocks(sessionId: string): void {
  const prefix = `${sessionId}:`;
  for (const key of new Set([...askUserClocks.keys(), ...pausedClocks])) {
    if (key.startsWith(prefix) || key === sessionId) {
      dropAskUserClock(key);
    }
  }
}

export function dropAskUserClock(key: string): void {
  dropGateClock(askUserClocks, key);
  pausedClocks.delete(key);
}

/** A failed reply stays retryable, without repeating an expired automatic cancel. */
export function pauseAskUserClock(key: string): void {
  pausedClocks.add(key);
}

export function isAskUserClockPaused(key: string): boolean {
  return pausedClocks.has(key);
}
