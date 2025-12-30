# Badger 2040 W Starter Kit

Fully async, memory-safe, battery-friendly runtime for the Pimoroni Badger 2040 W

## Features
* Dual mode operation - on battery, RTC alarms and buttons trigger one-shot updates before returning to deep sleep. On USB power, efficient tasks handle subsystems for continuous operation.
* RTC alarm wakes the device once per minute, to update the clock. The onboard RTC contains one byte of available RAM, which is currently used to remember a selected bitmap image to display. The RTC "default time" flag is checked on startup, and the time is only displayed if it's been set from the Internet.
* WiFi periodic sync to batch fetch time / weather information from HTTP, every hour on the hour
* PWM-driven LED, allows for smooth brightness animations and status signals without waking the screen
* Flash memory implementation for serializing / deserializing the current weather from OpenMeteo

## This project would not be possible without..
* [fatfingers23](https://github.com/fatfingers23) for giving this project its starting point
* [trvswgnr](https://github.com/trvswgnr) for their amazing ferris with a knife image. All i did was badly convert it to grayscale and scaled it down. 
* embassy framework and their great [examples](https://github.com/embassy-rs/embassy/tree/main/examples/rp). Exactly zero chance I would have any of this written without this directory.
* the [uc8151-rs](https://crates.io/crates/uc8151) crate. Would not be able to write to the e ink display without this great crate.
* And every other single crate found in [Cargo.toml](./Cargo.toml). None of it would be possible with out those packages and maintainers.
