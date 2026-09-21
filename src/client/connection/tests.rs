use super::*;
use crate::client::auth::{AuthHelper, AuthResult};
use crate::{VncConnector, VncVersion};
use std::time::Duration;
use tokio::io::{duplex, DuplexStream};

pub(super) async fn handshake(server: &mut DuplexStream, size: (u16, u16)) {
    handshake_auth(server, size, false).await;
}

async fn handshake_auth(server: &mut DuplexStream, size: (u16, u16), password: bool) {
    handshake_named(server, size, password, "test").await;
}

async fn handshake_named(server: &mut DuplexStream, size: (u16, u16), password: bool, name: &str) {
    server.write_all(b"RFB 003.008\n").await.unwrap();
    let mut version = [0; 12];
    server.read_exact(&mut version).await.unwrap();
    assert_eq!(&version, b"RFB 003.008\n");
    let security = if password { 2 } else { 1 };
    server.write_all(&[1, security]).await.unwrap();
    assert_eq!(server.read_u8().await.unwrap(), security);
    if password {
        server
            .write_all(&(0u8..16).collect::<Vec<_>>())
            .await
            .unwrap();
        let mut response = [0; 16];
        server.read_exact(&mut response).await.unwrap();
        // Independent DES-ECB reference: OpenSSL, bit-reversed password "test".
        assert_eq!(
            response,
            [
                0x51, 0xa8, 0x9f, 0xa0, 0x01, 0x3d, 0x72, 0xc6, 0x55, 0x01, 0x95, 0x13, 0xaf, 0x52,
                0xc2, 0x0c
            ]
        );
    }
    server.write_u32(0).await.unwrap();
    assert_eq!(server.read_u8().await.unwrap(), 1);
    server.write_u16(size.0).await.unwrap();
    server.write_u16(size.1).await.unwrap();
    server
        .write_all(&Vec::<u8>::from(PixelFormat::rgba()))
        .await
        .unwrap();
    server.write_u32(name.len() as u32).await.unwrap();
    server.write_all(name.as_bytes()).await.unwrap();
    let mut pixel_format = [0; 20];
    server.read_exact(&mut pixel_format).await.unwrap();
    assert_eq!(pixel_format[0], 0);
    assert_eq!(server.read_u8().await.unwrap(), 2);
    server.read_u8().await.unwrap();
    let count = server.read_u16().await.unwrap();
    for _ in 0..count {
        server.read_u32().await.unwrap();
    }
    let mut request = [0; 10];
    server.read_exact(&mut request).await.unwrap();
    assert_eq!(request[0], 3);
}

pub(super) async fn connect(stream: DuplexStream) -> VncClient {
    VncConnector::new(stream)
        .set_auth_method(async { Ok("test".to_string()) })
        .set_pixel_format(PixelFormat::rgba())
        .add_encoding(VncEncoding::Raw)
        .add_encoding(VncEncoding::CopyRect)
        .add_encoding(VncEncoding::Zrle)
        .add_encoding(VncEncoding::DesktopSizePseudo)
        .build()
        .unwrap()
        .try_start()
        .await
        .unwrap()
        .finish()
        .unwrap()
}

pub(super) async fn event(client: &VncClient) -> VncEvent {
    tokio::time::timeout(Duration::from_secs(2), client.recv_event())
        .await
        .unwrap()
        .unwrap()
}

pub(super) fn rect_header(rect: Rect, encoding: u32) -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 1];
    for value in [rect.x, rect.y, rect.width, rect.height] {
        bytes.extend(value.to_be_bytes());
    }
    bytes.extend(encoding.to_be_bytes());
    bytes
}

#[tokio::test]
async fn password_result_is_checked_without_invalid_enum() {
    for (number, expected) in [
        (0, Some(AuthResult::Ok)),
        (1, Some(AuthResult::Failed)),
        (2, None),
        (u32::MAX, None),
    ] {
        let mut bytes = vec![0; 16];
        bytes.extend(number.to_be_bytes());
        let mut input = std::io::Cursor::new(bytes);
        let auth = AuthHelper::read(&mut input, "test").await.unwrap();
        match expected {
            Some(expected) => assert_eq!(auth.finish(&mut input).await.unwrap(), expected),
            None => assert!(auth.finish(&mut input).await.is_err()),
        }
    }
}

#[tokio::test]
async fn no_auth_security_failure_is_not_ignored() {
    for result in [1u32, 2, u32::MAX] {
        let (stream, mut server) = duplex(512);
        let task = tokio::spawn(async move {
            server.write_all(b"RFB 003.008\n").await.unwrap();
            let mut version = [0; 12];
            server.read_exact(&mut version).await.unwrap();
            server.write_all(&[1, 1]).await.unwrap();
            server.read_u8().await.unwrap();
            server.write_u32(result).await.unwrap();
            if result == 1 {
                server.write_u32(4).await.unwrap();
                server.write_all(b"nope").await.unwrap();
            }
        });
        let result = VncConnector::new(stream)
            .set_auth_method(async { Ok("test".into()) })
            .add_encoding(VncEncoding::Raw)
            .build()
            .unwrap()
            .try_start()
            .await;
        assert!(result.is_err());
        task.await.unwrap();
    }
}

#[tokio::test]
async fn password_failure_33_does_not_wait_for_nonexistent_reason() {
    let (stream, mut server) = duplex(512);
    let (release, wait_for_release) = oneshot::channel();
    let task = tokio::spawn(async move {
        server.write_all(b"RFB 003.003\n").await.unwrap();
        let mut version = [0; 12];
        server.read_exact(&mut version).await.unwrap();
        server.write_u32(2).await.unwrap();
        server.write_all(&[0; 16]).await.unwrap();
        let mut response = [0; 16];
        server.read_exact(&mut response).await.unwrap();
        server.write_u32(1).await.unwrap();
        // Hold the stream open: a wrong implementation waits for a reason.
        let _ = wait_for_release.await;
    });
    let result = tokio::time::timeout(
        Duration::from_secs(1),
        VncConnector::new(stream)
            .set_auth_method(async { Ok("test".into()) })
            .set_version(VncVersion::RFB33)
            .add_encoding(VncEncoding::Raw)
            .build()
            .unwrap()
            .try_start(),
    )
    .await
    .unwrap();
    assert!(matches!(result, Err(VncError::WrongPassword)));
    let _ = release.send(());
    task.await.unwrap();
}

#[tokio::test]
async fn oversized_frame_and_name_rejected_before_payload() {
    for (width, height, name_len) in [
        (u16::MAX, 1, 0),
        (8192, 8192, 0),
        (0, 1, 0),
        (10, 10, u32::MAX),
    ] {
        let (mut client, mut server) = duplex(512);
        server.write_u16(width).await.unwrap();
        server.write_u16(height).await.unwrap();
        server
            .write_all(&Vec::<u8>::from(PixelFormat::rgba()))
            .await
            .unwrap();
        server.write_u32(name_len).await.unwrap();
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            read_server_init(&mut client, &mut Some(PixelFormat::rgba()), &|_| async {
                Ok(())
            }),
        )
        .await
        .unwrap();
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn resize_updates_actual_wire_refresh_dimensions() {
    let (stream, mut server) = duplex(4096);
    let task = tokio::spawn(async move {
        handshake(&mut server, (64, 64)).await;
        server
            .write_all(&rect_header(
                Rect {
                    x: 0,
                    y: 0,
                    width: 128,
                    height: 96,
                },
                VncEncoding::DesktopSizePseudo.into(),
            ))
            .await
            .unwrap();
        let mut request = [0; 10];
        server.read_exact(&mut request).await.unwrap();
        assert_eq!(request, [3, 1, 0, 0, 0, 0, 0, 128, 0, 96]);
        server
            .write_all(&rect_header(
                Rect {
                    x: 127,
                    y: 95,
                    width: 1,
                    height: 1,
                },
                VncEncoding::Raw.into(),
            ))
            .await
            .unwrap();
        server.write_all(&[1, 2, 3, 255]).await.unwrap();
    });
    let client = connect(stream).await;
    assert!(matches!(event(&client).await, VncEvent::SetResolution(_)));
    let VncEvent::SetResolution(size) = event(&client).await else {
        panic!("missing resize");
    };
    assert_eq!((size.width, size.height), (128, 96));
    client.input(X11Event::Refresh).await.unwrap();
    let VncEvent::RawImage(rect, data) = event(&client).await else {
        panic!("missing image");
    };
    assert_eq!((rect.x, rect.y, data), (127, 95, vec![1, 2, 3, 255]));
    client.close().await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn invalid_wire_encodings_bounds_and_copy_sources_fail() {
    let rect = Rect {
        x: 0,
        y: 0,
        width: 1,
        height: 1,
    };
    let mut copy = rect_header(rect, VncEncoding::CopyRect.into());
    copy.extend([0, 64, 0, 0]);
    let mut zrle = rect_header(rect, VncEncoding::Zrle.into());
    zrle.extend(u32::MAX.to_be_bytes());
    let mut clipboard = vec![3, 0, 0, 0];
    clipboard.extend(u32::MAX.to_be_bytes());
    for payload in [
        rect_header(rect, 12345),
        rect_header(rect, VncEncoding::Tight.into()),
        rect_header(rect, VncEncoding::CursorPseudo.into()),
        rect_header(rect, VncEncoding::LastRectPseudo.into()),
        rect_header(
            Rect {
                x: 0,
                y: 0,
                width: u16::MAX,
                height: 1,
            },
            VncEncoding::DesktopSizePseudo.into(),
        ),
        rect_header(Rect { x: 64, ..rect }, 0),
        copy,
        zrle,
        clipboard,
        vec![1],
    ] {
        let (stream, mut server) = duplex(4096);
        let task = tokio::spawn(async move {
            handshake(&mut server, (64, 64)).await;
            server.write_all(&payload).await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        });
        let client = connect(stream).await;
        event(&client).await;
        assert!(matches!(event(&client).await, VncEvent::Error(_)));
        client.close().await.unwrap();
        task.await.unwrap();
    }
}

#[tokio::test]
async fn close_interrupts_blocked_decoder_and_socket() {
    let (stream, mut server) = duplex(4096);
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        handshake(&mut server, (64, 64)).await;
        let _ = ready_tx.send(());
        // Fill the output queue and leave the decoder waiting to send an event.
        server.write_all(&[2; 100]).await.unwrap();
        let mut byte = [0];
        assert_eq!(server.read(&mut byte).await.unwrap(), 0);
    });
    let client = connect(stream).await;
    ready_rx.await.unwrap();
    tokio::task::yield_now().await;
    client.close().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn bounded_bridge_keeps_making_progress_without_more_input() {
    let (stream, mut server) = duplex(4096);
    let count = 10_000;
    let task = tokio::spawn(async move {
        handshake(&mut server, (64, 64)).await;
        server.write_all(&vec![2; count]).await.unwrap();
    });
    let client = connect(stream).await;
    event(&client).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        for _ in 0..count {
            assert!(matches!(client.recv_event().await.unwrap(), VncEvent::Bell));
        }
    })
    .await
    .unwrap();
    client.close().await.unwrap();
    task.await.unwrap();
}

#[tokio::test]
async fn password_authentication_succeeds_with_reference_challenge() {
    let (stream, mut server) = duplex(4096);
    let task = tokio::spawn(async move {
        handshake_auth(&mut server, (64, 64), true).await;
    });
    let client = connect(stream).await;
    assert!(matches!(event(&client).await, VncEvent::SetResolution(_)));
    task.await.unwrap();
    client.close().await.unwrap();
}

#[tokio::test]
async fn builder_rejects_unqualified_codecs_and_invalid_pixel_shifts() {
    for encoding in [
        VncEncoding::Tight,
        VncEncoding::Trle,
        VncEncoding::CursorPseudo,
    ] {
        let (stream, _server) = duplex(64);
        assert!(VncConnector::new(stream)
            .set_auth_method(async { Ok("test".into()) })
            .add_encoding(encoding)
            .build()
            .is_err());
    }
    for shift in [8, 32, 255] {
        let (stream, _server) = duplex(64);
        let mut format = PixelFormat::rgba();
        format.red_shift = shift;
        assert!(VncConnector::new(stream)
            .set_auth_method(async { Ok("test".into()) })
            .set_pixel_format(format)
            .add_encoding(VncEncoding::Raw)
            .build()
            .is_err());
    }
}

#[tokio::test]
async fn server_name_is_available_without_draining_frames_and_survives_clone() {
    for name in ["", "Test workstation ÆØÅ"] {
        let (client_stream, mut server) = duplex(4096);
        let server_task = tokio::spawn(async move {
            handshake_named(&mut server, (2, 2), false, name).await;
            server
        });
        let client = connect(client_stream).await;
        let _server = server_task.await.unwrap();
        assert_eq!(client.server_name(), name);
        let clone = client.clone();
        client.close().await.unwrap();
        assert_eq!(clone.server_name(), name);
    }
}
