(function () {
  try {
    var url = __URL__;
    var key = url;
    var jobs = window.__GROK_APP_ALBUM_THUMB_JOBS__ ||
      (window.__GROK_APP_ALBUM_THUMB_JOBS__ = Object.create(null));
    if (jobs[key] && jobs[key].state === "loading") {
      return JSON.stringify({ state: "loading" });
    }
    var controller = typeof AbortController === "function" ? new AbortController() : null;
    jobs[key] = { state: "loading", controller: controller };
    var timer = window.setTimeout(function () {
      try { if (controller) controller.abort(); } catch (_) {}
    }, 12000);

    function fail() {
      window.clearTimeout(timer);
      jobs[key] = { state: "error" };
    }

    function encode(source, width, height) {
      try {
        if (!width || !height || width > 32768 || height > 32768) return fail();
        var scale = Math.min(1, 480 / Math.max(width, height));
        var outWidth = Math.max(1, Math.round(width * scale));
        var outHeight = Math.max(1, Math.round(height * scale));
        var canvas = document.createElement("canvas");
        canvas.width = outWidth;
        canvas.height = outHeight;
        var context = canvas.getContext("2d", { alpha: false });
        if (!context) return fail();
        context.drawImage(source, 0, 0, outWidth, outHeight);
        var dataUrl = canvas.toDataURL("image/jpeg", 0.78);
        window.clearTimeout(timer);
        if (!dataUrl || dataUrl.indexOf("data:image/jpeg;base64,") !== 0 ||
            dataUrl.length > __MAX_DATA_URL__) return fail();
        jobs[key] = {
          state: "ready",
          dataUrl: dataUrl,
          width: width,
          height: height
        };
      } catch (_) {
        fail();
      }
    }

    fetch(url, {
      credentials: "include",
      mode: "cors",
      cache: "force-cache",
      redirect: "follow",
      signal: controller ? controller.signal : undefined
    })
      .then(function (response) {
        if (!response.ok) throw new Error("status");
        var finalUrl = new URL(response.url || url, location.href);
        if (finalUrl.protocol !== "https:" || finalUrl.hostname !== "assets.grok.com" ||
            (finalUrl.port && finalUrl.port !== "443") || finalUrl.username ||
            finalUrl.password || finalUrl.pathname.indexOf("/generated/") < 0) {
          throw new Error("redirect");
        }
        var type = String(response.headers.get("content-type") || "").toLowerCase();
        var length = Number(response.headers.get("content-length") || 0);
        if (type.indexOf("image/") !== 0 || length > __MAX_SOURCE_BYTES__) {
          throw new Error("type");
        }
        return response.blob();
      })
      .then(function (blob) {
        if (!blob || !blob.size || blob.size > __MAX_SOURCE_BYTES__) {
          throw new Error("size");
        }
        if (typeof createImageBitmap === "function") {
          return createImageBitmap(blob).then(function (bitmap) {
            encode(bitmap, bitmap.width, bitmap.height);
            try { bitmap.close(); } catch (_) {}
          });
        }
        return new Promise(function (resolve, reject) {
          var objectUrl = URL.createObjectURL(blob);
          var image = new Image();
          image.onload = function () {
            try {
              encode(image, image.naturalWidth, image.naturalHeight);
              resolve();
            } finally {
              URL.revokeObjectURL(objectUrl);
            }
          };
          image.onerror = function () {
            URL.revokeObjectURL(objectUrl);
            reject(new Error("decode"));
          };
          image.src = objectUrl;
        });
      })
      .catch(fail);
    return JSON.stringify({ state: "loading" });
  } catch (_) {
    return JSON.stringify({ state: "error" });
  }
})()
