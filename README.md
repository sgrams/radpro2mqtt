<!--
SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>

SPDX-License-Identifier: AGPL-3.0-or-later
-->

# radpro2mqtt

Bridges a Geiger counter running [Rad Pro](https://github.com/Gissio/radpro) firmware to
MQTT over its USB serial port, with Home Assistant discovery so the sensors appear
automatically.

## Usage

```sh
radpro2mqtt --port /dev/ttyACM0 --mqtt-url mqtt://broker.lan:1883 \
            --mqtt-username geiger --mqtt-password secret
```

Every flag has an equivalent environment variable (`RADPRO_PORT`, `MQTT_URL`,
`MQTT_USERNAME`, `MQTT_PASSWORD`, `MQTT_CLIENT_ID`); see `radpro2mqtt --help` for the
full list. Credentials may also be embedded in the URL (`mqtt://user:pass@host`), in
which case the flags take precedence. `mqtts://` connects over TLS using the system
root certificates.

Set `RUST_LOG=radpro2mqtt=debug` for per-poll detail.

## Topics

With the defaults (`--topic-prefix radpro`, `--node-id radpro`):

| Topic | Retained | Contents |
| --- | --- | --- |
| `radpro/radpro/state` | with `--retain` | JSON of all readings |
| `radpro/radpro/availability` | yes | `online` / `offline`, also the MQTT will |
| `homeassistant/sensor/radpro/<field>/config` | yes | discovery config per sensor |

State payload:

```json
{"rate_cpm":18.45,"dose_rate_usvh":0.12,"pulse_count":1007,"tube_time_s":86400,"battery_voltage":4.05}
```

`dose_rate_usvh` is derived from the tube sensitivity reported by the device.
Fields the firmware does not support are omitted, and no discovery config is
published for them. Run one instance per counter, with a distinct `--node-id` and
`--client-id` for each.

## Behaviour

The bridge keeps running when either link drops: MQTT reconnects via the client's
event loop, and the serial port is reopened with exponential backoff (1s to 60s)
while availability is published as `offline`.

## License

AGPL-3.0-or-later.
