//! PipeWire objects never leave their owning native thread. Only owned pixels do.
use crate::frame::{
    copy_pixels, validate_size, CapturedFrame, FrameStore, Layout, PixelRegion, RawFrame, MAX_SIDE,
};
use crate::StreamGrant;
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::{
    buffer::meta::{MetaHeader, MetaHeaderFlags, MetaVideoCrop, MetaVideoTransform},
    pod::Pod,
};
use std::{
    cell::{Cell, RefCell},
    os::fd::OwnedFd,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;

pub(crate) struct CaptureWorker {
    stop: Arc<AtomicBool>,
    owner: Option<JoinHandle<Result<(), String>>>,
}
impl CaptureWorker {
    pub(crate) fn start(
        fd: OwnedFd,
        stream: StreamGrant,
        run_id: String,
        frames: Arc<FrameStore>,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let cancellation = stop.clone();
        let owner = tokio::task::spawn_blocking(move || {
            let result = consume(fd, stream, run_id, &cancellation, frames.clone());
            frames.revoke();
            result
        });
        Self {
            stop,
            owner: Some(owner),
        }
    }
    pub(crate) async fn finished(&mut self) -> Result<(), String> {
        if let Some(owner) = self.owner.as_mut() {
            let result = owner
                .await
                .map_err(|e| format!("PipeWire owner failed: {e}"));
            self.owner.take();
            result?
        } else {
            Ok(())
        }
    }
    pub(crate) async fn stop(&mut self) -> Result<(), String> {
        self.stop.store(true, Ordering::Release);
        self.finished().await
    }
}
impl Drop for CaptureWorker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[derive(Clone, Copy)]
struct Format {
    width: u32,
    height: u32,
    layout: Layout,
}
struct State {
    format: Option<Format>,
    generation: u64,
    sequence: u64,
    run_id: String,
    node_id: u32,
    serial: u64,
    frames: Arc<FrameStore>,
    failure: Rc<RefCell<Option<String>>>,
}
impl State {
    fn fail(&self, message: impl Into<String>) {
        self.frames.revoke();
        *self.failure.borrow_mut() = Some(message.into());
    }
}

fn consume(
    fd: OwnedFd,
    grant: StreamGrant,
    run_id: String,
    stop: &AtomicBool,
    frames: Arc<FrameStore>,
) -> Result<(), String> {
    // pw_init is idempotent in pipewire-rs (Once); never globally deinit another run.
    pw::init();
    let mainloop = pw::main_loop::MainLoopBox::new(None).map_err(err)?;
    let context = pw::context::ContextBox::new(mainloop.loop_(), None).map_err(err)?;
    // Crucially never connect() / default socket / environment-selected remote.
    let core = context.connect_fd(fd, None).map_err(err)?;
    let failure = Rc::new(RefCell::new(None::<String>));
    let error = failure.clone();
    let _core_listener = core
        .add_listener_local()
        .error(move |_, _, res, message| {
            *error.borrow_mut() = Some(format!("PipeWire core error {res}: {message}"));
        })
        .register();
    let registry = core.get_registry().map_err(err)?;
    let found = Rc::new(Cell::new(None::<u64>));
    let observed = found.clone();
    let error = failure.clone();
    let removed = failure.clone();
    let node_id = grant.node_id;
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            if global.id != node_id {
                return;
            }
            let serial = global
                .props
                .as_ref()
                .and_then(|p| p.get("object.serial"))
                .and_then(|s| s.parse::<u64>().ok());
            if global.type_ != pw::types::ObjectType::Node
                || serial.is_none_or(|s| s == 0)
                || grant
                    .pipewire_serial
                    .is_some_and(|expected| Some(expected) != serial)
            {
                *error.borrow_mut() = Some("portal PipeWire node/serial identity mismatch".into());
            } else {
                observed.set(serial);
            }
        })
        .global_remove(move |id| {
            if id == node_id {
                *removed.borrow_mut() = Some("portal PipeWire node removed".into());
            }
        })
        .register();
    // Complete the initial registry snapshot before considering any node usable.
    let done = Rc::new(Cell::new(false));
    let sync = core.sync(0).map_err(err)?;
    let complete = done.clone();
    let _done = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == sync {
                complete.set(true);
            }
        })
        .register();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done.get() {
        if stop.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(message) = failure.borrow_mut().take() {
            return Err(message);
        }
        if Instant::now() >= deadline {
            return Err("PipeWire registry deadline".into());
        }
        mainloop
            .loop_()
            .iterate(pw::loop_::Timeout::Finite(Duration::from_millis(20)));
    }
    if let Some(message) = failure.borrow_mut().take() {
        return Err(message);
    }
    let serial = found
        .get()
        .ok_or("portal PipeWire node absent from granted remote")?;
    let result = (|| -> Result<(), String> {
        let stream = pw::stream::StreamBox::new(
            &core,
            "Grok authorized capture",
            properties! {
                "media.type" => "Video", "media.category" => "Capture", "media.role" => "Screen",
                "node.name" => format!("grok-cu-capture-{}", uuid::Uuid::new_v4().simple()),
                // Serial, never reusable ID or default source, drives autoconnection.
                "target.object" => serial.to_string(), "node.dont-reconnect" => "true",
            },
        )
        .map_err(err)?;
        let state = State {
            format: None,
            generation: 0,
            sequence: 0,
            run_id,
            node_id,
            serial,
            frames: frames.clone(),
            failure: failure.clone(),
        };
        let _listener = stream
            .add_local_listener_with_user_data(state)
            .state_changed(|_, state, _, new| {
                if !matches!(new, pw::stream::StreamState::Streaming) {
                    state.frames.clear();
                }
                match new {
                    pw::stream::StreamState::Error(message) => {
                        state.fail(format!("PipeWire stream error: {message}"))
                    }
                    pw::stream::StreamState::Unconnected => {
                        state.fail("PipeWire stream disconnected")
                    }
                    _ => {}
                }
            })
            .param_changed(|stream, state, id, param| {
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }
                state.frames.clear();
                state.format = None;
                let Some(param) = param else {
                    return;
                };
                let parsed = parse_format(param);
                match parsed {
                    Ok(format) => {
                        state.generation += 1;
                        state.format = Some(format);
                        if let Err(message) = negotiate_buffers(stream) {
                            state.fail(message);
                        }
                    }
                    Err(message) => state.fail(message),
                }
            })
            .process(|stream, state| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                // Buffer Drop always requeues, including malformed / stale frames.
                let Some(format) = state.format else {
                    return;
                };
                match decode_buffer(&mut buffer, format) {
                    Ok(Some((region, transform, pixels))) => {
                        state.sequence += 1;
                        state.frames.publish(CapturedFrame {
                            run_id: state.run_id.clone(),
                            node_id: state.node_id,
                            pipewire_serial: state.serial,
                            sequence: state.sequence,
                            format_generation: state.generation,
                            region,
                            buffer_width: format.width,
                            buffer_height: format.height,
                            transform,
                            captured_at: Instant::now(),
                            rgba: pixels,
                        });
                    }
                    Ok(None) => state.frames.clear(),
                    Err(message) => state.fail(message),
                }
            })
            .register()
            .map_err(err)?;
        let values = format_offer()?;
        let mut params = [Pod::from_bytes(&values).ok_or("invalid local format offer")?];
        stream
            .connect(
                spa::utils::Direction::Input,
                None,
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            )
            .map_err(err)?;
        while !stop.load(Ordering::Acquire) {
            if let Some(message) = failure.borrow_mut().take() {
                return Err(message);
            }
            let result = mainloop
                .loop_()
                .iterate(pw::loop_::Timeout::Finite(Duration::from_millis(20)));
            if result < 0 && result != -libc::EINTR {
                return Err(format!("PipeWire loop failed: {result}"));
            }
        }
        Ok(())
    })();
    // All paths after stream construction destroy its listener and stream
    // before this roundtrip, including format/loop/source failures. Local RAII
    // alone does not prove the daemon has processed the destroy request.
    frames.revoke();
    let retirement = acknowledge_retirement(&core, mainloop.loop_());
    match (result, retirement) {
        (Ok(()), result) | (result, Ok(())) => result,
        (Err(capture), Err(retirement)) => Err(format!("{capture}; {retirement}")),
    }
}

fn acknowledge_retirement(core: &pw::core::Core, loop_: &pw::loop_::Loop) -> Result<(), String> {
    // PipeWire core.sync is an ordered method/event barrier on this granted
    // connection, never a new/default remote connection. Do not cancel this
    // wait just because Stop is set: the original owner still owns cleanup.
    let sequence = core
        .sync(0)
        .map_err(|e| format!("PipeWire retirement sync: {e}"))?;
    let done = Rc::new(Cell::new(false));
    let completed = done.clone();
    let failure = Rc::new(RefCell::new(None));
    let failed = failure.clone();
    let _listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id == pw::core::PW_ID_CORE && seq == sequence {
                completed.set(true);
            }
        })
        .error(move |_, _, res, message| {
            *failed.borrow_mut() = Some(format!("PipeWire retirement error {res}: {message}"));
        })
        .register();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(message) = failure.borrow_mut().take() {
            return Err(message);
        }
        if done.get() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("PipeWire retirement acknowledgement deadline".into());
        }
        let result = loop_.iterate(pw::loop_::Timeout::Finite(Duration::from_millis(20)));
        if result < 0 && result != -libc::EINTR {
            return Err(format!("PipeWire retirement loop failed: {result}"));
        }
    }
}

fn parse_format(param: &Pod) -> Result<Format, String> {
    let (kind, subtype) = spa::param::format_utils::parse_format(param).map_err(err)?;
    if kind != spa::param::format::MediaType::Video
        || subtype != spa::param::format::MediaSubtype::Raw
    {
        return Err("non-raw PipeWire video format".into());
    }
    let mut info = spa::param::video::VideoInfoRaw::default();
    info.parse(param).map_err(err)?;
    use spa::param::video::VideoFormat as V;
    let layout = match info.format() {
        V::RGBA => Layout::Rgba,
        V::RGBx => Layout::Rgbx,
        V::BGRA => Layout::Bgra,
        V::BGRx => Layout::Bgrx,
        V::RGB => Layout::Rgb,
        V::BGR => Layout::Bgr,
        _ => return Err("unsupported negotiated PipeWire pixel format".into()),
    };
    validate_size(info.size().width, info.size().height)?;
    Ok(Format {
        width: info.size().width,
        height: info.size().height,
        layout,
    })
}

type Decoded = Option<(PixelRegion, crate::VideoTransform, Arc<[u8]>)>;
fn decode_buffer(buffer: &mut pw::buffer::Buffer<'_>, format: Format) -> Result<Decoded, String> {
    if buffer.find_meta::<MetaHeader>().is_some_and(|m| {
        m.flags()
            .intersects(MetaHeaderFlags::CORRUPTED | MetaHeaderFlags::GAP)
    }) {
        return Ok(None);
    }
    let transform = buffer
        .find_meta::<MetaVideoTransform>()
        .map(|m| crate::VideoTransform::from_raw(m.transform().as_raw()))
        .transpose()?
        .unwrap_or(crate::VideoTransform::Normal);
    let crop = buffer
        .find_meta::<MetaVideoCrop>()
        .map(|m| m.meta_region())
        .filter(|r| r.is_valid())
        .map(|r| PixelRegion {
            x: r.position().x as u32,
            y: r.position().y as u32,
            width: r.size().width,
            height: r.size().height,
        });
    let planes = buffer.datas_mut();
    if planes.len() != 1 {
        return Err("expected exactly one packed PipeWire plane".into());
    }
    let plane = &mut planes[0];
    if plane.as_raw().chunk.is_null() {
        return Err("PipeWire plane lacks chunk".into());
    }
    if plane
        .chunk()
        .flags()
        .contains(spa::buffer::ChunkFlags::CORRUPTED)
        || plane.chunk().size() == 0
    {
        return Ok(None);
    }
    // No unsafe dmabuf mapping or implicit global capture fallback. The offer
    // omits modifiers; the producer must supply mappable CPU buffers.
    if !matches!(
        plane.type_(),
        spa::buffer::DataType::MemFd | spa::buffer::DataType::MemPtr
    ) {
        return Err("PipeWire did not supply CPU-mappable video memory".into());
    }
    let (offset, size, stride) = (
        plane.chunk().offset(),
        plane.chunk().size(),
        plane.chunk().stride(),
    );
    let data = plane.data().ok_or("PipeWire plane is not mapped")?;
    copy_pixels(RawFrame {
        data,
        offset,
        size,
        stride,
        width: format.width,
        height: format.height,
        layout: format.layout,
        crop,
    })
    .map(|(region, pixels)| Some((region, transform, pixels)))
}

fn format_offer() -> Result<Vec<u8>, String> {
    use spa::param::{
        format::{FormatProperties as P, MediaSubtype, MediaType},
        video::VideoFormat as V,
        ParamType,
    };
    let object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        ParamType::EnumFormat,
        spa::pod::property!(P::MediaType, Id, MediaType::Video),
        spa::pod::property!(P::MediaSubtype, Id, MediaSubtype::Raw),
        spa::pod::property!(
            P::VideoFormat,
            Choice,
            Enum,
            Id,
            V::BGRx,
            V::BGRx,
            V::BGRA,
            V::RGBx,
            V::RGBA,
            V::RGB,
            V::BGR
        ),
        spa::pod::property!(
            P::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: MAX_SIDE,
                height: MAX_SIDE
            }
        ),
        spa::pod::property!(
            P::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 30, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 60, denom: 1 }
        )
    );
    Ok(spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map_err(err)?
    .0
    .into_inner())
}
fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn negotiate_buffers(stream: &pw::stream::Stream) -> Result<(), String> {
    use spa::{
        pod::{ChoiceValue, Object, Property, Value},
        utils::{Choice, ChoiceEnum, ChoiceFlags, Id},
    };
    let mask = (1 << spa::sys::SPA_DATA_MemPtr) | (1 << spa::sys::SPA_DATA_MemFd);
    let mut objects = vec![Object {
        type_: spa::sys::SPA_TYPE_OBJECT_ParamBuffers,
        id: spa::sys::SPA_PARAM_Buffers,
        properties: vec![
            Property::new(
                spa::sys::SPA_PARAM_BUFFERS_dataType,
                Value::Choice(ChoiceValue::Int(Choice(
                    ChoiceFlags::empty(),
                    ChoiceEnum::Flags {
                        default: mask,
                        flags: vec![mask],
                    },
                ))),
            ),
            Property::new(spa::sys::SPA_PARAM_BUFFERS_blocks, Value::Int(1)),
        ],
    }];
    for (kind, size) in [
        (
            spa::sys::SPA_META_Header,
            std::mem::size_of::<spa::sys::spa_meta_header>(),
        ),
        (
            spa::sys::SPA_META_VideoCrop,
            std::mem::size_of::<spa::sys::spa_meta_region>(),
        ),
        (
            spa::sys::SPA_META_VideoTransform,
            std::mem::size_of::<spa::sys::spa_meta_videotransform>(),
        ),
    ] {
        objects.push(Object {
            type_: spa::sys::SPA_TYPE_OBJECT_ParamMeta,
            id: spa::sys::SPA_PARAM_Meta,
            properties: vec![
                Property::new(spa::sys::SPA_PARAM_META_type, Value::Id(Id(kind))),
                Property::new(spa::sys::SPA_PARAM_META_size, Value::Int(size as i32)),
            ],
        });
    }
    let values: Vec<Vec<u8>> = objects
        .into_iter()
        .map(|object| {
            spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &Value::Object(object),
            )
            .map(|v| v.0.into_inner())
            .map_err(err)
        })
        .collect::<Result<_, _>>()?;
    let mut params = values
        .iter()
        .map(|bytes| Pod::from_bytes(bytes).ok_or_else(|| "invalid local buffer offer".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    stream.update_params(&mut params).map_err(err)
}
