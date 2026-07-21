<!--
SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>

SPDX-License-Identifier: AGPL-3.0-or-later
-->

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```sh
cargo build                     # or --release
cargo test
cargo test skips_entities       # single test by name substring
cargo clippy --all-targets
cargo fmt
```

There is no hardware in CI, so the device path is exercised manually — see
"Testing without hardware" below.

Commit conventions are in `CONTRIBUTING.md`: Conventional Commits, and
`Assisted-by: <tool>:<model-version>` for AI-assisted work. Do not add
`Co-authored-by:` for yourself — that trailer is reserved for humans.

## Architecture

A single-binary bridge: serial in, MQTT out. Four modules, each with one job.

- `cli.rs` — clap `Cli`, plus `state_topic()` / `availability_topic()`. These two
  helpers are the single source of topic layout; discovery embeds their output, so
  changing a topic here propagates everywhere.
- `radpro.rs` — the Rad Pro USB protocol (115200 8N1, `GET <property>\r\n` →
  `OK[ value]\r\n` | `ERROR\r\n`, protocol doc:
  <https://github.com/Gissio/radpro/blob/main/docs/comm.md>). Strictly
  request/response over one connection: an unexpected line means the stream has
  desynchronised and is treated as fatal so the caller reconnects, while `ERROR`
  becomes the `Unsupported` error type because it only means this firmware lacks
  that property. `tubeSensitivity` is read once at connect and used to derive
  `dose_rate_usvh` from `tubeRate`.
- `discovery.rs` — the `ENTITIES` table maps `Measurement` JSON fields to Home
  Assistant sensor configs. Adding a reading means adding a field to `Measurement`
  *and* a row here; the key must match the serde field name, since the value
  template is built from it.
- `main.rs` — wiring. The runtime is deliberately `current_thread`: the whole
  program is two tasks that spend their lives blocked on a serial read or a
  socket, and a worker per core cost ~64 MB of glibc malloc arena reservation
  each (1.9 GB of virtual address space on a 28-core box, against 5 MB RSS).
  Nothing here is CPU bound, so nothing may block the thread. The rumqttc event
  loop runs in its own task (it drives
  reconnects and flushes queued publishes, so it must poll even while the serial
  side is down); `bridge()` owns serial reconnection with exponential backoff;
  `poll_device()` returns `Result<Infallible>` because it only ever exits by error.

Two invariants worth preserving:

- Discovery is published *after* the first successful reading, not at startup, so
  entities are only created for properties the firmware actually reports. Fields
  that came back absent are `None`, skipped in the state JSON, and filtered out of
  discovery.
- `Publisher` is the only thing that publishes, and it suppresses anything the
  broker already holds: discovery once per process, availability only on a
  change. Everything it sends is retained, so a device that flaps would
  otherwise produce a stream of identical retained messages. State is published
  every tick and retained only with `--retain`. The MQTT will covers process
  death.
- Backoff resets only after a link that survived `HEALTHY_LINK`, not after one
  that merely produced a reading — otherwise a device that connects and
  immediately drops reconnects once a second forever.
- `avg_rate_cpm` needs two pulse counts, so the first reading of a connection
  only primes it and `poll_device` publishes nothing that tick. That keeps every
  published state complete: an entity is never announced without a value, and
  the field never disappears from a payload it appeared in. A counter that goes
  backwards (reset on the device) restarts the window the same way.

## Testing without hardware

`socat` provides a PTY pair; a script on one end answers the protocol while the
binary opens the other:

```sh
socat pty,raw,echo=0,link=$PWD/ttyDEV pty,raw,echo=0,link=$PWD/ttyHOST &
```

Answer `GET tubeRate` etc. on `ttyHOST`, then run against `--port ./ttyDEV`.
Have the fake device reply `ERROR` to some property to check the optional-field
path, and kill `socat` mid-run to check reconnect backoff.
