import assert from "node:assert/strict";

// Real Chrome PNG encoder/decoder and OffscreenCanvas in a trusted extension page.
// MV3 service workers prohibit dynamic import; product imports this module statically.
// Synthetic pixels exercise
// the normalizer only; this does not claim captureVisibleTab/activeTab acceptance.
export async function verifyCaptureNormalization(page) {
  const result = await page.evaluate(async () => {
    const { normalizeCapture } = await import(chrome.runtime.getURL("capture-document.mjs"));
    const canvas = new OffscreenCanvas(1600, 900);
    const context = canvas.getContext("2d");
    const dataUrl = async () => {
      const bytes = new Uint8Array(await (await canvas.convertToBlob({ type: "image/png" })).arrayBuffer());
      let binary = "";
      for (let i = 0; i < bytes.length; i += 8192) binary += String.fromCharCode(...bytes.subarray(i, i + 8192));
      return "data:image/png;base64," + btoa(binary);
    };
    const view = { viewportWidth: 800, viewportHeight: 450, pixelRatio: 2, visualScale: 1, visualX: 0, visualY: 0 };
    context.fillStyle = "#e02010"; context.fillRect(0, 0, 1600, 900);
    const source = await dataUrl();
    const solid = await normalizeCapture(source, view);
    const decoded = await createImageBitmap(new Blob([Uint8Array.from(atob(solid.pngBase64), c => c.charCodeAt(0))], { type: "image/png" }));
    const check = new OffscreenCanvas(decoded.width, decoded.height);
    const checkContext = check.getContext("2d"); checkContext.drawImage(decoded, 0, 0);
    const pixel = [...checkContext.getImageData(100, 100, 1, 1).data]; decoded.close();
    const rejected = [];
    for (const invalid of [{ ...view, viewportWidth: 8010 }, { ...view, visualScale: 2 }, { ...view, visualY: 10 }]) {
      try { await normalizeCapture(source, invalid); rejected.push(false); } catch { rejected.push(true); }
    }
    // Deterministic high-entropy source forces byte-based downscaling after edge scaling.
    const pixels = context.createImageData(1600, 900); let random = 17;
    for (let i = 0; i < pixels.data.length; i += 4) {
      for (let channel = 0; channel < 3; channel++) {
        random = (Math.imul(random, 1664525) + 1013904223) >>> 0;
        pixels.data[i + channel] = random >>> 24;
      }
      pixels.data[i + 3] = 255;
    }
    context.putImageData(pixels, 0, 0);
    const noise = await normalizeCapture(await dataUrl(), view);
    return { solid: { width: solid.width, height: solid.height, bytes: solid.pngBase64.length, pixel },
      noise: { width: noise.width, height: noise.height, bytes: noise.pngBase64.length }, rejected };
  });
  assert.deepEqual(result.solid.pixel, [224, 32, 16, 255]);
  assert.equal(result.solid.width, 1280); assert.equal(result.solid.height, 720);
  assert(result.solid.bytes <= 400000);
  assert(result.noise.width < 1280 && result.noise.bytes <= 400000);
  assert(Math.abs(result.noise.width / result.noise.height - 1600 / 900) < 0.01);
  assert(result.rejected.every(Boolean));
  return result;
}
