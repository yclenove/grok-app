import { execFile } from "node:child_process";
import { readFile, readlink } from "node:fs/promises";
import { isAbsolute, join, resolve, win32 } from "node:path";
import { promisify } from "node:util";

const runFile = promisify(execFile);

// CommandLineToArgvW quoting rules for ordinary Chromium argv. Reject an
// unterminated quote instead of guessing where a profile argument ends.
export function windowsArguments(line) {
  if (typeof line !== "string" || line.includes("\0")) return null;
  const args = [];
  let i = 0;
  while (i < line.length) {
    while (line[i] === " " || line[i] === "\t") i += 1;
    if (i === line.length) break;
    let arg = "", quoted = false;
    while (i < line.length && (quoted || (line[i] !== " " && line[i] !== "\t"))) {
      let slashes = 0;
      while (line[i] === "\\") { slashes += 1; i += 1; }
      if (line[i] === '"') {
        arg += "\\".repeat(Math.floor(slashes / 2));
        if (slashes % 2) arg += '"';
        else if (quoted && line[i + 1] === '"') { arg += '"'; i += 1; }
        else quoted = !quoted;
        i += 1;
      } else {
        arg += "\\".repeat(slashes);
        if (!quoted && (line[i] === " " || line[i] === "\t")) break;
        if (i < line.length) arg += line[i++];
      }
    }
    if (quoted) return null;
    args.push(arg);
  }
  return args;
}

export function ownedBrowserPid(rows, { parentPid, executablePath, userDataDir, platform }) {
  const path = platform === "win32" ? win32 : { isAbsolute, resolve };
  const normal = value => typeof value === "string" && path.isAbsolute(value)
    ? (platform === "win32" ? path.resolve(value).toLowerCase() : path.resolve(value)) : null;
  const exe = normal(executablePath), profile = normal(userDataDir);
  if (!exe || !profile || !Number.isSafeInteger(parentPid) || parentPid <= 0 || !Array.isArray(rows)) return null;
  const matches = rows.filter(row => {
    if (!row || !Number.isInteger(row.pid) || row.pid <= 0 || row.pid >= 2 ** 32 ||
      row.pid === parentPid || row.parentPid !== parentPid || normal(row.executablePath) !== exe ||
      !Array.isArray(row.args) || row.args.some(arg => typeof arg !== "string")) return false;
    const directories = row.args.filter(arg => arg.startsWith("--user-data-dir="));
    return !row.args.some(arg => arg === "--user-data-dir" || arg === "--type" || arg.startsWith("--type=")) &&
      directories.length === 1 && normal(directories[0].slice("--user-data-dir=".length)) === profile;
  });
  return matches.length === 1 ? matches[0].pid : null;
}

export async function discoverOwnedBrowserPid(launch) {
  if (!launch || !isAbsolute(launch.executablePath || "") || !isAbsolute(launch.userDataDir || "")) return null;
  const parentPid = process.pid;
  let rows;
  try {
    if (process.platform === "win32") {
      const systemRoot = process.env.SystemRoot || process.env.WINDIR;
      if (!systemRoot || !isAbsolute(systemRoot)) return null;
      const { stdout } = await runFile(join(systemRoot, "System32", "WindowsPowerShell", "v1.0", "powershell.exe"), [
        "-NoProfile", "-NonInteractive", "-Command",
        `[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); @(Get-CimInstance Win32_Process -Filter "ParentProcessId=${parentPid}" | Where-Object { $_.ProcessId -ne $PID } | Select-Object ProcessId,ParentProcessId,ExecutablePath,CommandLine) | ConvertTo-Json -Compress`,
      ], { encoding: "utf8", windowsHide: true, timeout: 2000, maxBuffer: 256 * 1024 });
      const parsed = stdout.trim() ? JSON.parse(stdout) : [];
      rows = (Array.isArray(parsed) ? parsed : [parsed]).map(row => ({
        pid: row.ProcessId, parentPid: row.ParentProcessId, executablePath: row.ExecutablePath,
        args: windowsArguments(row.CommandLine),
      }));
    } else if (process.platform === "linux") {
      const children = await readFile(`/proc/${parentPid}/task/${parentPid}/children`, "utf8");
      rows = await Promise.all(children.trim().split(/\s+/).filter(value => /^\d+$/.test(value)).map(async value => {
        try {
          const pid = Number(value);
          const [executablePath, command, status] = await Promise.all([
            readlink(`/proc/${pid}/exe`), readFile(`/proc/${pid}/cmdline`, "utf8"), readFile(`/proc/${pid}/status`, "utf8"),
          ]);
          return { pid, executablePath, parentPid: Number(/^PPid:\s+(\d+)$/m.exec(status)?.[1]),
            args: command.replace(/\0$/, "").split("\0") };
        } catch { return {}; }
      }));
    } else {
      // No heuristic `ps` argument splitting on macOS: an unknown identity is
      // preferable to borrowing another profile's process. Native lifetime
      // ownership remains the supervisor's responsibility on every platform.
      return null;
    }
    return ownedBrowserPid(rows, { ...launch, parentPid, platform: process.platform });
  } catch {
    return null;
  }
}
