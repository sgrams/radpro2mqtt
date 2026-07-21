// SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Client for the Rad Pro USB/serial protocol.
//!
//! The device speaks line-oriented ASCII at 115200 8N1. Requests are
//! `GET <property>\r\n`; responses are `OK[ <value>]\r\n` or `ERROR\r\n`.
//! See <https://github.com/Gissio/radpro/blob/main/docs/comm.md>.

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_serial::{SerialPortBuilderExt, SerialStream};

/// A command the device answered with `ERROR`, i.e. it does not support the
/// property. Distinguished from I/O failures so callers can degrade instead of
/// tearing down the connection.
#[derive(Debug)]
pub struct Unsupported(pub String);

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "device rejected `{}`", self.0)
    }
}

impl std::error::Error for Unsupported {}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    pub hardware_id: String,
    pub software_id: String,
    pub device_id: String,
}

/// Minimum spacing between two pulse counts before their difference is worth
/// turning into a rate; below this the elapsed time is mostly jitter.
const MIN_RATE_WINDOW: Duration = Duration::from_millis(500);

/// Counts per minute from the growth of the pulse counter over `elapsed`.
///
/// `None` when the window is too short to divide by, which also covers the
/// first reading of a connection.
fn derived_cpm(delta: u64, elapsed: Duration) -> Option<f64> {
    if elapsed < MIN_RATE_WINDOW {
        return None;
    }
    Some(delta as f64 * 60.0 / elapsed.as_secs_f64())
}

/// One poll of the device. Optional fields are omitted when the firmware does
/// not implement the property.
#[derive(Debug, Clone, Serialize)]
pub struct Measurement {
    /// Instantaneous count rate as the firmware reports it, counts per minute.
    pub rate_cpm: f64,
    /// Mean count rate over the poll interval, computed from the pulse counter.
    ///
    /// Absent on the first reading of a connection, and whenever the counter
    /// has gone backwards, which means it was reset under us.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_rate_cpm: Option<f64>,
    /// Equivalent dose rate in µSv/h, derived from the tube sensitivity.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dose_rate_usvh: Option<f64>,
    /// Lifetime pulse count.
    pub pulse_count: u64,
    /// Tube lifetime, seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tube_time_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub battery_voltage: Option<f64>,
}

/// Downgrade an unavailable property to `None`. I/O trouble still surfaces via
/// the mandatory properties, which are read first.
fn optional<T>(res: Result<T>) -> Option<T> {
    match res {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::debug!(error = %e, "optional property unavailable");
            None
        }
    }
}

pub struct RadPro {
    io: BufReader<SerialStream>,
    timeout: Duration,
    /// cpm per µSv/h, read once at connect.
    sensitivity: Option<f64>,
    /// Pulse count and the moment it was read, for the derived rate.
    previous: Option<(u64, Instant)>,
}

impl RadPro {
    pub async fn connect(port: &str, baud: u32, timeout: Duration) -> Result<Self> {
        let stream = tokio_serial::new(port, baud)
            .data_bits(tokio_serial::DataBits::Eight)
            .parity(tokio_serial::Parity::None)
            .stop_bits(tokio_serial::StopBits::One)
            .flow_control(tokio_serial::FlowControl::None)
            .open_native_async()
            .with_context(|| format!("opening serial port {port}"))?;

        let mut dev = Self {
            io: BufReader::new(stream),
            timeout,
            sensitivity: None,
            previous: None,
        };

        // A sensitivity of zero would make the dose rate meaningless.
        dev.sensitivity = match dev.get_f64("tubeSensitivity").await {
            Ok(v) if v > 0.0 => Some(v),
            Ok(v) => {
                tracing::warn!(
                    sensitivity = v,
                    "implausible tube sensitivity, dose rate disabled"
                );
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "tube sensitivity unavailable, dose rate disabled");
                None
            }
        };

        Ok(dev)
    }

    pub async fn device_info(&mut self) -> Result<DeviceInfo> {
        let raw = self.request("GET deviceId").await?;
        let mut parts = raw.split(';');
        let mut next = |field: &str| -> Result<String> {
            parts
                .next()
                .map(|s| s.trim().to_string())
                .ok_or_else(|| anyhow!("deviceId response missing {field}: {raw:?}"))
        };
        Ok(DeviceInfo {
            hardware_id: next("hardware id")?,
            software_id: next("software id")?,
            device_id: next("device id")?,
        })
    }

    pub async fn measure(&mut self) -> Result<Measurement> {
        let rate_cpm = self.get_f64("tubeRate").await?;
        let pulse_count = self.get_u64("tubePulseCount").await?;
        let read_at = Instant::now();
        let tube_time = self.get_u64("tubeTime").await;
        let battery = self.get_f64("deviceBatteryVoltage").await;

        // A counter that went backwards was reset on the device; start a fresh
        // window rather than reporting a negative or absurd rate.
        let avg_rate_cpm = match self.previous {
            Some((previous, at)) if pulse_count >= previous => {
                derived_cpm(pulse_count - previous, read_at.duration_since(at))
            }
            Some(_) => {
                tracing::warn!("pulse counter went backwards, restarting the rate window");
                None
            }
            None => None,
        };
        self.previous = Some((pulse_count, read_at));

        Ok(Measurement {
            rate_cpm,
            avg_rate_cpm,
            dose_rate_usvh: self.sensitivity.map(|s| rate_cpm / s),
            pulse_count,
            tube_time_s: optional(tube_time),
            battery_voltage: optional(battery),
        })
    }

    async fn get_f64(&mut self, property: &str) -> Result<f64> {
        let raw = self.request(&format!("GET {property}")).await?;
        raw.parse()
            .with_context(|| format!("{property}: expected a number, got {raw:?}"))
    }

    async fn get_u64(&mut self, property: &str) -> Result<u64> {
        let raw = self.request(&format!("GET {property}")).await?;
        raw.parse()
            .with_context(|| format!("{property}: expected an integer, got {raw:?}"))
    }

    /// Send one command and return the payload following `OK`.
    async fn request(&mut self, command: &str) -> Result<String> {
        tokio::time::timeout(self.timeout, self.exchange(command))
            .await
            .map_err(|_| anyhow!("timed out waiting for response to `{command}`"))?
    }

    async fn exchange(&mut self, command: &str) -> Result<String> {
        self.io.write_all(command.as_bytes()).await?;
        self.io.write_all(b"\r\n").await?;
        self.io.flush().await?;

        let mut line = String::new();
        if self.io.read_line(&mut line).await? == 0 {
            bail!("serial port closed while awaiting response to `{command}`");
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line == "ERROR" {
            return Err(Unsupported(command.to_string()).into());
        }
        match line.strip_prefix("OK") {
            Some(rest) => Ok(rest.trim().to_string()),
            // Unsolicited output would desynchronise the request/response
            // pairing, so treat it as fatal and let the caller reconnect.
            None => bail!("unexpected response to `{command}`: {line:?}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_over_a_window_become_a_rate_per_minute() {
        // 30 counts in 30 s is 60 cpm.
        assert_eq!(derived_cpm(30, Duration::from_secs(30)), Some(60.0));
        // The same counts over a minute is half that.
        assert_eq!(derived_cpm(30, Duration::from_secs(60)), Some(30.0));
        // A quiet tube is a real answer, not a missing one.
        assert_eq!(derived_cpm(0, Duration::from_secs(10)), Some(0.0));
    }

    #[test]
    fn windows_too_short_to_divide_by_are_rejected() {
        assert_eq!(derived_cpm(5, Duration::from_millis(499)), None);
        assert_eq!(derived_cpm(5, Duration::ZERO), None);
        assert!(derived_cpm(5, MIN_RATE_WINDOW).is_some());
    }

    #[test]
    fn a_large_counter_does_not_lose_precision() {
        // The counter is 32 bits wide and only ever grows.
        let delta = u32::MAX as u64 - (u32::MAX as u64 - 120);
        assert_eq!(derived_cpm(delta, Duration::from_secs(60)), Some(120.0));
    }
}
