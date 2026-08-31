(function () {
  try {
    if (location.protocol !== "https:" || location.hostname !== "grok.com" ||
        location.pathname.indexOf("/imagine/saved") !== 0) return;
    function hasSecurityChallenge() {
      try {
        var surface = document.querySelector(
          'iframe[src*="/cdn-cgi/challenge-platform/"], #challenge-stage, #challenge-running, #challenge-body-text, form#challenge-form, input[name="cf-turnstile-response"], textarea[name="cf-turnstile-response"], [data-translate="challenge_headline"]'
        );
        var asset = document.querySelector(
          'script[src*="/cdn-cgi/challenge-platform/"], link[href*="/cdn-cgi/styles/challenges.css"]'
        );
        return !!surface || (!document.querySelector("main") && !!asset);
      } catch (_) {
        return false;
      }
    }
    function recoverSignedOutShell() {
      try {
        if (document.readyState === "complete" && !document.querySelector("main") &&
            !hasSecurityChallenge()) {
          location.replace("https://grok.com/");
        }
      } catch (_) {}
    }
    function scheduleRecovery() {
      window.setTimeout(recoverSignedOutShell, 1600);
    }
    if (document.readyState === "complete") {
      scheduleRecovery();
    } else {
      window.addEventListener("load", scheduleRecovery, { once: true });
    }
  } catch (_) {}
})();
