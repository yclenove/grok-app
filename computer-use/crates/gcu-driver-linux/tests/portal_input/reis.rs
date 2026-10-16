//! Real reis wire protocol against an in-process EIS fixture; no desktop or child process.
use super::*;
use reis::{
    eis,
    handshake::EisHandshaker,
    request::{DeviceCapability, EisRequest, EisRequestConverter},
    PendingRequestResult,
};
use std::os::unix::net::UnixStream;

async fn request(socket: &tokio::io::unix::AsyncFd<eis::Context>) -> Option<eis::Request> {
    loop {
        if let Some(result) = socket.get_ref().pending_request() {
            return match result {
                PendingRequestResult::Request(request) => Some(request),
                _ => panic!("invalid fixture request"),
            };
        }
        let mut ready = socket.readable().await.unwrap();
        if socket.get_ref().read().is_err() {
            return None;
        }
        ready.clear_ready();
    }
}

#[tokio::test(start_paused = true)]
async fn portal_fd_reis_frames_arrive_and_pause_invalidates_generation() {
    let (client, server) = UnixStream::pair().unwrap();
    let (events, mut receive) = tokio::sync::mpsc::unbounded_channel();
    let (ready, device_ready) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let context = eis::Context::new(server).unwrap();
        let socket = tokio::io::unix::AsyncFd::new(context.clone()).unwrap();
        let mut handshake = EisHandshaker::new(&context, 1);
        let response = loop {
            let response = handshake
                .handle_request(request(&socket).await.unwrap())
                .unwrap();
            context.flush().unwrap();
            if let Some(response) = response {
                break response;
            }
        };
        let mut converter = EisRequestConverter::new(&context, response, 1);
        let connection = converter.handle().clone();
        let caps = DeviceCapability::PointerAbsolute
            | DeviceCapability::Button
            | DeviceCapability::Scroll
            | DeviceCapability::Keyboard
            | DeviceCapability::Text;
        let _seat = connection.add_seat(Some("fixture"), caps);
        connection.flush().unwrap();
        let mut ready = Some(ready);
        while let Some(raw) = request(&socket).await {
            converter.handle_request(raw).unwrap();
            while let Some(event) = converter.next_request() {
                match event {
                    EisRequest::Bind(bind) => {
                        let device = bind.seat.add_device(
                            Some("fixture"),
                            eis::device::DeviceType::Virtual,
                            caps,
                            |d| {
                                d.device().region_mapping_id("m1");
                                d.device().region(100, 200, 800, 600, 2.0);
                            },
                        );
                        device.resumed();
                        connection.flush().unwrap();
                        ready
                            .take()
                            .unwrap()
                            .send((device, connection.clone()))
                            .unwrap();
                    }
                    EisRequest::KeyboardKey(key) => events
                        .send(InputEvent::Key {
                            code: key.key,
                            pressed: key.state == eis::keyboard::KeyState::Press,
                        })
                        .unwrap(),
                    EisRequest::Button(button) => events
                        .send(InputEvent::Button {
                            code: button.button,
                            pressed: button.state == eis::button::ButtonState::Press,
                        })
                        .unwrap(),
                    EisRequest::PointerMotionAbsolute(point) => events
                        .send(InputEvent::Absolute(point.dx_absolute, point.dy_absolute))
                        .unwrap(),
                    EisRequest::TextUtf8(text) => events.send(InputEvent::Text(text.text)).unwrap(),
                    _ => {}
                }
            }
            connection.flush().unwrap();
        }
    });
    let transport = drive(gcu_driver_linux::wayland::ei::ReisTransport::connect(
        client.into(),
        "m1".into(),
    ))
    .await
    .unwrap();
    let (device, connection) = device_ready.await.unwrap();
    let generation = transport.snapshot().unwrap().generation;
    let expected = [
        InputEvent::Absolute(500., 500.),
        InputEvent::Key {
            code: 42,
            pressed: true,
        },
        InputEvent::Key {
            code: 42,
            pressed: false,
        },
        InputEvent::Button {
            code: 0x110,
            pressed: true,
        },
        InputEvent::Button {
            code: 0x110,
            pressed: false,
        },
        InputEvent::Text("中文👩‍💻".into()),
    ];
    for event in &expected {
        transport.send(event, Some(generation)).unwrap();
    }
    for event in expected {
        assert_eq!(drive(receive.recv()).await.unwrap(), event);
    }
    device.paused();
    connection.flush().unwrap();
    drive(async {
        while transport.snapshot().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(transport
        .send(
            &InputEvent::Key {
                code: 42,
                pressed: true
            },
            Some(generation)
        )
        .is_err());
    transport.disconnect();
    drive(server).await.unwrap();
}
