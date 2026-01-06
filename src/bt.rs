use serde::Deserialize;
use trouble_host::prelude::*;

// GATT Server definition
#[gatt_server]
pub struct Server {
    display_service: DisplayService,
}

/// Display service
#[gatt_service(uuid = service::BATTERY)]
pub struct DisplayService {
    #[characteristic(uuid = "408813df-5dd4-1f87-ec11-cdb001100000", write, read, notify)]
    status: u8,
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
