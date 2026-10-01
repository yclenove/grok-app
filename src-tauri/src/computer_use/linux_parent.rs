//! GTK parent preparation boundary for the explicit native Wayland preview.
//! Default builds do not select this backend; installed-platform acceptance
//! remains required before enabling release support.

use grok_computer_use_core::native_parent::NativeParent;
use grok_computer_use_core::session_grants::AuthorizationTicket;
use grok_computer_use_gtk_parent::ParentLease;
use grok_computer_use_wayland::{PortalOptions, PortalRegistry, PortalSelection};
use gtk::prelude::Cast;

async fn prepare_for_selection<R: tauri::Runtime>(
    window: tauri::Window<R>,
    ticket: AuthorizationTicket,
) -> Result<ParentLease, String> {
    ticket.check_preparation()?;
    let (send, receive) = tokio::sync::oneshot::channel();
    let queued_window = window.clone();
    let queued_ticket = ticket.clone();
    window
        .run_on_main_thread(move || {
            // Unexpected worker loss must not enqueue another export. Normal
            // cancellation keeps this receiver alive until the queue returns.
            if send.is_closed() {
                return;
            }
            let result = queued_ticket.check_preparation().and_then(|()| {
                let gtk_window = queued_window.gtk_window().map_err(|e| e.to_string())?;
                grok_computer_use_gtk_parent::export(&gtk_window.upcast(), &queued_ticket)
            });
            // If delivery races cancellation, Drop requests retirement on the
            // retained GTK owner, never from this asynchronous caller's thread.
            let _ = send.send(result);
        })
        .map_err(|e| e.to_string())?;
    // No caller deadline may discard an unknown UI operation. The registry
    // retains this future and owns readiness, revocation and the release join.
    grok_computer_use_core::native_parent::retain_parent_dispatch(receive).await
}

pub(crate) fn select_for_authorization<R: tauri::Runtime>(
    window: tauri::Window<R>,
    registry: &PortalRegistry,
    options: PortalOptions,
    generation: u64,
    target_generation: u64,
    ticket: &AuthorizationTicket,
) -> Result<PortalSelection, String> {
    let queued_ticket = ticket.clone();
    registry.select_parented_gnome_for_authorization(
        options,
        generation,
        target_generation,
        ticket,
        move || async move {
            prepare_for_selection(window, queued_ticket)
                .await
                .map(|lease| Box::new(lease) as Box<dyn NativeParent>)
        },
    )
}
