// SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

use clap::Parser;

/// Bridge a Rad Pro Geiger counter to MQTT, with Home Assistant discovery.
#[derive(Parser, Debug, Clone)]
#[command(name = "radpro2mqtt", version, about, long_about = None)]
pub struct Cli {
    /// Serial port the Rad Pro device is attached to.
    #[arg(short = 'p', long, default_value = "/dev/ttyACM0", env = "RADPRO_PORT")]
    pub port: String,

    /// Serial baud rate (Rad Pro uses 115200 8N1).
    #[arg(long, default_value_t = 115_200, env = "RADPRO_BAUD")]
    pub baud: u32,

    /// Broker URL: mqtt://host:1883 or mqtts://host:8883.
    #[arg(short = 'b', long, env = "MQTT_URL")]
    pub mqtt_url: String,

    #[arg(short = 'u', long, env = "MQTT_USERNAME")]
    pub mqtt_username: Option<String>,

    #[arg(short = 'P', long, env = "MQTT_PASSWORD", hide_env_values = true)]
    pub mqtt_password: Option<String>,

    /// MQTT client id. Must be unique on the broker.
    #[arg(long, default_value = "radpro2mqtt", env = "MQTT_CLIENT_ID")]
    pub client_id: String,

    /// Seconds between device polls.
    #[arg(short = 'i', long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    pub interval: u64,

    /// Seconds to wait for a response to a single serial command.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: u64,

    /// Base topic for state and availability.
    #[arg(long, default_value = "radpro")]
    pub topic_prefix: String,

    /// Home Assistant discovery prefix.
    #[arg(long, default_value = "homeassistant")]
    pub discovery_prefix: String,

    /// Node id used in topics and unique ids. Must be unique per device.
    #[arg(long, default_value = "radpro")]
    pub node_id: String,

    /// Device name shown in Home Assistant.
    #[arg(long, default_value = "Rad Pro")]
    pub device_name: String,

    /// Retain state messages (discovery and availability are always retained).
    #[arg(long)]
    pub retain: bool,

    /// Do not publish Home Assistant discovery configs.
    #[arg(long)]
    pub no_discovery: bool,
}

impl Cli {
    pub fn state_topic(&self) -> String {
        format!("{}/{}/state", self.topic_prefix, self.node_id)
    }

    pub fn availability_topic(&self) -> String {
        format!("{}/{}/availability", self.topic_prefix, self.node_id)
    }
}
