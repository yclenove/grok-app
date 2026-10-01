// Chrome captures a window's active tab, not a tab ID. SharedTabs must fence this
// call on both sides. This module never activates tabs or broadens permissions.
const sourceBytes = 24 * 1024 * 1024;
const base64Cap = 400000;

export async function normalizeCapture(dataUrl, view, live = () => true) {
  const deadline = performance.now() + 4000;
  const check = () => { if (!live() || performance.now() >= deadline) throw new Error("capture cancelled"); };
  check();
  if (typeof dataUrl !== "string" || !dataUrl.startsWith("data:image/png;base64,") || dataUrl.length > sourceBytes) {
    throw new Error("capture unavailable");
  }
  const raw = atob(dataUrl.slice(22));
  const bytes = Uint8Array.from(raw, char => char.charCodeAt(0));
  const header = new DataView(bytes.buffer);
  if (bytes.length < 33 || header.getUint32(0) !== 0x89504e47 || header.getUint32(4) !== 0x0d0a1a0a
    || header.getUint32(8) !== 13 || header.getUint32(12) !== 0x49484452) throw new Error("invalid capture");
  const sourceWidth = header.getUint32(16); const sourceHeight = header.getUint32(20);
  if (!sourceWidth || !sourceHeight || sourceWidth > 8192 || sourceHeight > 8192 || sourceWidth * sourceHeight > 16777216
    || !Number.isFinite(view.pixelRatio) || view.pixelRatio <= 0 || view.pixelRatio > 8
    || view.visualScale !== 1 || view.visualX !== 0 || view.visualY !== 0
    || Math.abs(sourceWidth - view.viewportWidth * view.pixelRatio) > 2
    || Math.abs(sourceHeight - view.viewportHeight * view.pixelRatio) > 2) throw new Error("capture geometry changed");
  const bitmap = await createImageBitmap(new Blob([bytes], { type: "image/png" }));
  try {
    check();
    if (bitmap.width !== sourceWidth || bitmap.height !== sourceHeight) throw new Error("invalid capture");
    let edge = Math.min(1280, Math.max(sourceWidth, sourceHeight));
    for (let attempt = 0; attempt < 7; attempt++, edge = Math.floor(edge * 0.8)) {
      check();
      const scale = edge / Math.max(sourceWidth, sourceHeight);
      const width = Math.max(1, Math.round(sourceWidth * scale));
      const height = Math.max(1, Math.round(sourceHeight * scale));
      const canvas = new OffscreenCanvas(width, height);
      const context = canvas.getContext("2d", { alpha: false });
      if (!context) throw new Error("capture unavailable");
      context.drawImage(bitmap, 0, 0, width, height);
      const blob = await canvas.convertToBlob({ type: "image/png" });
      check();
      // Bound base64 before allocating it. Never ship an unbounded source image.
      if (Math.ceil(blob.size / 3) * 4 <= base64Cap) {
        const output = new Uint8Array(await blob.arrayBuffer());
        check();
        let binary = "";
        for (let offset = 0; offset < output.length; offset += 8192) binary += String.fromCharCode(...output.subarray(offset, offset + 8192));
        return { pngBase64: btoa(binary), width, height, sourceWidth, sourceHeight };
      }
    }
    throw new Error("capture too large");
  } finally { bitmap.close(); }
}
