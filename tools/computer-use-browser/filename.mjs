/** Download names that must never be passed to the worker or written to staging. */
const RESERVED_DOWNLOAD = /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(\.|$)/i;

export function sanitizeDownloadFilename(raw) {
  const filename = String(raw || "download.bin").trim();
  if (
    !filename ||
    filename.length > 80 ||
    filename.includes("..") ||
    /[\\/:*?"<>|\0]/.test(filename) ||
    RESERVED_DOWNLOAD.test(filename)
  ) {
    return null;
  }
  return filename;
}
