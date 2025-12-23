const ENV_DATA: &str = include_str!("../.env");

pub fn env_value(key: &str) -> &'static str {
    for line in ENV_DATA.lines() {
        // let parts: Vec<&str, 2> = line.split('=').collect();
        if let Some((key_cur, value)) = line.split_once('=') {
            if key == key_cur {
                return value;
            }
        }
    }
    panic!(
        "Key: {:?} not found in .env file. May also need to provide your own .env from a copy of .env.save",
        key
    );
}
