<!--
SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>

SPDX-License-Identifier: AGPL-3.0-or-later
-->

# radpro2mqtt

[![CI](https://github.com/sgrams/radpro2mqtt/actions/workflows/ci.yml/badge.svg)](https://github.com/sgrams/radpro2mqtt/actions/workflows/ci.yml)

Bridges a Geiger counter running [Rad Pro](https://github.com/Gissio/radpro) firmware to
MQTT over its USB serial port, with Home Assistant discovery so the sensors appear
automatically.

## Building

Needs a stable Rust toolchain; there are no system dependencies beyond a C
toolchain for the serial crate.

```sh
cargo build --release
```

The binary lands in `target/release/radpro2mqtt`. `cargo run -- <args>` builds and
runs in one step during development.

## Usage

```sh
radpro2mqtt --port /dev/ttyACM0 --mqtt-url mqtt://broker.lan:1883 \
            --mqtt-username geiger --mqtt-password secret
```

Reading the serial port needs membership of the group owning `/dev/ttyACM*` —
`uucp` on Arch, `dialout` on Debian and Ubuntu. For a permanent install prefer a
stable `/dev/serial/by-id/...` path, since `ttyACM0` can renumber between boots.

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
{"rate_cpm":18.45,"avg_rate_cpm":19.2,"dose_rate_usvh":0.12,"pulse_count":1007,"tube_time_s":86400,"battery_voltage":4.05}
```

There are two count rates, and they answer different questions. `rate_cpm` is the
firmware's own figure, which responds quickly to a source being brought near the
tube. `avg_rate_cpm` is the mean over the poll interval, computed here from the
growth of the pulse counter; radioactive decay is a Poisson process, so at
background levels a short window sees only a handful of counts and the
instantaneous figure swings widely. The longer your `--interval`, the steadier
`avg_rate_cpm` gets. Use it for graphs and history, and `rate_cpm` when you want
the device to react.

Because that mean needs two counter readings, nothing is published until the
second poll of a connection — one interval after start-up.

`dose_rate_usvh` is derived from the tube sensitivity reported by the device.
Fields the firmware does not support are omitted, and no discovery config is
published for them. Run one instance per counter, with a distinct `--node-id` and
`--client-id` for each.

## Running as a service

`packaging/` holds a systemd unit and an environment file template:

```sh
sudo install -Dm755 target/release/radpro2mqtt /usr/local/bin/radpro2mqtt
sudo install -Dm600 packaging/radpro2mqtt.env.example /etc/radpro2mqtt.env
sudo install -Dm644 packaging/radpro2mqtt.service /etc/systemd/system/radpro2mqtt.service
sudoedit /etc/radpro2mqtt.env          # port, broker URL, credentials
sudo systemctl enable --now radpro2mqtt
journalctl -u radpro2mqtt -f
```

The service runs unprivileged under a `DynamicUser`, with access to nothing but
the network and `/dev/ttyACM*`. It joins the group owning that device — `uucp`,
as shipped; change `SupplementaryGroups=` to `dialout` on Debian or Ubuntu.
Credentials live in the environment file rather than the command line, so they
stay out of `ps` output.

## Message rate

One state message per `--interval` (default 10s, so 6/min). Discovery is sent once
per run, and availability only when it changes, so an unstable device cannot flood
the broker with retained messages.

## Behaviour

The bridge keeps running when either link drops: MQTT reconnects via the client's
event loop, and the serial port is reopened with exponential backoff (1s to 60s)
while availability is published as `offline`. Backoff only resets once a link has
stayed up for a minute, so a device that connects and immediately drops backs off
instead of retrying in a tight loop.

The discovery configs also carry `expire_after`, set to three poll intervals (at
least 30s). The will message covers a bridge that dies, but not one wedged with a
live connection and a silent device; with `expire_after`, Home Assistant marks the
sensors unavailable on its own once readings stop arriving, instead of showing a
stale value indefinitely.

## License

AGPL-3.0-or-later.
