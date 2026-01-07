use defmt::{Format, error};
use embassy_net::Stack;
use embassy_net::dns::DnsSocket;
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use heapless::Vec;
use log::info;
use reqwless::client::HttpClient;
use reqwless::request::{Method, RequestBuilder};
use serde::Deserialize;
use time::{Date, Month, PrimitiveDateTime, Time};

use crate::helpers::parse_rfc3339;
use crate::state::{CurrentWeather, POWER_MUTEX, WEATHER};
use crate::time::set_time;
use crate::{FlashDevice, RtcDevice, flash};

static TIME_API: &str = env!("TIME_API");
static TEMP_API: &str = env!("TEMP_API");

#[derive(Format)]
pub struct HttpError;

pub async fn http_get<'a, 'b>(
    stack: &Stack<'a>,
    url: &str,
    buf: &'b mut [u8],
) -> Result<&'b [u8], HttpError> {
    let dns_client = DnsSocket::new(*stack);

    let client_state = TcpClientState::<1, 1024, 1024>::new();
    let client = TcpClient::<'_, 1>::new(*stack, &client_state);

    let mut http_client = HttpClient::new(&client, &dns_client);

    let req = http_client.request(Method::GET, url).await;

    if let Err(e) = req {
        error!("Failed to send HTTP request: {:?}", e);
        return Err(HttpError);
    }

    let mut req = req.unwrap().headers(&[
        ("Accept", "*/*"),
        ("User-Agent", "Rusty-Badger/1.0"),
        ("Connection", "close"),
    ]);

    let response = req.send(buf).await;

    if let Err(e) = response {
        error!("Failed to send HTTP request: {:?}", e);
        return Err(HttpError);
    }

    let response = response.unwrap();

    let body_bytes = response.body().read_to_end().await;

    match body_bytes {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            error!("Failed to read HTTP response body: {:?}", e);
            Err(HttpError)
        }
    }
}

pub async fn fetch_api<'a, T>(stack: &Stack<'_>, rx_buf: &'a mut [u8], url: &str) -> Result<T, ()>
where
    T: Deserialize<'a>,
{
    match http_get(stack, url, rx_buf).await {
        Ok(bytes) => match serde_json_core::de::from_slice::<T>(bytes) {
            Ok((response, _)) => Ok(response),
            Err(_e) => {
                error!("Failed to parse response body");
                Err(())
            }
        },
        Err(e) => {
            error!("Failed to make API request: {:?}", e);
            Err(())
        }
    }
}

pub async fn fetch_time(stack: &Stack<'_>, rx_buf: &mut [u8], rtc_device: &'static RtcDevice) {
    let _guard = POWER_MUTEX.lock().await;

    if let Ok(response) = fetch_api::<TimeApiResponse>(stack, rx_buf, TIME_API).await {
        set_time(rtc_device, response.into()).await;
    }
}

pub async fn fetch_weather(
    stack: &Stack<'_>,
    rx_buf: &mut [u8],
    flash_device: &'static FlashDevice,
) {
    let _guard = POWER_MUTEX.lock().await;

    if let Ok(response) = fetch_api::<OpenMeteoResponse>(stack, rx_buf, TEMP_API).await {
        let weather = response.current;

        info!(
            "Temp: {}C, Code: {}",
            weather.temperature, weather.weathercode
        );

        {
            let mut data = WEATHER.lock().await;
            *data = Some(weather);
        }

        flash::save_state(flash_device).await;
    }
}

#[derive(Deserialize)]
pub struct TimeApiResponse<'a> {
    pub datetime: &'a str,
}

impl<'a> From<TimeApiResponse<'a>> for PrimitiveDateTime {
    fn from(response: TimeApiResponse) -> Self {
        info!("Datetime: {:?}", response.datetime);
        parse_rfc3339(response.datetime)
    }
}

#[derive(Deserialize)]
pub struct OpenMeteoResponse {
    pub current: CurrentWeather,
}
