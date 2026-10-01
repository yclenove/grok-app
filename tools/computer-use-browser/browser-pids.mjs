import { execFileSync, spawnSync } from "node:child_process";
import { isAbsolute, join } from "node:path";
import { discoverOwnedBrowserPid } from "./owned-browser-process.mjs";

export function childPids(parentPid) {
  if (!Number.isInteger(parentPid) || parentPid <= 0) return [];
  try {
    if (process.platform === "win32") {
      const systemRoot = process.env.SystemRoot || process.env.WINDIR;
      if (!systemRoot || !isAbsolute(systemRoot)) return [];
      const raw = execFileSync(
        join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe"),
        [
          "-NoProfile",
          "-NonInteractive",
          "-Command",
          `Get-CimInstance Win32_Process -Filter "ParentProcessId=${parentPid}" | Where-Object { $_.ProcessId -ne $PID } | Select-Object -ExpandProperty ProcessId`,
        ],
        { encoding: "utf8", timeout: 5000, windowsHide: true },
      );
      return raw
        .split(/\s+/)
        .map(Number)
        .filter((n) => Number.isInteger(n) && n > 0);
    }
    const mac = process.platform === "darwin";
    const query = spawnSync("/bin/ps", mac ? ["-axo", "pid=,ppid="] : ["-o", "pid=", "--ppid", String(parentPid)], {
      encoding: "utf8",
      timeout: 3000,
    });
    if (query.error || query.status !== 0) return [];
    return query.stdout.trim().split(/\r?\n/).map(line => line.trim().split(/\s+/).map(Number))
      .filter(row => !mac || row[1] === parentPid).map(row => row[0])
      .filter(pid => Number.isInteger(pid) && pid > 0 && pid !== query.pid);
  } catch {
    return [];
  }
}

export function browserPidLookup(context, launch) {
  let pid = null, pending = null, closed = false;
  context.once("close", () => { closed = true; });
  const identity = Object.freeze({ ...launch });
  return () => {
    if (pid || closed) return Promise.resolve(pid);
    if (!pending) {
      pending = captureBrowserPid(context, identity).then(value => {
        if (!closed) pid = value;
        return pid;
      }).finally(() => { pending = null; });
    }
    // Share an in-flight read, but never cache an unavailable diagnostic forever.
    // Every later read still uses this exact original context and launch tuple.
    return pending;
  };
}

export async function captureBrowserPid(context, launch) {
  // Playwright's persistent Browser has no public process() method. Ask this
  // exact owned browser over its private CDP connection when available; for a
  // persistent context, require the kernel parent, executable and exact profile.
  // A numeric PID at capture time is not lifetime-safe termination authority.
  let timer;
  const query = async () => {
    let session;
    try {
      const browser = context.browser();
      if (!browser) return await discoverOwnedBrowserPid(launch);
      session = await browser.newBrowserCDPSession();
      const result = await session.send("SystemInfo.getProcessInfo");
      if (!Array.isArray(result?.processInfo)) return null;
      const rows = result.processInfo.filter(row => row?.type === "browser");
      if (rows.length !== 1) return null;
      const pid = rows[0].id;
      return Number.isInteger(pid) && pid > 0 && pid < 2 ** 32 && pid !== process.pid ? pid : null;
    } catch {
      return null;
    } finally {
      // A late attach is still detached after the diagnostic budget expires.
      if (session) await session.detach().catch(() => {});
    }
  };
  try {
    return await Promise.race([
      query(),
      new Promise(resolve => { timer = setTimeout(() => resolve(null), 2500); }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}
