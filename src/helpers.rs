use core::fmt::Arguments;
use heapless::String;
use serde_json_core::heapless::Vec;
use time::{Date, Month, PrimitiveDateTime, Time};

/// Makes it easier to format strings in a single line method
pub fn easy_format<const N: usize>(args: Arguments<'_>) -> String<N> {
    let mut formatted_string: String<N> = String::<N>::new();
    let result = core::fmt::write(&mut formatted_string, args);
    match result {
        Ok(_) => formatted_string,
        Err(_) => {
            panic!("Error formatting the string")
        }
    }
}

pub fn parse_rfc3339(when: &str) -> PrimitiveDateTime {
    //split at T
    let datetime = when.split('T').collect::<Vec<&str, 2>>();
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
