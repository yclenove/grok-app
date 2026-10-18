//! Fixed JavaScript installed by the host. User source is never concatenated here.

/// Compile user source as an async function body. `__src` is a string value.
pub(crate) const COMPILE: &str = r#"
(function () {
  var AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
  globalThis.__user = new AsyncFunction("cu", "emit", globalThis.__src);
  delete globalThis.__src;
})()
"#;

/// Remove eval and constructor escapes after the user function object exists.
pub(crate) const LOCKDOWN: &str = r#"
(function () {
  var trap = function () { throw new Error("disabled"); };
  function lock(ctor) {
    if (typeof ctor !== "function" || !ctor.prototype) throw new Error("lock");
    Object.defineProperty(ctor.prototype, "constructor", {
      value: trap, writable: false, configurable: false
    });
  }
  lock(Function);
  lock(Object.getPrototypeOf(async function () {}).constructor);
  var names = ["eval", "Function", "process", "require", "os", "std", "fs", "Deno", "fetch", "setTimeout", "setInterval", "setImmediate", "queueMicrotask"];
  for (var i = 0; i < names.length; i++) {
    try { delete globalThis[names[i]]; } catch (e) {}
    try {
      Object.defineProperty(globalThis, names[i], {
        value: undefined, writable: false, configurable: false
      });
    } catch (e2) {}
  }
})()
"#;

/// Install `cu` / `emit`, drop host bindings, then run the compiled user function.
///
/// Native functions are captured in this closure before they are deleted from
/// `globalThis`. User source is not part of this string.
pub(crate) const RUNNER: &str = r#"
await (async function (user) {
  var appFn = globalThis.__cu_app;
  var stateFn = globalThis.__cu_state;
  var setFn = globalThis.__cu_set;
  var pressFn = globalThis.__cu_press;
  var clickFn = globalThis.__cu_click;
  var typeFn = globalThis.__cu_type;
  var waitElFn = globalThis.__cu_wait_el;
  var waitTextFn = globalThis.__cu_wait_text;
  var emitFn = globalThis.__cu_emit;
  var finish = globalThis.__cu_finish;
  var fail = globalThis.__cu_fail;
  function rejected(raw) {
    return typeof raw === "string" && raw.indexOf("ERR:") === 0;
  }
  function unwrap(raw) {
    if (rejected(raw)) throw new Error(raw.slice(4));
    return JSON.parse(raw);
  }
  function wrap(raw) {
    var state = unwrap(raw);
    state.find = function (query) {
      var want = "";
      var name = null;
      if (query && query.role != null) {
        want = String(query.role).split("_").join(" ").toLowerCase();
      }
      if (query && query.name != null) name = String(query.name);
      var elements = state.elements || [];
      for (var i = 0; i < elements.length; i++) {
        var element = elements[i];
        var role = String(element.role || "").split("_").join(" ").toLowerCase();
        if (want && role !== want) continue;
        if (name != null && String(element.name || "") !== name) continue;
        return element;
      }
      return null;
    };
    return state;
  }
  function appOf(name) {
    var raw = appFn(String(name));
    if (rejected(raw)) throw new Error(raw.slice(4));
    return {
      state: function (opts) {
        var shot = !(opts && opts.screenshot === false);
        return wrap(stateFn(String(name), shot));
      },
      setValue: function (field, value) {
        var index = field && field.index != null ? field.index : 0;
        return wrap(setFn(String(name), index, String(value)));
      },
      press: function (key) {
        return wrap(pressFn(String(name), String(key)));
      },
      click: function (field) {
        var index = field && field.index != null ? field.index : 0;
        return wrap(clickFn(String(name), index));
      },
      typeText: function (text, field) {
        var index = field && field.index != null ? field.index : -1;
        return wrap(typeFn(String(name), String(text), index));
      },
      waitFor: function (spec) {
        spec = spec || {};
        var timeout = spec.timeout_ms || 5000;
        if (spec.element) {
          return wrap(waitElFn(
            String(name),
            String(spec.element.role || ""),
            spec.element.name == null ? "" : String(spec.element.name),
            timeout
          ));
        }
        if (spec.text_present != null) {
          return wrap(waitTextFn(String(name), String(spec.text_present), true, timeout));
        }
        if (spec.text_absent != null) {
          return wrap(waitTextFn(String(name), String(spec.text_absent), false, timeout));
        }
        throw new Error("waitFor needs element, text_present, or text_absent");
      }
    };
  }
  var cu = { app: function (name) { return appOf(name); } };
  function emit(value) {
    var text = "null";
    try { if (typeof value !== "undefined") text = JSON.stringify(value); } catch (e) {}
    var raw = emitFn(text);
    if (rejected(raw)) throw new Error(raw.slice(4));
  }
  Object.defineProperty(globalThis, "cu", { value: cu, writable: false, configurable: false });
  Object.defineProperty(globalThis, "emit", { value: emit, writable: false, configurable: false });
  var hidden = ["__cu_app", "__cu_state", "__cu_set", "__cu_press", "__cu_click", "__cu_type", "__cu_wait_el", "__cu_wait_text", "__cu_emit", "__cu_finish", "__cu_fail", "__user"];
  for (var i = 0; i < hidden.length; i++) {
    try { delete globalThis[hidden[i]]; } catch (e) {}
  }
  try {
    var value = await user(cu, emit);
    if (typeof value === "undefined") finish("");
    else {
      var encoded = "";
      try { encoded = JSON.stringify(value); } catch (e) { encoded = "null"; }
      finish(encoded);
    }
  } catch (e) {
    fail(String(e && e.message || e));
  }
})(globalThis.__user);
"#;
