(function () {
  var LABEL = __GROK_SBDL_LABEL__;
  if (window.__grokSbdlInstalled) {
    window.__grokSbdlLabel = LABEL;
    return;
  }
  window.__grokSbdlData = {};
  window.__grokSbdlBlobMap = new Map();
  window.__grokSbdlSeq = 1;
  window.__grokSbdlRecent = Object.create(null);

  function dedupe(key) {
    var now = Date.now();
    if (window.__grokSbdlRecent[key] && now - window.__grokSbdlRecent[key] < 2500) return true;
    window.__grokSbdlRecent[key] = now;
    return false;
  }

  try {
    var origCreate = URL.createObjectURL.bind(URL);
    var origRevoke = URL.revokeObjectURL.bind(URL);
    URL.createObjectURL = function (obj) {
      var url = origCreate(obj);
      try {
        if (obj && typeof Blob !== "undefined" && obj instanceof Blob) {
          window.__grokSbdlBlobMap.set(url, obj);
        }
      } catch (e) {}
      return url;
    };
    URL.revokeObjectURL = function (url) {
      // Keep map entry until after click handlers; delete async.
      var u = url;
      setTimeout(function () {
        try { window.__grokSbdlBlobMap.delete(u); } catch (e) {}
      }, 3000);
      return origRevoke(url);
    };
  } catch (e) {}

  function guessName(name, blob, href) {
    var n = (name && String(name).trim()) || "";
    if (n) return n;
    if (blob && blob.type) {
      var t = String(blob.type);
      if (t.indexOf("mp4") >= 0) return "download.mp4";
      if (t.indexOf("webm") >= 0) return "download.webm";
      if (t.indexOf("png") >= 0) return "download.png";
      if (t.indexOf("jpeg") >= 0 || t.indexOf("jpg") >= 0) return "download.jpg";
      if (t.indexOf("pdf") >= 0) return "download.pdf";
      if (t.indexOf("zip") >= 0) return "download.zip";
    }
    if (href) {
      try {
        var path = String(href).split("?")[0].split("#")[0];
        var base = path.split("/").pop() || "";
        if (base && base.indexOf(".") > 0) return decodeURIComponent(base);
      } catch (e) {}
    }
    return "download.bin";
  }

  function pingScheme(pathAndQuery) {
    var roots = ["sbdl://localhost", "http://sbdl.localhost"];
    for (var i = 0; i < roots.length; i++) {
      try {
        var img = new Image();
        img.src = roots[i] + pathAndQuery + "&_t=" + Date.now() + "&i=" + i;
      } catch (e) {}
    }
  }

  function signalUrlDownload(name, url) {
    var safeName = String(name || "download.bin").replace(/\|/g, "_");
    if (dedupe("url:" + url)) return;
    try {
      var prev = document.title;
      document.title = "__GROK_DL_URL__|" + safeName + "|" + url;
      setTimeout(function () { try { document.title = prev; } catch (e) {} }, 150);
    } catch (e) {}
    pingScheme(
      "/url?label=" + encodeURIComponent(LABEL) +
      "&name=" + encodeURIComponent(safeName) +
      "&u=" + encodeURIComponent(url)
    );
  }

  function signalBlobReady(id, b64Len, name) {
    var safeName = String(name || "download.bin").replace(/\|/g, "_");
    try {
      var prev = document.title;
      document.title = "__GROK_SBDL__|" + id + "|" + b64Len + "|" + safeName;
      setTimeout(function () { try { document.title = prev; } catch (e) {} }, 150);
    } catch (e) {}
    pingScheme(
      "/ready?label=" + encodeURIComponent(LABEL) +
      "&id=" + encodeURIComponent(id) +
      "&len=" + encodeURIComponent(String(b64Len)) +
      "&name=" + encodeURIComponent(safeName)
    );
  }

  function deliverBlob(blob, filename) {
    if (!blob) return;
    var name = filename || "download.bin";
    var dkey = "blob:" + name + ":" + (blob.size || 0);
    if (dedupe(dkey)) return;
    var id = String(window.__grokSbdlSeq++);
    window.__grokSbdlLast = { id: id, name: name, size: blob.size, at: Date.now(), phase: "reading" };
    try {
      var reader = new FileReader();
      reader.onloadend = function () {
        try {
          var result = reader.result;
          if (typeof result !== "string") {
            window.__grokSbdlLast.phase = "fail";
            return;
          }
          var comma = result.indexOf(",");
          var b64 = comma >= 0 ? result.slice(comma + 1) : result;
          window.__grokSbdlData[id] = b64;
          window.__grokSbdlLast.phase = "signaled";
          window.__grokSbdlLast.b64Len = b64.length;
          signalBlobReady(id, b64.length, name);
        } catch (e) {
          try { console.warn("[grok-sbdl] reader", e); } catch (e2) {}
        }
      };
      reader.onerror = function () {
        try { window.__grokSbdlLast.phase = "fail"; } catch (e) {}
      };
      reader.readAsDataURL(blob);
    } catch (e) {
      try { console.warn("[grok-sbdl] deliver", e); } catch (e2) {}
    }
  }

  function dataUrlToBlob(href) {
    var comma = href.indexOf(",");
    if (comma < 0) return null;
    var meta = href.slice(0, comma);
    var data = href.slice(comma + 1);
    var isB64 = /;base64/i.test(meta);
    var mimeMatch = /^data:([^;,]+)/i.exec(meta);
    var mime = (mimeMatch && mimeMatch[1]) || "application/octet-stream";
    if (isB64) {
      var raw = atob(data);
      var arr = new Uint8Array(raw.length);
      for (var i = 0; i < raw.length; i++) arr[i] = raw.charCodeAt(i);
      return new Blob([arr], { type: mime });
    }
    try {
      return new Blob([decodeURIComponent(data)], { type: mime });
    } catch (e) {
      return new Blob([data], { type: mime });
    }
  }

  // HTTP(S): do NOT follow redirects in-page (S3 signed URLs fail CORS with Origin null
  // in WKWebView). Prefer redirect:manual → pass Location to Rust, else Rust + cookies.
  function fetchThenDeliver(href, name) {
    if (dedupe("fetch:" + href + ":" + name)) return;
    window.__grokSbdlLast = { name: name, href: href, phase: "fetch", at: Date.now() };
    var done = false;
    function toRust(u) {
      if (done) return;
      done = true;
      signalUrlDownload(name, u || href);
    }
    try {
      fetch(href, {
        credentials: "include",
        mode: "cors",
        redirect: "manual",
        cache: "no-store"
      })
        .then(function (r) {
          // Same-origin API often 302 → S3. Location is the signed URL (no CORS on Rust).
          if (r.status >= 300 && r.status < 400) {
            var loc = r.headers.get("Location") || r.headers.get("location");
            if (loc) {
              try { loc = new URL(loc, href).href; } catch (e) {}
              window.__grokSbdlLast.phase = "redirect";
              window.__grokSbdlLast.location = loc;
              toRust(loc);
              return;
            }
          }
          // opaqueredirect / status 0
          if (r.type === "opaqueredirect" || r.status === 0) {
            toRust(href);
            return;
          }
          if (r.ok) {
            return r.blob().then(function (b) {
              done = true;
              deliverBlob(b, name);
            });
          }
          toRust(href);
        })
        .catch(function (err) {
          try { console.warn("[grok-sbdl] page fetch", err); } catch (e) {}
          toRust(href);
        });
    } catch (e) {
      toRust(href);
    }
  }

  function handleHref(href, filename, force) {
    if (!href || typeof href !== "string") return false;
    var name = guessName(filename, null, href);

    if (href.indexOf("blob:") === 0) {
      var blob = window.__grokSbdlBlobMap.get(href);
      name = guessName(filename, blob, href);
      if (blob) {
        deliverBlob(blob, name);
        return true;
      }
      try {
        fetch(href)
          .then(function (r) { return r.blob(); })
          .then(function (b) { deliverBlob(b, name); })
          .catch(function (err) {
            try { console.warn("[grok-sbdl] blob fetch", err); } catch (e) {}
          });
        return true;
      } catch (e) {
        return false;
      }
    }

    if (href.indexOf("data:") === 0) {
      try {
        var b = dataUrlToBlob(href);
        if (!b) return false;
        deliverBlob(b, name);
        return true;
      } catch (e) {
        return false;
      }
    }

    if (href.indexOf("https://") === 0 || href.indexOf("http://") === 0) {
      var low = href.toLowerCase();
      var pathLooksFile =
        low.indexOf(".mp4") >= 0 || low.indexOf(".webm") >= 0 || low.indexOf(".mov") >= 0 ||
        low.indexOf(".zip") >= 0 || low.indexOf(".pdf") >= 0 || low.indexOf(".png") >= 0 ||
        low.indexOf(".jpg") >= 0 || low.indexOf(".jpeg") >= 0 || low.indexOf(".gif") >= 0 ||
        low.indexOf(".webp") >= 0 || low.indexOf(".bin") >= 0 || low.indexOf(".wav") >= 0 ||
        low.indexOf(".mp3") >= 0 || low.indexOf(".m4a") >= 0;
      var looksApiDownload =
        low.indexOf("/download") >= 0 || low.indexOf("/export") >= 0 ||
        low.indexOf("/render") >= 0 || low.indexOf("/jobs/") >= 0;
      if (force || pathLooksFile || looksApiDownload) {
        fetchThenDeliver(href, name);
        return true;
      }
    }
    return false;
  }

  function tryAnchor(a) {
    if (!a || !a.tagName || String(a.tagName).toUpperCase() !== "A") return false;
    // The isolated Host kernel sets this only around its exact native click.
    // It skips our manual bridge; it never grants a CU destination or run.
    if (a.getAttribute("data-grok-native-download") === "1") return false;
    if (a.getAttribute("data-grok-sbdl") === "1") return true;
    var href = "";
    try { href = a.href || a.getAttribute("href") || ""; } catch (e) {
      href = a.getAttribute("href") || "";
    }
    var name = "";
    try { name = a.getAttribute("download") || a.download || ""; } catch (e) {}
    var hasDl =
      a.hasAttribute("download") ||
      (typeof a.download === "string" && a.download !== "") ||
      href.indexOf("blob:") === 0 ||
      href.indexOf("data:") === 0;
    if (!hasDl) return false;
    var ok = handleHref(href, name, true);
    if (ok) {
      try { a.setAttribute("data-grok-sbdl", "1"); } catch (e) {}
    }
    return ok;
  }

  function tryIframeSrc(src) {
    if (!src || typeof src !== "string") return false;
    if (src === "about:blank" || src.indexOf("javascript:") === 0) return false;
    // ChatCut Xx() fallback: hidden iframe to download URL
    return handleHref(src, "", true);
  }

  // --- hooks ---
  document.addEventListener(
    "click",
    function (ev) {
      try {
        var t = ev.target;
        if (!t || !t.closest) return;
        var a = t.closest("a");
        if (!a) return;
        if (tryAnchor(a)) {
          ev.preventDefault();
          ev.stopPropagation();
          if (ev.stopImmediatePropagation) ev.stopImmediatePropagation();
        }
      } catch (e) {}
    },
    true
  );

  try {
    var aProto = HTMLAnchorElement.prototype;
    var origAClick = aProto.click;
    aProto.click = function () {
      try {
        if (tryAnchor(this)) return;
      } catch (e) {}
      return origAClick.apply(this, arguments);
    };
  } catch (e) {}

  // Critical: ChatCut does appendChild(a); a.click(); revoke — handle on append
  // BEFORE click/revoke. Also catch iframe download fallback.
  try {
    var origAppend = Node.prototype.appendChild;
    Node.prototype.appendChild = function (child) {
      try {
        if (child && child.tagName) {
          var tag = String(child.tagName).toUpperCase();
          if (tag === "A") {
            tryAnchor(child);
          } else if (tag === "IFRAME") {
            var s = "";
            try { s = child.src || child.getAttribute("src") || ""; } catch (e) {}
            if (s && tryIframeSrc(s)) {
              try { child.removeAttribute("src"); child.src = "about:blank"; } catch (e2) {}
            }
          }
        }
      } catch (e) {}
      return origAppend.apply(this, arguments);
    };
  } catch (e) {}

  try {
    var origInsert = Node.prototype.insertBefore;
    Node.prototype.insertBefore = function (child, ref) {
      try {
        if (child && child.tagName) {
          var tag = String(child.tagName).toUpperCase();
          if (tag === "A") tryAnchor(child);
          else if (tag === "IFRAME") {
            var s = "";
            try { s = child.src || child.getAttribute("src") || ""; } catch (e) {}
            if (s && tryIframeSrc(s)) {
              try { child.removeAttribute("src"); child.src = "about:blank"; } catch (e2) {}
            }
          }
        }
      } catch (e) {}
      return origInsert.apply(this, arguments);
    };
  } catch (e) {}

  // iframe.src = url (set after create)
  try {
    var iframeProto = HTMLIFrameElement.prototype;
    var srcDesc = Object.getOwnPropertyDescriptor(iframeProto, "src") ||
      Object.getOwnPropertyDescriptor(HTMLElement.prototype, "src");
    if (srcDesc && srcDesc.set) {
      Object.defineProperty(iframeProto, "src", {
        configurable: true,
        enumerable: true,
        get: srcDesc.get,
        set: function (v) {
          try {
            if (v && tryIframeSrc(String(v))) {
              return srcDesc.set.call(this, "about:blank");
            }
          } catch (e) {}
          return srcDesc.set.call(this, v);
        }
      });
    }
  } catch (e) {}

  window.__grokSbdlInstalled = true;
  window.__grokSbdlLabel = LABEL;
  try { console.info("[grok-sbdl] ready v3", LABEL); } catch (e) {}
})();
