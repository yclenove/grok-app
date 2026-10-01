//! One-shot SAVE_TARGETS handoff. This is a clipboard protocol acknowledgement,
//! not disk durability and never evidence of native paste/input completion.
use super::*;
use x11rb::protocol::res::{Client, ConnectionExt as _};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffStatus {
    NotRequested,
    Unavailable,
    Pending,
    Acknowledged,
    Declined,
    Interrupted,
    Unknown,
}

pub(super) struct ManagerWatch {
    selection: Atom,
    save: Atom,
    property: Atom,
    epoch: u64,
}
pub(super) struct ManagerTransfer {
    pub(super) requestor: Window,
    manager: Window,
    client: Client,
    manager_epoch: u64,
    clipboard_epoch: u64,
    timestamp: u32,
    reply: Option<bool>,
    delivered: HashSet<Atom>,
}

impl ClipboardOwner {
    pub fn handoff_status(&self) -> HandoffStatus {
        self.handoff_state
    }

    /// Request saving ONLY restored original data. Never expose temporary task
    /// text to a clipboard history manager through this API. The caller must
    /// keep pumping/retaining this service until a terminal result and safe close.
    /// Cancel/timeout after sending does not cancel another client's operation,
    /// release native input occupancy, resend SAVE_TARGETS, or destroy our data.
    pub fn begin_handoff(
        &mut self,
        lease: &ClipboardLease,
        cancellation: &ActionCancellation,
    ) -> Result<HandoffStatus, String> {
        self.check_lease(lease)?;
        cancellation.check()?;
        if self.keeper_pending() {
            return Err(
                "clipboard keeper transfer is pending; do not start a competing manager handoff"
                    .into(),
            );
        }
        if self.state != ClipboardState::Restored || self.saved.was_empty() {
            return Err(
                "clipboard manager handoff requires restored nonempty original ownership".into(),
            );
        }
        if self.handoff.is_some() {
            return Err("clipboard manager handoff was already attempted; do not replay".into());
        }
        self.watch_manager()?;
        let timestamp = self.server_time()?;
        self.fenced(|this| {
            let owner = this.barrier()?;
            if owner != this.window || this.owned_epoch != Some(this.reader.epoch) {
                this.handoff_state = HandoffStatus::Interrupted;
                return Ok(this.handoff_state);
            }
            let selection = this
                .manager_watch
                .as_ref()
                .ok_or("missing manager watch")?
                .selection;
            let manager = this
                .reader
                .conn
                .get_selection_owner(selection)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .owner;
            this.collect()?;
            if manager == x11rb::NONE {
                this.handoff_state = HandoffStatus::Unavailable;
                return Ok(this.handoff_state);
            }
            let client = resource_client(&this.reader.conn, manager)?;
            let requestor = window(&this.reader.conn, this.reader.root)?;
            let watch = this.manager_watch.as_ref().ok_or("missing manager watch")?;
            let targets: Vec<_> = this.saved.formats.iter().map(|f| f.target).collect();
            let staged = this
                .reader
                .conn
                .change_property32(
                    PropMode::REPLACE,
                    requestor,
                    watch.property,
                    AtomEnum::ATOM,
                    &targets,
                )
                .map_err(err)?
                .check()
                .map_err(err);
            if let Err(error) = staged.and_then(|()| cancellation.check()) {
                let _ = this
                    .reader
                    .conn
                    .destroy_window(requestor)
                    .map(|cookie| cookie.check());
                return Err(error);
            }
            this.handoff = Some(ManagerTransfer {
                requestor,
                manager,
                client,
                manager_epoch: watch.epoch,
                clipboard_epoch: this.reader.epoch,
                timestamp,
                reply: None,
                delivered: HashSet::new(),
            });
            this.handoff_state = HandoffStatus::Pending;
            // Record before send: even an error can be post-dispatch. Keep the
            // exact requestor alive and never automatically retry this request.
            this.reader
                .conn
                .convert_selection(requestor, selection, watch.save, watch.property, timestamp)
                .map_err(err)?
                .check()
                .map_err(err)?;
            Ok(this.handoff_state)
        })
    }

    fn watch_manager(&mut self) -> Result<(), String> {
        if self.manager_watch.is_some() {
            return Ok(());
        }
        let selection = atom(&self.reader.conn, b"CLIPBOARD_MANAGER")?;
        let save = atom(&self.reader.conn, b"SAVE_TARGETS")?;
        let property = atom(&self.reader.conn, b"_GROK_CU_SAVE_TARGETS")?;
        self.reader
            .conn
            .xfixes_select_selection_input(
                self.reader.watch,
                selection,
                SelectionEventMask::SET_SELECTION_OWNER
                    | SelectionEventMask::SELECTION_WINDOW_DESTROY
                    | SelectionEventMask::SELECTION_CLIENT_CLOSE,
            )
            .map_err(err)?
            .check()
            .map_err(err)?;
        // Publish the watch only after its subscription succeeded. A failed
        // setup must not make a retry silently skip ownership-change tracking.
        self.manager_watch = Some(ManagerWatch {
            selection,
            save,
            property,
            epoch: 0,
        });
        self.collect()
    }

    pub(super) fn handoff_pending(&self) -> bool {
        matches!(
            self.handoff_state,
            HandoffStatus::Pending | HandoffStatus::Unknown
        )
    }

    pub(super) fn track_handoff(&mut self, event: &Event) -> Result<(), String> {
        let Some(watch) = self.manager_watch.as_mut() else {
            return Ok(());
        };
        if let Event::XfixesSelectionNotify(change) = event {
            if change.response_type & 0x80 == 0
                && change.window == self.reader.watch
                && change.selection == watch.selection
            {
                watch.epoch = watch
                    .epoch
                    .checked_add(1)
                    .ok_or("clipboard manager epoch exhausted")?;
            }
        }
        let Some(handoff) = self.handoff.as_mut() else {
            return Ok(());
        };
        if let Event::SelectionNotify(reply) = event {
            if reply.requestor == handoff.requestor
                && reply.selection == watch.selection
                && reply.target == watch.save
                && reply.time == handoff.timestamp
                && [x11rb::NONE, watch.property].contains(&reply.property)
                && handoff.reply.is_none()
            {
                handoff.reply = Some(reply.property != x11rb::NONE);
            }
        }
        Ok(())
    }

    pub(super) fn note_delivery(
        &mut self,
        source: &Source,
        target: Atom,
        requestor: Window,
    ) -> Result<(), String> {
        let Some(handoff) = self.handoff.as_ref() else {
            return Ok(());
        };
        if self.handoff_state != HandoffStatus::Pending
            || !matches!(source, Source::Saved(s) if Arc::ptr_eq(s, &self.saved))
        {
            return Ok(());
        }
        // A different application's read is not evidence the captured manager
        // received the requested format. Never trust _NET_WM_PID properties.
        let client = resource_client(&self.reader.conn, requestor)?;
        if same_client(&client, &handoff.client) {
            self.handoff
                .as_mut()
                .ok_or("missing handoff")?
                .delivered
                .insert(target);
        }
        Ok(())
    }

    pub(super) fn check_handoff(&mut self) -> Result<(), String> {
        if !self.handoff_pending() {
            return Ok(());
        }
        self.fenced(|this| {
            let clipboard = this.barrier()?;
            let selection = this
                .manager_watch
                .as_ref()
                .ok_or("missing manager watch")?
                .selection;
            let manager = this
                .reader
                .conn
                .get_selection_owner(selection)
                .map_err(err)?
                .reply()
                .map_err(err)?
                .owner;
            this.collect()?;
            // A last INCR delete can precede a manager acknowledgement in this
            // very batch. Process it before judging completeness, not after.
            if !this.pending.is_empty() {
                return Ok(());
            }
            let watch = this.manager_watch.as_ref().ok_or("missing manager watch")?;
            let handoff = this.handoff.as_ref().ok_or("missing handoff")?;
            if manager != handoff.manager || watch.epoch != handoff.manager_epoch {
                this.handoff_state = HandoffStatus::Interrupted;
                return Ok(());
            }
            let next_epoch = handoff
                .clipboard_epoch
                .checked_add(1)
                .ok_or("clipboard epoch exhausted")?;
            let manager_owns = clipboard != x11rb::NONE
                && clipboard != this.window
                && same_client(
                    &resource_client(&this.reader.conn, clipboard)?,
                    &handoff.client,
                );
            let retained = clipboard == this.window && this.reader.epoch == handoff.clipboard_epoch;
            let handed_over = manager_owns && this.reader.epoch == next_epoch;
            if !retained && !handed_over {
                this.handoff_state = HandoffStatus::Interrupted;
                return Ok(());
            }
            if this.handoff_state == HandoffStatus::Unknown {
                return Ok(());
            }
            if let Some(accepted) = handoff.reply {
                this.handoff_state = if !accepted {
                    HandoffStatus::Declined
                } else if handed_over
                    && this
                        .saved
                        .formats
                        .iter()
                        .all(|f| handoff.delivered.contains(&f.target))
                {
                    HandoffStatus::Acknowledged
                } else {
                    HandoffStatus::Unknown
                };
            }
            Ok(())
        })
    }
}

fn resource_client(conn: &RustConnection, window: Window) -> Result<Client, String> {
    let clients = conn
        .res_query_clients()
        .map_err(err)?
        .reply()
        .map_err(err)?
        .clients;
    let matches: Vec<_> = clients
        .into_iter()
        .filter(|c| window != x11rb::NONE && window & !c.resource_mask == c.resource_base)
        .collect();
    match matches.as_slice() {
        [client] => Ok(*client),
        _ => Err("XRes did not prove one clipboard resource client".into()),
    }
}
fn same_client(a: &Client, b: &Client) -> bool {
    a.resource_base == b.resource_base && a.resource_mask == b.resource_mask
}
