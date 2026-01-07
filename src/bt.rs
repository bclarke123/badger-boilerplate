use heapless::Vec;
use serde::Deserialize;
use trouble_host::prelude::*;

use crate::state::{DISPLAY_CHANGED, LABEL, Screen};

// GATT Server definition
#[gatt_server]
pub struct Server {
    display_service: DisplayService,
}

/// Display service
#[gatt_service(uuid = "9ecadc24-0ee5-a9e0-93f3-a3b50100406e")]
pub struct DisplayService {
    #[characteristic(uuid = "9ecadc24-0ee5-a9e0-93f3-a3b50200406e", write, read, notify)]
    status: Vec<u8, 512>,
}

#[derive(Deserialize)]
pub enum Status {
    Busy,
    Free,
    Focus,
}

#[derive(Deserialize)]
pub struct CalendarInfo<'a> {
    pub status: Status,
    pub start_time: [u8; 2],
    pub duration: u8,
    pub label: &'a str,
}

/// Create an advertiser to use to connect to a BLE Central, and wait for it to connect.
pub async fn advertise<'values, 'server, C: Controller>(
    name: &'values str,
    peripheral: &mut Peripheral<'values, C, DefaultPacketPool>,
    server: &'server Server<'values>,
) -> Result<GattConnection<'values, 'server, DefaultPacketPool>, BleHostError<C::Error>> {
    let mut advertiser_data = [0; 31];
    let len = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut advertiser_data[..],
    )?;
    let advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &advertiser_data[..len],
                scan_data: &[],
            },
        )
        .await?;
    let conn = advertiser.accept().await?.with_attribute_server(server)?;
    Ok(conn)
}

pub async fn gatt_events_task<P: PacketPool>(
    server: &Server<'_>,
    conn: &GattConnection<'_, '_, P>,
) -> Result<(), Error> {
    let status = &server.display_service.status;
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => break,
            GattConnectionEvent::Gatt { event } => {
                match &event {
                    GattEvent::Write(event) => {
                        if event.handle() == status.handle {
                            let value = server.get(status).ok();
                            update_from_bytes(value).await;
                        }
                    }
                    _ => {}
                };
                // This step is also performed at drop(), but writing it explicitly is necessary
                // in order to ensure reply is sent.
                match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(_) => {}
                };
            }
            _ => {} // ignore other Gatt Connection Events
        }
    }

    Ok(())
}

async fn update_from_bytes(bytes: Option<Vec<u8, 512>>) {
    if let Some(bytes) = bytes {
        if let Ok((info, _bytes)) = serde_json_core::from_slice::<CalendarInfo>(&bytes) {
            let label = &mut *LABEL.lock().await;
            label.clear();
            core::fmt::write(label, format_args!("{}", info.label)).ok();

            DISPLAY_CHANGED.signal(Screen::Full);
        }
    }
}
