//! Owned fixture readiness, not a production snapshot/action retry. WebView2's
//! NavigationCompleted can precede the renderer applying its initial viewport.

use super::*;
use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;

pub(super) fn wait(
    webview: &ICoreWebView2,
    width: i32,
    height: i32,
    startup: Option<&startup::Startup>,
) -> Result<(), String> {
    if width <= 0 || height <= 0 {
        return Err("owned WebView fixture bounds unavailable".into());
    }
    let script = format!(
        r#"(() => {{
          const state = {{ready: false, frames: 0}};
          globalThis.__cuNativeFixtureReadiness = state;
          const reset = () => {{ state.frames = 0; }};
          const sources = [window, window.visualViewport].filter(Boolean);
          sources.forEach(source => source.addEventListener('resize', reset));
          const tick = () => {{
            const matches = document.readyState === 'complete'
              && !document.hidden
              && Math.abs(innerWidth * devicePixelRatio - {width}) <= 1
              && Math.abs(innerHeight * devicePixelRatio - {height}) <= 1;
            state.frames = matches ? state.frames + 1 : 0;
            if (state.frames >= 2) {{
              sources.forEach(source => source.removeEventListener('resize', reset));
              state.ready = true;
            }} else {{ requestAnimationFrame(tick); }}
          }};
          requestAnimationFrame(tick);
          return true;
        }})()"#
    );
    let current = || startup.map_or(Ok(()), |startup| startup.check());
    run_script_current(webview, &script, None, current)?;
    let expires = Instant::now() + Duration::from_secs(4);
    loop {
        let status = run_script_current(
            webview,
            "globalThis.__cuNativeFixtureReadiness?.ready === true",
            None,
            current,
        )?;
        if status == "true" {
            return Ok(());
        }
        if Instant::now() >= expires {
            return Err("owned WebView fixture viewport never became ready".into());
        }
        // The bounded native reply wait pumps the owning STA.
        // Readiness requires renderer animation frames, not a fixed startup sleep.
    }
}
