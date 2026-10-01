use grok_computer_use_core::session_grants::AuthorizationTicket;
use grok_computer_use_gtk_parent::ParentLease;
use gtk::prelude::*;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::mpsc,
    time::Duration,
};
use tokio::sync::oneshot;

pub enum Command {
    Export(
        AuthorizationTicket,
        oneshot::Sender<Result<ParentLease, String>>,
    ),
    Snapshot(oneshot::Sender<(bool, Vec<(u32, bool)>)>),
    PointerSnapshot(oneshot::Sender<(u64, Vec<(u32, bool)>)>),
    ArmRecovery {
        armed: oneshot::Sender<()>,
        clicked: oneshot::Sender<Vec<(u32, bool)>>,
    },
}

pub fn run(runtime: tokio::runtime::Runtime) -> Result<(), Box<dyn std::error::Error>> {
    gtk::init()?;
    let context = gtk::glib::MainContext::default();
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Owned GNOME portal grant acceptance — NOT Grok App");
    window.set_default_size(640, 240);
    let layout = gtk::Box::new(gtk::Orientation::Vertical, 12);
    layout.pack_start(&gtk::Label::new(Some("Owned VM: approve the real screen-sharing dialog.\nThen do not touch input until GRANT_READY.\nEnter/click receipts are recorded; no arbitrary commands are accepted.")), true, true, 0);
    let button = gtk::Button::with_label("Owned pointer target: 0");
    button.set_widget_name("owned-pointer-target");
    button.set_can_focus(false);
    button.set_size_request(320, 64);
    button.set_halign(gtk::Align::Center);
    let css = gtk::CssProvider::new();
    css.load_from_data(b"#owned-pointer-target { background-image: none; background-color: rgb(17, 201, 83); border-radius: 0; }")?;
    button
        .style_context()
        .add_provider(&css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    layout.pack_start(&button, false, false, 12);
    // Owned acceptance only. Never offers or emits an automatic re-grant.
    let recovery = gtk::Button::with_label("Owned acceptance: request NEW portal consent");
    recovery.set_can_focus(false);
    recovery.set_halign(gtk::Align::Center);
    recovery.set_no_show_all(true);
    layout.pack_start(&recovery, false, false, 12);
    let recovery_reply = Rc::new(RefCell::new(None::<oneshot::Sender<Vec<(u32, bool)>>>));
    let recovery_events = Rc::new(RefCell::new(Vec::new()));
    for pressed in [true, false] {
        let events = recovery_events.clone();
        let callback = move |_: &gtk::Button, event: &gtk::gdk::EventButton| {
            let mut events = events.borrow_mut();
            if events.len() < 8 {
                events.push((event.button(), pressed));
            }
            gtk::glib::Propagation::Proceed
        };
        if pressed {
            recovery.connect_button_press_event(callback);
        } else {
            recovery.connect_button_release_event(callback);
        }
    }
    let pending = recovery_reply.clone();
    let receipts = recovery_events.clone();
    recovery.connect_clicked(move |button| {
        if let Some(reply) = pending.borrow_mut().take() {
            button.set_sensitive(false);
            button.hide();
            let receipts = receipts.clone();
            // Let this original release event finish dispatch before reading it.
            gtk::glib::idle_add_local_once(move || {
                let _ = reply.send(receipts.borrow().clone());
            });
        }
    });
    window.add(&layout);
    let clicks = Rc::new(Cell::new(0_u64));
    let count = clicks.clone();
    button.connect_clicked(move |button| {
        count.set(count.get() + 1);
        button.set_label(&format!("Owned pointer target: {}", count.get()));
    });
    let pointer = Rc::new(RefCell::new(Vec::new()));
    for pressed in [true, false] {
        let pointer = pointer.clone();
        let callback = move |_: &gtk::Button, event: &gtk::gdk::EventButton| {
            let mut events = pointer.borrow_mut();
            if events.len() < 32 {
                events.push((event.button(), pressed));
            }
            gtk::glib::Propagation::Proceed
        };
        if pressed {
            button.connect_button_press_event(callback);
        } else {
            button.connect_button_release_event(callback);
        }
    }
    let events = Rc::new(RefCell::new(Vec::new()));
    let pressed = events.clone();
    window.connect_key_press_event(move |_, event| {
        pressed.borrow_mut().push(((*event.keyval()), true));
        gtk::glib::Propagation::Proceed
    });
    let released = events.clone();
    window.connect_key_release_event(move |_, event| {
        released.borrow_mut().push(((*event.keyval()), false));
        gtk::glib::Propagation::Proceed
    });
    window.show_all();
    let (queue, commands) = mpsc::channel();
    let owner = runtime.spawn(super::run::accept(queue));
    // The GTK owner keeps running until the original registry, native watch and
    // parent lease have joined. A hidden/destroyed window must not end it early.
    while !owner.is_finished() {
        while context.pending() {
            context.iteration(false);
        }
        match commands.recv_timeout(Duration::from_millis(2)) {
            Ok(Command::Export(ticket, reply)) => {
                let result = ticket
                    .check_preparation()
                    .and_then(|()| grok_computer_use_gtk_parent::export(&window, &ticket));
                let _ = reply.send(result);
            }
            Ok(Command::Snapshot(reply)) => {
                let _ = reply.send((window.is_active(), events.borrow().clone()));
            }
            Ok(Command::PointerSnapshot(reply)) => {
                let _ = reply.send((clicks.get(), pointer.borrow().clone()));
            }
            Ok(Command::ArmRecovery { armed, clicked }) => {
                if recovery_reply.borrow().is_none() && recovery_events.borrow().is_empty() {
                    *recovery_reply.borrow_mut() = Some(clicked);
                    recovery.set_sensitive(true);
                    recovery.show();
                    let _ = armed.send(());
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
    let result = runtime.block_on(owner)?;
    unsafe {
        window.destroy();
    }
    while context.pending() {
        context.iteration(false);
    }
    result.map_err(Into::into)
}
