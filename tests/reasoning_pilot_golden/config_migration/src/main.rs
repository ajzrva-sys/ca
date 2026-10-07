use std::io::{self, Read};

fn main() {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .expect("failed to read stdin");
    let config = reasoning_pilot_config_migration::load_config(&input).expect("invalid config");
    println!("provider_url={}", config.provider_url);
    println!("model={}", config.model);
    println!("timeout_seconds={}", config.timeout_seconds);
}
