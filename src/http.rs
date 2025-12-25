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

use crate::RtcDevice;
use crate::state::{CurrentWeather, POWER_MUTEX, RTC_TIME, WEATHER};

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

pub async fn fetch_time(stack: &Stack<'_>, rx_buf: &mut [u8], rtc_device: &RtcDevice) {
    let _guard = POWER_MUTEX.lock().await;

    if let Ok(response) = fetch_api::<TimeApiResponse>(stack, rx_buf, TIME_API).await {
        let datetime: PrimitiveDateTime = response.into();

        rtc_device
            .lock()
            .await
            .set_datetime(&datetime)
            .await
            .expect("Failed to update RTC time");

        let mut data = RTC_TIME.lock().await;
        *data = Some(datetime);
    }
}

pub async fn fetch_weather(stack: &Stack<'_>, rx_buf: &mut [u8]) {
    let _guard = POWER_MUTEX.lock().await;

    if let Ok(response) = fetch_api::<OpenMeteoResponse>(stack, rx_buf, TEMP_API).await {
        info!(
            "Temp: {}C, Code: {}",
            response.current.temperature, response.current.weathercode
        );

        let mut data = WEATHER.lock().await;
        *data = Some(response.current);
    }
}

#[derive(Deserialize)]
struct TimeApiResponse<'a> {
    datetime: &'a str,
}

impl<'a> From<TimeApiResponse<'a>> for PrimitiveDateTime {
    fn from(response: TimeApiResponse) -> Self {
        info!("Datetime: {:?}", response.datetime);
        //split at T
        let datetime = response.datetime.split('T').collect::<Vec<&str, 2>>();
        //split at -
        let date = datetime[0].split('-').collect::<Vec<&str, 3>>();
        let year = date[0].parse::<i32>().unwrap();
        let month = date[1].parse::<u8>().unwrap();
        let day = date[2].parse::<u8>().unwrap();
        //split at :
        let time = datetime[1].split(':').collect::<Vec<&str, 4>>();
        let hour = time[0].parse::<u8>().unwrap();
        let minute = time[1].parse::<u8>().unwrap();
        //split at .
        let second_split = time[2].split('.').collect::<Vec<&str, 2>>();
        let second = second_split[0].parse::<u8>().unwrap();

        let date = Date::from_calendar_date(year, Month::try_from(month).unwrap(), day).unwrap();
        let time = Time::from_hms(hour, minute, second).unwrap();

        PrimitiveDateTime::new(date, time)
    }
}

#[derive(Deserialize)]
pub struct OpenMeteoResponse {
    pub current: CurrentWeather,
}
