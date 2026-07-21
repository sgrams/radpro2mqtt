// SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Home Assistant MQTT discovery payloads.
//!
//! One `sensor` config per measurement field, all sharing a single JSON state
//! topic and the bridge's availability topic.
//! See <https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery>.

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::radpro::DeviceInfo;

struct Entity {
    /// Key in the state JSON; also the object id suffix.
    key: &'static str,
    name: &'static str,
    unit: &'static str,
    device_class: Option<&'static str>,
    state_class: &'static str,
    icon: Option<&'static str>,
    precision: u8,
    diagnostic: bool,
}

const ENTITIES: &[Entity] = &[
    Entity {
        key: "rate_cpm",
        name: "Count rate",
        unit: "cpm",
        device_class: None,
        state_class: "measurement",
        icon: Some("mdi:radioactive"),
        precision: 2,
        diagnostic: false,
    },
    Entity {
        key: "dose_rate_usvh",
        name: "Dose rate",
        unit: "µSv/h",
        device_class: None,
        state_class: "measurement",
        icon: Some("mdi:radioactive"),
        precision: 3,
        diagnostic: false,
    },
    Entity {
        key: "pulse_count",
        name: "Pulse count",
        unit: "counts",
        device_class: None,
        state_class: "total_increasing",
        icon: Some("mdi:counter"),
        precision: 0,
        diagnostic: false,
    },
    Entity {
        key: "tube_time_s",
        name: "Tube lifetime",
        unit: "s",
        device_class: Some("duration"),
        state_class: "total_increasing",
        icon: None,
        precision: 0,
        diagnostic: true,
    },
    Entity {
        key: "battery_voltage",
        name: "Battery voltage",
        unit: "V",
        device_class: Some("voltage"),
        state_class: "measurement",
        icon: None,
        precision: 3,
        diagnostic: true,
    },
];

/// Build the discovery messages as `(topic, payload)` pairs.
///
/// `sample` is a real measurement: fields the firmware did not report are
/// skipped, so Home Assistant never renders an entity that stays unknown.
pub fn configs(cli: &Cli, info: Option<&DeviceInfo>, sample: &Value) -> Vec<(String, String)> {
    let device = json!({
        "identifiers": [info.map_or_else(|| cli.node_id.clone(), |i| i.device_id.clone())],
        "name": cli.device_name,
        "manufacturer": "Rad Pro",
        "model": info.map(|i| i.hardware_id.as_str()).unwrap_or("Geiger counter"),
        "sw_version": info.map(|i| i.software_id.as_str()).unwrap_or_default(),
    });

    ENTITIES
        .iter()
        .filter(|e| sample.get(e.key).is_some_and(|v| !v.is_null()))
        .map(|e| {
            let mut config = json!({
                "name": e.name,
                "unique_id": format!("{}_{}", cli.node_id, e.key),
                "object_id": format!("{}_{}", cli.node_id, e.key),
                "state_topic": cli.state_topic(),
                "availability_topic": cli.availability_topic(),
                "value_template": format!("{{{{ value_json.{} }}}}", e.key),
                "unit_of_measurement": e.unit,
                "state_class": e.state_class,
                "suggested_display_precision": e.precision,
                "device": device,
            });

            let map = config.as_object_mut().expect("config is an object");
            if let Some(dc) = e.device_class {
                map.insert("device_class".into(), json!(dc));
            }
            if let Some(icon) = e.icon {
                map.insert("icon".into(), json!(icon));
            }
            if e.diagnostic {
                map.insert("entity_category".into(), json!("diagnostic"));
            }

            let topic = format!(
                "{}/sensor/{}/{}/config",
                cli.discovery_prefix, cli.node_id, e.key
            );
            (topic, config.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn cli() -> Cli {
        Cli::parse_from(["radpro2mqtt", "-b", "mqtt://localhost"])
    }

    #[test]
    fn skips_entities_the_firmware_does_not_report() {
        let sample = json!({ "rate_cpm": 12.0, "pulse_count": 5, "battery_voltage": null });
        let topics: Vec<_> = configs(&cli(), None, &sample)
            .into_iter()
            .map(|(t, _)| t)
            .collect();

        assert_eq!(
            topics,
            [
                "homeassistant/sensor/radpro/rate_cpm/config",
                "homeassistant/sensor/radpro/pulse_count/config",
            ]
        );
    }

    #[test]
    fn state_and_availability_topics_match_the_bridge() {
        let cli = cli();
        let sample = json!({ "rate_cpm": 12.0 });
        let (_, payload) = configs(&cli, None, &sample).remove(0);
        let config: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(config["state_topic"], cli.state_topic());
        assert_eq!(config["availability_topic"], cli.availability_topic());
        assert_eq!(config["value_template"], "{{ value_json.rate_cpm }}");
    }

    #[test]
    fn device_identity_comes_from_the_hardware_when_known() {
        let info = DeviceInfo {
            hardware_id: "FS2011".into(),
            software_id: "Rad Pro 2.0".into(),
            device_id: "abcd".into(),
        };
        let sample = json!({ "rate_cpm": 12.0 });
        let (_, payload) = configs(&cli(), Some(&info), &sample).remove(0);
        let config: Value = serde_json::from_str(&payload).unwrap();

        assert_eq!(config["device"]["identifiers"], json!(["abcd"]));
        assert_eq!(config["device"]["model"], "FS2011");
        assert_eq!(config["device"]["sw_version"], "Rad Pro 2.0");
    }
}
