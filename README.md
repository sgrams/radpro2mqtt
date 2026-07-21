<!--
SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>

SPDX-License-Identifier: AGPL-3.0-or-later
-->

# radpro2mqtt

[![CI](https://github.com/sgrams/radpro2mqtt/actions/workflows/ci.yml/badge.svg)](https://github.com/sgrams/radpro2mqtt/actions/workflows/ci.yml)

Reads a Geiger counter running [Rad Pro](https://github.com/Gissio/radpro) firmware
over USB serial and publishes its measurements to MQTT. Home Assistant discovers the
sensors on its own; there is nothing to add to `configuration.yaml`.

Rad Pro runs on the FS2011, Bosean FS-600/FS-1000/FS-5000, FNIRSI GC-01/GC-03 and
GQ GMC-800, among others. This bridge talks to whatever that firmware exposes, so
readings the device does not support are simply left out.

## Entities

| Entity | Unit | Notes |
| --- | --- | --- |
| Count rate | cpm | the firmware's own figure, quick to react |
| Average count rate | cpm | mean over the poll interval, steadier |
| Dose rate | µSv/h | from the tube sensitivity the device reports |
| Pulse count | counts | lifetime total, `total_increasing` |
| Tube lifetime | s | diagnostic |
| Battery voltage | V | diagnostic, if the firmware reports it |

### Why two count rates

Radioactive decay is a Poisson process. At background levels a short window holds
only a handful of counts, so an instantaneous rate swings widely — most of that
movement is counting statistics, not radiation.

`rate_cpm` is what the firmware reports and responds quickly when a source is
brought near the tube. `avg_rate_cpm` is computed here from the growth of the pulse
counter over the poll interval: it is an exact mean rather than an estimate, and it
gets steadier the longer the interval. Graph the average, alert on the
instantaneous one.

## Build

Needs a stable Rust toolchain and a C toolchain for the serial crate; nothing else.

```sh
cargo build --release
```

The binary lands in `target/release/radpro2mqtt`. During development
`cargo run -- <args>` builds and runs in one step.

## Quick start

```sh
radpro2mqtt --port /dev/ttyACM0 --mqtt-url mqtt://broker.lan:1883 \
            --mqtt-username geiger --mqtt-password secret
```

Reading the serial port requires membership of the group that owns `/dev/ttyACM*`:
`uucp` on Arch, `dialout` on Debian and Ubuntu. Prefer a stable
`/dev/serial/by-id/...` path for anything permanent, since `ttyACM0` can renumber
between reboots — `ls -l /dev/serial/by-id/` shows yours.

`RUST_LOG=radpro2mqtt=debug` logs every poll and publish, which is the quickest way
to see what your firmware does and does not answer.

## Configuration

| Option | Environment | Default | Notes |
| --- | --- | --- | --- |
| `-p`, `--port` | `RADPRO_PORT` | `/dev/ttyACM0` | serial device |
| `--baud` | `RADPRO_BAUD` | `115200` | Rad Pro uses 115200 8N1 |
| `-b`, `--mqtt-url` | `MQTT_URL` | — | `mqtt://host:1883` or `mqtts://host:8883` |
| `-u`, `--mqtt-username` | `MQTT_USERNAME` | — | |
| `-P`, `--mqtt-password` | `MQTT_PASSWORD` | — | |
| `--client-id` | `MQTT_CLIENT_ID` | `radpro2mqtt` | must be unique on the broker |
| `-i`, `--interval` | | `10` | seconds between polls |
| `--timeout` | | `5` | seconds to wait for one serial reply |
| `--topic-prefix` | | `radpro` | |
| `--node-id` | | `radpro` | must be unique per counter |
| `--device-name` | | `Rad Pro` | shown in Home Assistant |
| `--discovery-prefix` | | `homeassistant` | |
| `--retain` | | off | retain state messages |
| `--no-discovery` | | off | skip the discovery configs |

`mqtts://` uses the system root certificates. Credentials may also be embedded in
the URL (`mqtt://user:pass@host`); the flags win if both are given. Passing them by
environment keeps them out of `ps` output and shell history.

To run more than one counter, give each instance its own `--node-id` and
`--client-id`.

## Topics

With the defaults:

| Topic | Retained | Contents |
| --- | --- | --- |
| `radpro/radpro/state` | with `--retain` | JSON of all readings |
| `radpro/radpro/availability` | yes | `online` / `offline`, also the MQTT will |
| `homeassistant/sensor/radpro/<field>/config` | yes | one discovery config per sensor |

```json
{"rate_cpm":18.45,"avg_rate_cpm":19.2,"dose_rate_usvh":0.12,"pulse_count":1007,"tube_time_s":86400,"battery_voltage":4.05}
```

Nothing is published until the second poll of a connection, because the average
needs two counter readings. Discovery configs are published only for the fields
that first reading actually contained, so no entity is ever created that would sit
forever unknown.

## Reliability

One state message per interval — six a minute by default. Discovery is sent once
per run and availability only when it changes, so an unstable device cannot flood
the broker with retained messages.

Both links recover on their own. MQTT reconnects through the client's event loop.
The serial port is reopened with exponential backoff from 1s to 60s, publishing
`offline` while it is down; the backoff only resets after a link has stayed up for
a minute, so a device that connects and immediately drops is not retried in a tight
loop.

Three separate failures are covered: the retained will message handles the bridge
dying, `offline` handles the counter being unplugged, and `expire_after` — three
intervals, at least 30s — handles a bridge left holding a live broker connection to
a silent device, where Home Assistant would otherwise show a stale reading forever.

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

The service runs unprivileged under a `DynamicUser` with access to nothing but a
socket and `/dev/ttyACM*`, and joins the group owning that device — `uucp` as
shipped, so change `SupplementaryGroups=` on Debian or Ubuntu.

## Development

`cargo test` covers the pure logic. The device and broker halves are exercised by
hand against a PTY pair and a stub broker; `CLAUDE.md` describes the setup, which
also reproduces reconnect and backoff behaviour without touching hardware.

See `CONTRIBUTING.md` for commit conventions and the REUSE licensing headers.

## License

AGPL-3.0-or-later. The protocol implementation follows the Rad Pro
[communication documentation](https://github.com/Gissio/radpro/blob/main/docs/comm.md).
