//! Defines peripheral logic.

use bluer::{
    adv::Advertisement,
    gatt::local::{
        characteristic_control, Application, Characteristic, CharacteristicControlEvent,
        CharacteristicWrite, CharacteristicWriteMethod, Service,
    },
    Adapter,
};
use futures::{pin_mut, StreamExt};
use log::{debug, info};
use std::{collections::BTreeMap, str::from_utf8};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use uuid::Uuid;

/// Manufacturer ID for LE advertisement (testing ID used for now).
const TESTING_MANUFACTURER_ID: u16 = 0xffff;

/// Advertises a peripheral device:
///
/// - with adapter `adapter`
/// - named `device_name`
/// - with a service IDed as `svc_uuid`
/// - with a characteristic IDed as `proxy_device_name_char_uuid`
///
/// Waits for a BLE central to write a UTF8-encoded string to that characteristic and returns the
/// written value (or an error).
pub async fn advertise_and_find_proxy_device_name(
    adapter: &Adapter,
    device_name: String,
    svc_uuid: Uuid,
    proxy_device_name_char_uuid: Uuid,
) -> bluer::Result<String> {
    let mut manufacturer_data = BTreeMap::new();
    manufacturer_data.insert(
        TESTING_MANUFACTURER_ID,
        /*arbitrary data */ vec![0x21, 0x22, 0x23, 0x24],
    );
    let le_advertisement = Advertisement {
        service_uuids: vec![svc_uuid].into_iter().collect(),
        manufacturer_data,
        discoverable: Some(true),
        local_name: Some(device_name.clone()),
        ..Default::default()
    };
    let _adv_handle = Some(adapter.advertise(le_advertisement).await?);
    info!("Registered advertisement");

    let (char_control, char_handle) = characteristic_control();
    let app = Application {
        services: vec![Service {
            uuid: svc_uuid,
            primary: true,
            characteristics: vec![Characteristic {
                uuid: proxy_device_name_char_uuid,
                write: Some(CharacteristicWrite {
                    write: true,
                    // TODO(medium): Encrypt the char. Encrypting will force the mobile device to
                    // pair with the rock4 upon its initial connection attempt. Encrypting seems to
                    // sometimes cause write failure when the mobile device and the rock4 are
                    // already paired (writes are not seen in the select below). Check `btmon` for
                    // a `WriteResponse` error like unauthenticated. I think we have to `trust` the
                    // mobile device from the rock4.
                    //
                    //encrypt_write: true,
                    //encrypt_authenticated_write: true,
                    //secure_write: true,
                    method: CharacteristicWriteMethod::Io,
                    ..Default::default()
                }),
                control_handle: char_handle,
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let _app_handle = Some(adapter.serve_gatt_application(app).await?);

    info!("Advertising proxy device name char to be written to. Local device name: {device_name}");

    info!("Waiting for proxy device name to be written. Press enter to quit.");
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    pin_mut!(char_control);

    loop {
        tokio::select! {
            // TODO(low): Add a better select case than waiting for a new stdin line. Ideally, we
            // are just sensitive to SIGTERM/SIGINT. tokio::main may handle that for us already
            // but I have not tested.
            _ = lines.next_line() => break,
            evt = char_control.next() => {
                match evt {
                    Some(CharacteristicControlEvent::Write(req)) => {
                        debug!("Accepting write request event with MTU {}", req.mtu());
                        let mut read_buf = vec![0; req.mtu()];
                        let mut reader = req.accept()?;
                        let num_bytes = reader.read(&mut read_buf).await?;
                        let trimmed_read_buf = &read_buf[0..num_bytes];
                        match from_utf8(trimmed_read_buf) {
                                Ok(proxy_device_name_str) => {
                                    return Ok(proxy_device_name_str.to_string());
                                }
                                Err(e) => {
                                    return Err(bluer::Error {
                                        kind: bluer::ErrorKind::Failed,
                                        message: format!("Written proxy device name is not a UTF8-encoded string: {e}"),
                                    });
                                }
                            }
                    },
                    Some(CharacteristicControlEvent::Notify(notifier)) => {
                        debug!("Should not happen: accepting notify request event with MTU {}", notifier.mtu());
                    },
                    None => break,
                }
            },
        }
    }

    Err(bluer::Error {
        kind: bluer::ErrorKind::Failed,
        message: "Failed to collect a proxy device name".to_string(),
    })
}