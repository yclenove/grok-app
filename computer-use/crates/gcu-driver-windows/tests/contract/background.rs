//! Exact foreground assertions, with a same-element direct UIA control for virtual press.
use super::*;
use super::{
    unsafe_activation, unsafe_background_probe::Native, unsafe_desktop, unsafe_foreground,
    unsafe_invoke,
};
use gcu_core::model::States;

async fn responsive(driver_window: &WindowRef) -> Result<(), String> {
    // JSON is written inside the frame. Wait for the native pump as well, never
    // restore foreground or replay the action while waiting for its effect.
    poll_async(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        "background fixture WM_NULL readiness",
        async || unsafe_desktop::responsiveness(driver_window).await,
        |probe| probe.responsive,
    )
    .await?;
    Ok(())
}

async fn unchanged(
    win: &WindowRef,
    expected: u64,
    after_dispatch: u64,
    action: &str,
) -> Result<(), String> {
    let after_effect = unsafe_foreground::current();
    responsive(win).await?;
    let actual = unsafe_foreground::current();
    progress(format!("BACKGROUND: action={action}; expected={expected:x}; after_dispatch={after_dispatch:x}; after_effect={after_effect:?}; after_pump={actual:?}"));
    require(after_dispatch == expected && after_effect.hwnd == expected && actual.hwnd == expected,
        format!("background {action} stole foreground: expected={expected:x}; after_dispatch={after_dispatch:x}; after_effect={after_effect:?}; after_pump={actual:?}"))
}

pub async fn virtual_element(
    driver: &WindowsDriver,
    fixture: &Fixture,
    win: &WindowRef,
) -> CaseResult {
    let button = node(driver, win, "counter_button").await?;
    let element = button
        .handle
        .downcast_ref::<gcu_driver_windows::uia::UiaElement>()
        .ok_or("not a UIA element")?;
    let cover = unsafe_desktop::Cover::start(unsafe_desktop::window_rect(win, fixture.pid())?)?;
    let expected = unsafe_foreground::current();
    if expected.hwnd == 0 || expected.pid == 0 || expected.pid == fixture.pid() {
        return Ok(Outcome::Skip(format!("C16 virtual UIA button requires an unrelated foreground for the Invoke control; actual={expected:?}; cover_setup={:?}", cover.foreground_setup)));
    }
    let count = fixture.read()?["counter"]
        .as_u64()
        .ok_or("counter missing")?;
    let trace = unsafe_activation::Trace::start()?;
    trace
        .record(
            "same-element direct Invoke before",
            element.hwnd,
            cover.hwnd,
        )
        .await;
    let control = unsafe_invoke::invoke(
        element.hwnd,
        Some("counter_button"),
        expected.hwnd,
        Some(&element.runtime_id),
    )
    .await?;
    fixture
        .wait("same-element direct Invoke counter exactly once", |v| {
            v["counter"] == count + 1
        })
        .await?;
    let after_effect = unsafe_foreground::current();
    responsive(win).await?;
    let after_pump = unsafe_foreground::current();
    require(
        fixture.read()?["counter"] == count + 1,
        "direct Invoke counter replayed",
    )?;
    trace
        .record(
            "same-element direct Invoke after pump",
            element.hwnd,
            cover.hwnd,
        )
        .await;
    let data = format!("element_type=egui virtual UIA Button; RuntimeId={:?}; before={:?}; after_dispatch={:?}; after_effect={after_effect:?}; after_pump={after_pump:?}; counter={count}->{}", element.runtime_id, control.before, control.after_dispatch, count + 1);
    progress(format!(
        "C16 COVER PREPARATION: {:?}; measured unrelated foreground={expected:?}",
        cover.foreground_setup
    ));
    progress(format!("C16 SAME-RUN INVOKE CONTROL: {data}"));
    let changed: Vec<_> = [&control.after_dispatch, &after_effect, &after_pump]
        .into_iter()
        .filter(|f| f.hwnd != control.before.hwnd)
        .collect();
    if !changed.is_empty() {
        require(changed.iter().all(|f| f.pid == fixture.pid()), format!("Invoke control foreground changed to an unrelated process; attribution unresolved: {data}"))?;
        return Ok(Outcome::Skip(format!("provider: bare UIA Invoke itself activates this virtual element type on Windows; {data}")));
    }
    // The control did not activate: the driver must meet the original exact
    // foreground/effect assertions. Fresh observation retains the same target.
    let button = node(driver, win, "counter_button").await?;
    let refreshed = button
        .handle
        .downcast_ref::<gcu_driver_windows::uia::UiaElement>()
        .ok_or("refreshed target is not a UIA element")?;
    require(
        refreshed.hwnd == element.hwnd && refreshed.runtime_id == element.runtime_id,
        "Invoke control and driver press must address the same element",
    )?;
    require(
        unsafe_desktop::foreground() == expected.hwnd,
        "unrelated foreground changed before driver press",
    )?;
    let delivery = perform(driver, DriverAction::Press { el: button.handle }).await?;
    require(
        delivery == Delivery::Accessibility,
        "virtual press did not use UIA semantics",
    )?;
    let after_dispatch = unsafe_desktop::foreground();
    fixture
        .wait("background driver counter exactly once", |v| {
            v["counter"] == count + 2
        })
        .await?;
    unchanged(win, expected.hwnd, after_dispatch, "virtual press").await?;
    require(
        fixture.read()?["counter"] == count + 2,
        "driver counter replayed",
    )?;
    let field = node(driver, win, "name_field").await?;
    if !field.states.contains(States::SETTABLE) {
        return Ok(Outcome::Skip("provider: Invoke control and driver press preserved foreground, but no writable UIA text ValuePattern for C16 SetValue".into()));
    }
    require(
        unsafe_desktop::foreground() == expected.hwnd,
        "unrelated foreground changed before SetValue",
    )?;
    perform(
        driver,
        DriverAction::SetValue {
            el: field.handle,
            value: "background".into(),
        },
    )
    .await?;
    let after_dispatch = unsafe_desktop::foreground();
    fixture
        .wait("background text", |v| v["text"] == "background")
        .await?;
    unchanged(win, expected.hwnd, after_dispatch, "SetValue").await?;
    Ok(Outcome::Passed)
}

pub async fn native_button(driver: &WindowsDriver) -> CaseResult {
    let native = Native::start()?;
    let executable = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let app = driver
        .resolve_app(&AppQuery(executable))
        .await
        .map_err(|e| e.to_string())?;
    require(
        app.info.pid == Some(std::process::id()),
        "native control process identity mismatch",
    )?;
    let windows = driver
        .list_windows(Some(&app))
        .await
        .map_err(|e| e.to_string())?;
    let info = windows
        .into_iter()
        .find(|w| {
            u64::from_str_radix(w.id.split(':').next().unwrap_or(""), 16).ok() == Some(native.root)
        })
        .ok_or("exact native control HWND missing")?;
    let win = WindowRef { app, id: info.id };
    let cover =
        unsafe_desktop::Cover::start(unsafe_desktop::window_rect(&win, std::process::id())?)?;
    let button = node(driver, &win, "GCU native counter").await?;
    // Native root is shown with SWP_NOACTIVATE, retaining normal window styles.
    // This assertion needs an unrelated foreground, not specifically the cover:
    // use the actual HWND if setup was denied. Equality checks remain exact.
    let before = unsafe_foreground::current();
    require(
        before.hwnd != 0
            && before.pid != 0
            && before.hwnd != native.root
            && before.hwnd != native.button,
        format!(
            "native BUTTON is not background: {before:?}; cover_setup={:?}",
            cover.foreground_setup
        ),
    )?;
    let count = native.clicks();
    let delivery = perform(driver, DriverAction::Press { el: button.handle }).await?;
    let after_dispatch = unsafe_desktop::foreground();
    require(
        delivery == Delivery::TargetedEvent,
        format!("native BUTTON must use BM_CLICK: {delivery:?}"),
    )?;
    poll(
        &mut DesktopClock::default(),
        Duration::from_secs(5),
        "native BUTTON WM_COMMAND exactly once",
        || Ok(native.clicks()),
        |n| *n == count + 1,
    )
    .await?;
    unchanged(&win, before.hwnd, after_dispatch, "native BUTTON BM_CLICK").await?;
    require(native.clicks() == count + 1, "native BUTTON replayed")?;
    progress(format!("C16-BUTTON EVIDENCE: driver delivery={delivery:?}; counter={count}->{}; foreground_before={before:?}; foreground_after={:?}; cover_setup={:?}", native.clicks(), unsafe_foreground::current(), cover.foreground_setup));
    Ok(Outcome::Passed)
}
