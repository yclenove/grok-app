//! Exact original handles retire downloads; crash maintenance requires both
//! recorded kernel owners retired and every candidate fully validated first.

use super::*;

pub(super) fn cleanup(lease: &Lease) -> Result<(), String> {
    let mut state = lease.state.lock();
    if state.complete {
        return Ok(());
    }
    let store = Store::open(&lease.app_root, false)?;
    if store.header != lease.record.binding.store {
        return Err("native download journal store replaced".into());
    }
    prepare(lease, &mut state)?;
    retire(lease, &mut state)
}

fn allowed_entries(path: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(path).map_err(safety::error)? {
        let entry = entry.map_err(safety::error)?;
        if entry.file_name() != MARKER && entry.file_name() != "download.bin" {
            return Err("native download buffer contains an unexpected child; retained".into());
        }
    }
    Ok(())
}

pub(super) fn prepare(lease: &Lease, state: &mut State) -> Result<(), String> {
    let binding = &lease.record.binding;
    if let Some(record) = &state.record {
        if safety::identity(record, false)? != binding.record
            || safety::read(record, MAX_RECORD)? != json(&lease.record)?
        {
            return Err("native download journal record changed".into());
        }
    } else if lease.record_path.try_exists().map_err(safety::error)? {
        return Err("native download retired record pathname occupied; retained".into());
    }
    if let Some(retired) = &state.retired {
        let receipt = state
            .receipt
            .as_ref()
            .ok_or("native download retirement receipt missing")?;
        if receipt.identity.as_ref() != Some(&safety::identity(retired, false)?)
            || safety::read(retired, MAX_RECORD)? != json(receipt)?
            || receipt.record != lease.record
        {
            return Err("native download retirement receipt changed".into());
        }
    }
    let parent = lease
        .buffer_path
        .parent()
        .ok_or("native buffer parent missing")?;
    let parent_pin = safety::directory(parent, false)?;
    if safety::identity(&parent_pin, true)? != binding.temp_parent_identity {
        return Err("native download Temp parent replaced".into());
    }
    if let Some(buffer) = &state.buffer {
        if safety::identity(buffer, true)? != binding.buffer {
            return Err("native download buffer directory replaced".into());
        }
        allowed_entries(&lease.buffer_path)?;
        if let Some(marker) = &state.marker {
            if safety::identity(marker, false)? != lease.record.marker
                || safety::read(marker, MAX_RECORD)? != json(binding)?
            {
                return Err("native download buffer marker replaced".into());
            }
        } else if state.receipt.is_none()
            || lease
                .buffer_path
                .join(MARKER)
                .try_exists()
                .map_err(safety::error)?
        {
            return Err("native download original marker absent or replaced; retained".into());
        }
        let data_path = lease.buffer_path.join("download.bin");
        if !state.prepared {
            let data_pin = if data_path.try_exists().map_err(safety::error)? {
                Some(safety::file(&data_path, true)?)
            } else {
                None
            };
            let data = data_pin
                .as_ref()
                .map(|file| safety::identity(file, false))
                .transpose()?;
            if state
                .receipt
                .as_ref()
                .is_some_and(|receipt| data.is_some() && receipt.data != data)
            {
                return Err("native download original data replaced; retained".into());
            }
            if state.receipt.is_none() {
                state.receipt = Some(Receipt {
                    record: lease.record.clone(),
                    data,
                    identity: None,
                });
            }
            state.data = data_pin;
            state.prepared = true;
        } else if state.data.is_none() && data_path.try_exists().map_err(safety::error)? {
            return Err("native download retired data pathname occupied; retained".into());
        }
    } else if state.receipt.is_none() || lease.buffer_path.try_exists().map_err(safety::error)? {
        return Err("native download original buffer absent or replaced; retained".into());
    }
    Ok(())
}

pub(super) fn retire(lease: &Lease, state: &mut State) -> Result<(), String> {
    if state.retired.is_none() {
        let pin = safety::write_new(&lease.receipt_path, &[])?;
        let receipt = state
            .receipt
            .as_mut()
            .ok_or("native download retirement plan missing")?;
        receipt.identity = Some(safety::identity(&pin, false)?);
        let bytes = json(receipt)?;
        state.retired = Some(pin);
        let pin = state
            .retired
            .as_mut()
            .ok_or("native download retirement pin missing")?;
        pin.write_all(&bytes).map_err(safety::error)?;
        pin.sync_all().map_err(safety::error)?;
        if safety::read(pin, MAX_RECORD)? != bytes {
            return Err("native download retirement publication changed".into());
        }
    }
    if state.buffer.is_some() {
        safety::delete(&mut state.data)?;
        safety::delete(&mut state.marker)?;
        safety::delete(&mut state.buffer)?;
        if lease.buffer_path.try_exists().map_err(safety::error)? {
            return Err("native download buffer deletion still pending".into());
        }
    }
    // Flat record/receipt avoids an empty transaction directory becoming an
    // unprovable orphan if the process exits after deleting its last marker.
    safety::delete(&mut state.record)?;
    if lease.record_path.try_exists().map_err(safety::error)? {
        return Err("native download record deletion still pending".into());
    }
    safety::delete(&mut state.retired)?;
    state.complete = true;
    Ok(())
}

fn candidate(store: &Store, app_root: &Path, transaction: &str) -> Result<Lease, String> {
    let record_path = store.path.join(format!("{transaction}.json"));
    let receipt_path = store.path.join(format!("{transaction}.retired.json"));
    let record_pin = if record_path.try_exists().map_err(safety::error)? {
        Some(safety::file(&record_path, true)?)
    } else {
        None
    };
    let retired_pin = if receipt_path.try_exists().map_err(safety::error)? {
        Some(safety::file(&receipt_path, true)?)
    } else {
        None
    };
    let receipt: Option<Receipt> = retired_pin
        .as_ref()
        .map(|file| -> Result<Receipt, String> {
            let bytes = safety::read(file, MAX_RECORD)?;
            let receipt: Receipt = serde_json::from_slice(&bytes).map_err(safety::error)?;
            if json(&receipt)? != bytes {
                return Err("native download retirement receipt invalid".into());
            }
            Ok(receipt)
        })
        .transpose()?;
    let record: Record = if let Some(file) = &record_pin {
        serde_json::from_slice(&safety::read(file, MAX_RECORD)?).map_err(safety::error)?
    } else {
        receipt
            .as_ref()
            .ok_or("native download partial publication; retained")?
            .record
            .clone()
    };
    let binding = &record.binding;
    let buffer_path = buffer_path(binding)?;
    if binding.transaction != transaction || binding.store != store.header {
        return Err("native download journal binding changed; retained".into());
    }
    if !binding.app.retired()? || !binding.browser.retired()? {
        return Err("native download original App or browser is still live; retained".into());
    }
    let mut pins = safety::parents(
        buffer_path.parent().ok_or("native buffer parent missing")?,
        false,
    )?;
    let buffer = if buffer_path.try_exists().map_err(safety::error)? {
        Some(safety::directory(&buffer_path, true)?)
    } else {
        None
    };
    let marker = if buffer.is_some()
        && buffer_path
            .join(MARKER)
            .try_exists()
            .map_err(safety::error)?
    {
        Some(safety::file(&buffer_path.join(MARKER), true)?)
    } else {
        None
    };
    for pin in &store.pins {
        pins.push(pin.try_clone().map_err(safety::error)?);
    }
    let lease = Lease {
        app_root: app_root.into(),
        buffer_path,
        record_path,
        receipt_path,
        record,
        state: Mutex::new(State {
            buffer,
            marker,
            record: record_pin,
            retired: retired_pin,
            receipt,
            data: None,
            prepared: false,
            _owner: store.owner.try_clone().map_err(safety::error)?,
            _pins: pins,
            complete: false,
        }),
    };
    prepare(&lease, &mut lease.state.lock())?;
    Ok(lease)
}

pub(super) fn maintain(app_root: &Path) -> Result<(), String> {
    let store = Store::open(app_root, false)?;
    let mut candidates = Vec::new();
    for transaction in store.slots()? {
        candidates.push(candidate(&store, app_root, &transaction)?);
    }
    // Original cross-process lock and all candidate handles stay held until the
    // complete preflight succeeds. A corrupt later candidate deletes nothing.
    for lease in candidates {
        let mut state = lease.state.lock();
        retire(&lease, &mut state)?;
    }
    Ok(())
}
