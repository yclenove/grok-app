//! ICCCM selection serving. Foreign-window failures decline only that conversion;
//! transport failures remain errors. This module never dispatches native input.
use super::*;
use x11rb::errors::ReplyError;
use x11rb::protocol::xproto::SelectionNotifyEvent;

const MAX_PAIRS: usize = 32;

impl ClipboardOwner {
    pub(super) fn serve_request(&mut self, request: Request) -> Result<(), String> {
        let event = &request.event;
        let property = if event.property == x11rb::NONE {
            event.target
        } else {
            event.property
        };
        let accepted = request.allowed
            && event.requestor != x11rb::NONE
            && event.requestor != self.window
            && event.requestor != self.reader.watch
            && valid_time(event.time, request.acquired, self.clock)
            && if event.target == self.multiple {
                event.property != x11rb::NONE && self.multiple_request(&request)?
            } else {
                self.convert(&request, event.target, property)?
            };
        let notify = SelectionNotifyEvent {
            response_type: xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: event.time,
            requestor: event.requestor,
            selection: event.selection,
            target: event.target,
            property: if accepted { property } else { x11rb::NONE },
        };
        let delivered = foreign(
            self.reader
                .conn
                .send_event(false, event.requestor, EventMask::NO_EVENT, notify)
                .map_err(err)?
                .check(),
        )?;
        if !delivered {
            self.remove_window(event.requestor);
        }
        Ok(())
    }

    fn multiple_request(&mut self, request: &Request) -> Result<bool, String> {
        let event = &request.event;
        if self
            .transfers
            .contains_key(&(event.requestor, event.property))
        {
            return Ok(false);
        }
        let reply = match self
            .reader
            .conn
            .get_property(
                false,
                event.requestor,
                event.property,
                AtomEnum::ANY,
                0,
                (MAX_PAIRS * 2) as u32,
            )
            .map_err(err)?
            .reply()
        {
            Ok(reply) => reply,
            Err(ReplyError::X11Error(_)) => return Ok(false),
            Err(error) => return Err(err(error)),
        };
        if reply.format != 32
            || ![self.atom_pair, u32::from(AtomEnum::ATOM)].contains(&reply.type_)
            || reply.bytes_after != 0
            || reply.value_len as usize > MAX_PAIRS * 2
            || reply.value.len() != reply.value_len as usize * 4
            || reply.value_len % 2 != 0
        {
            return Ok(false);
        }
        let Some(values) = reply.value32() else {
            return Ok(false);
        };
        let mut pairs: Vec<u32> = values.collect();
        let mut used = HashSet::from([event.property]);
        for pair in pairs.as_chunks_mut::<2>().0 {
            if pair[0] == self.multiple
                || pair[1] == x11rb::NONE
                || !used.insert(pair[1])
                || !self.convert(request, pair[0], pair[1])?
            {
                // GTK/Xlib's target/property pairs: mark the failed PROPERTY None.
                pair[1] = x11rb::NONE;
            }
        }
        foreign(
            self.reader
                .conn
                .change_property32(
                    PropMode::REPLACE,
                    event.requestor,
                    event.property,
                    self.atom_pair,
                    &pairs,
                )
                .map_err(err)?
                .check(),
        )
    }

    fn convert(&mut self, request: &Request, target: Atom, property: Atom) -> Result<bool, String> {
        let window = request.event.requestor;
        if property == x11rb::NONE || self.transfers.contains_key(&(window, property)) {
            return Ok(false);
        }
        if target == self.reader.targets {
            let mut targets = vec![self.reader.targets, self.timestamp_atom, self.multiple];
            targets.extend(request.source.formats().iter().map(|f| f.target));
            return foreign(
                self.reader
                    .conn
                    .change_property32(
                        PropMode::REPLACE,
                        window,
                        property,
                        AtomEnum::ATOM,
                        &targets,
                    )
                    .map_err(err)?
                    .check(),
            );
        }
        if target == self.timestamp_atom {
            return foreign(
                self.reader
                    .conn
                    .change_property32(
                        PropMode::REPLACE,
                        window,
                        property,
                        AtomEnum::INTEGER,
                        &[request.acquired],
                    )
                    .map_err(err)?
                    .check(),
            );
        }
        let Some(index) = request
            .source
            .formats()
            .iter()
            .position(|f| f.target == target)
        else {
            return Ok(false);
        };
        let payload = &request.source.formats()[index].payload;
        if payload.bytes.len() <= self.chunk_bytes {
            let written = self
                .write_payload(window, property, payload, &payload.bytes)?
                .is_some();
            if written {
                self.note_delivery(&request.source, target, window)?;
            }
            return Ok(written);
        }
        if self.transfers.len() >= MAX_TRANSFERS {
            return Ok(false);
        }
        if !foreign(
            self.reader
                .conn
                .change_window_attributes(
                    window,
                    &ChangeWindowAttributesAux::new()
                        .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
                )
                .map_err(err)?
                .check(),
        )? {
            return Ok(false);
        }
        let cookie = self
            .reader
            .conn
            .change_property32(
                PropMode::REPLACE,
                window,
                property,
                self.reader.incr,
                &[payload.bytes.len() as u32],
            )
            .map_err(err)?;
        let written_at = cookie.sequence_number();
        if !foreign(cookie.check())? {
            self.unwatch_idle(window);
            return Ok(false);
        }
        self.transfers.insert(
            (window, property),
            Transfer {
                source: request.source.clone(),
                index,
                offset: 0,
                deadline: Instant::now() + TRANSFER_DEADLINE,
                terminator_sent: false,
                written_at,
                uncertain: false,
                target,
            },
        );
        Ok(true)
    }

    fn write_payload(
        &self,
        window: Window,
        property: Atom,
        payload: &Payload,
        bytes: &[u8],
    ) -> Result<Option<u64>, String> {
        let width = usize::from(payload.format / 8);
        if ![8, 16, 32].contains(&payload.format) || !bytes.len().is_multiple_of(width) {
            return Err("invalid materialized clipboard format".into());
        }
        let cookie = self
            .reader
            .conn
            .change_property(
                PropMode::REPLACE,
                window,
                property,
                payload.kind,
                payload.format,
                (bytes.len() / width) as u32,
                bytes,
            )
            .map_err(err)?;
        let sequence = cookie.sequence_number();
        Ok(foreign(cookie.check())?.then_some(sequence))
    }

    pub(super) fn advance(
        &mut self,
        event: PropertyNotifyEvent,
        sequence: u64,
    ) -> Result<(), String> {
        let key = (event.window, event.atom);
        if event.state != Property::DELETE {
            return Ok(());
        }
        let Some(mut transfer) = self.transfers.remove(&key) else {
            return Ok(());
        };
        // x11rb expands the wire's 16-bit sequence to the full request sequence.
        // An old delete queued before this transfer's header cannot acknowledge it.
        if sequence < transfer.written_at {
            self.transfers.insert(key, transfer);
            return Ok(());
        }
        if transfer.uncertain && Instant::now() < transfer.deadline {
            self.transfers.insert(key, transfer);
            return Ok(());
        }
        if transfer.terminator_sent || Instant::now() >= transfer.deadline {
            if transfer.terminator_sent && !transfer.uncertain {
                self.note_delivery(&transfer.source, transfer.target, event.window)?;
            }
            self.unwatch_idle(event.window);
            return Ok(());
        }
        let payload = &transfer.source.formats()[transfer.index].payload;
        let end = (transfer.offset + self.chunk_bytes).min(payload.bytes.len());
        let bytes = &payload.bytes[transfer.offset..end];
        let result = self.write_payload(event.window, event.atom, payload, bytes);
        match result {
            Ok(Some(written_at)) => {
                transfer.terminator_sent = bytes.is_empty();
                transfer.offset = end;
                transfer.written_at = written_at;
                self.transfers.insert(key, transfer);
                Ok(())
            }
            Ok(None) => {
                self.unwatch_idle(event.window);
                Ok(())
            }
            Err(error) => {
                // Unknown send is not completion. Retain the transfer until an
                // actual acknowledgement or its bounded clipboard-only expiry.
                transfer.uncertain = true;
                self.transfers.insert(key, transfer);
                Err(error)
            }
        }
    }

    pub(super) fn remove_window(&mut self, window: Window) {
        self.transfers.retain(|(w, _), _| *w != window);
        self.unwatch_idle(window);
    }
    fn unwatch_idle(&self, window: Window) {
        if !self.transfers.keys().any(|(w, _)| *w == window) {
            // Best-effort local subscription cleanup, never delete a foreign
            // property (it may already belong to a later conversion).
            if let Ok(cookie) = self.reader.conn.change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::NO_EVENT),
            ) {
                let _ = cookie.check();
            }
        }
    }
    pub(super) fn expire_transfers(&mut self) {
        let now = Instant::now();
        let expired: Vec<_> = self
            .transfers
            .iter()
            .filter(|(_, t)| now >= t.deadline)
            .map(|(key, _)| *key)
            .collect();
        for key in expired {
            self.transfers.remove(&key);
            self.unwatch_idle(key.0);
        }
    }
}

fn foreign(result: Result<(), ReplyError>) -> Result<bool, String> {
    match result {
        Ok(()) => Ok(true),
        Err(ReplyError::X11Error(_)) => Ok(false),
        Err(error) => Err(err(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamps_check_both_ends_and_wrap() {
        assert!(valid_time(0, 20, 30));
        assert!(valid_time(20, 20, 30));
        assert!(valid_time(30, 20, 30));
        assert!(!valid_time(19, 20, 30));
        assert!(!valid_time(31, 20, 30));
        assert!(valid_time(u32::MAX, u32::MAX - 4, 5));
        assert!(valid_time(2, u32::MAX - 4, 5));
        assert!(!valid_time(u32::MAX - 5, u32::MAX - 4, 5));
        assert!(!valid_time(6, u32::MAX - 4, 5));
    }
}
