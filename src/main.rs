// SPDX-FileCopyrightText: 2026 Stan Grams <sjg@haxx.space>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod cli;
mod discovery;
mod radpro;

use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use clap::Parser;
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS, Transport};
use serde_json::Value;
use tokio::time::{MissedTickBehavior, interval, sleep};
use tracing::{debug, error, info, warn};
use url::Url;

use crate::cli::Cli;
use crate::radpro::{DeviceInfo, RadPro};

/// Backoff bounds for reopening the serial port.
const RECONNECT_MIN: Duration = Duration::from_secs(1);
const RECONNECT_MAX: Duration = Duration::from_secs(60);
/// How long a serial link must survive to count as healthy and earn a backoff reset.
const HEALTHY_LINK: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "radpro2mqtt=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let (client, mut eventloop) = AsyncClient::new(mqtt_options(&cli)?, 32);

    // The event loop drives reconnection and flushes queued publishes, so it
    // must keep running even while the serial side is down.
    let broker = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => info!("connected to broker"),
                Ok(event) => debug!(?event),
                Err(e) => {
                    warn!(error = %e, "broker connection lost, retrying");
                    sleep(RECONNECT_MIN).await;
                }
            }
        }
    });

    let result = tokio::select! {
        r = bridge(&cli, &client) => r,
        _ = tokio::signal::ctrl_c() => {
            info!("interrupted, shutting down");
            Ok(())
        }
    };

    // Best effort: the will message covers us if these do not get out.
    let _ = client
        .publish(cli.availability_topic(), QoS::AtLeastOnce, true, "offline")
        .await;
    let _ = client.disconnect().await;
    broker.abort();

    result
}

fn mqtt_options(cli: &Cli) -> Result<MqttOptions> {
    let url = Url::parse(&cli.mqtt_url).with_context(|| format!("parsing {}", cli.mqtt_url))?;

    let tls = match url.scheme() {
        "mqtt" | "tcp" => false,
        "mqtts" | "ssl" => true,
        other => bail!("unsupported URL scheme `{other}`, expected mqtt:// or mqtts://"),
    };
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("no host in {}", cli.mqtt_url))?;
    let port = url.port().unwrap_or(if tls { 8883 } else { 1883 });

    let mut opts = MqttOptions::new(&cli.client_id, host, port);
    opts.set_keep_alive(Duration::from_secs((cli.interval * 2).clamp(15, 300)));
    opts.set_last_will(LastWill::new(
        cli.availability_topic(),
        "offline",
        QoS::AtLeastOnce,
        true,
    ));
    if tls {
        opts.set_transport(Transport::tls_with_default_config());
    }

    // Explicit flags win over credentials embedded in the URL.
    let username = cli.mqtt_username.clone().or_else(|| {
        Some(url.username())
            .filter(|u| !u.is_empty())
            .map(Into::into)
    });
    let password = cli
        .mqtt_password
        .clone()
        .or_else(|| url.password().map(Into::into));
    if let Some(username) = username {
        opts.set_credentials(username, password.unwrap_or_default());
    }

    Ok(opts)
}

/// Publishes on behalf of the bridge, suppressing messages that would restate
/// what the broker already holds. Everything it sends is retained, so a
/// flapping device must not turn into a stream of identical publishes.
struct Publisher<'a> {
    cli: &'a Cli,
    client: &'a AsyncClient,
    /// Last availability published, `None` before the first one.
    online: Option<bool>,
    /// Discovery is retained by the broker, so once per process is enough.
    announced: bool,
}

impl<'a> Publisher<'a> {
    fn new(cli: &'a Cli, client: &'a AsyncClient) -> Self {
        Self {
            cli,
            client,
            online: None,
            announced: false,
        }
    }

    async fn set_availability(&mut self, online: bool) -> Result<()> {
        if self.online == Some(online) {
            return Ok(());
        }
        let payload = if online { "online" } else { "offline" };
        self.client
            .publish(
                self.cli.availability_topic(),
                QoS::AtLeastOnce,
                true,
                payload,
            )
            .await?;
        self.online = Some(online);
        debug!(payload, "published availability");
        Ok(())
    }

    async fn announce(&mut self, info: Option<&DeviceInfo>, state: &Value) -> Result<()> {
        if self.announced || self.cli.no_discovery {
            return Ok(());
        }
        for (topic, payload) in discovery::configs(self.cli, info, state) {
            self.client
                .publish(&topic, QoS::AtLeastOnce, true, payload)
                .await?;
            debug!(%topic, "published discovery config");
        }
        self.announced = true;
        Ok(())
    }

    async fn publish_state(&self, state: &Value) -> Result<()> {
        self.client
            .publish(
                self.cli.state_topic(),
                QoS::AtLeastOnce,
                self.cli.retain,
                serde_json::to_vec(state)?,
            )
            .await?;
        Ok(())
    }
}

/// Own the serial link: connect, publish, and reconnect with backoff forever.
async fn bridge(cli: &Cli, client: &AsyncClient) -> Result<()> {
    let mut publisher = Publisher::new(cli, client);
    let mut backoff = RECONNECT_MIN;

    loop {
        // `poll_device` only ever returns an error, so this binding is exhaustive.
        let started = Instant::now();
        let Err(e) = poll_device(cli, &mut publisher).await;

        error!(error = ?e, port = %cli.port, "device link failed");
        publisher.set_availability(false).await?;

        // Only a link that stayed up earns fast retries. A device that connects
        // and immediately drops would otherwise reconnect once a second forever.
        backoff = if started.elapsed() >= HEALTHY_LINK {
            RECONNECT_MIN
        } else {
            (backoff * 2).min(RECONNECT_MAX)
        };
        sleep(backoff).await;
    }
}

async fn poll_device(cli: &Cli, publisher: &mut Publisher<'_>) -> Result<std::convert::Infallible> {
    let timeout = Duration::from_secs(cli.timeout);
    let mut device = RadPro::connect(&cli.port, cli.baud, timeout).await?;
    info!(port = %cli.port, "serial port open");

    let info = match device.device_info().await {
        Ok(info) => {
            info!(
                hardware = %info.hardware_id,
                software = %info.software_id,
                id = %info.device_id,
                "device identified"
            );
            Some(info)
        }
        Err(e) => {
            warn!(error = %e, "could not read device id");
            None
        }
    };

    let mut ticker = interval(Duration::from_secs(cli.interval));
    // A slow device must not cause a burst of catch-up polls.
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        ticker.tick().await;

        let measurement = device.measure().await?;
        let state = serde_json::to_value(&measurement)?;
        debug!(?measurement);

        // Discovery is deferred until the first reading so that entities are
        // only created for properties this firmware actually reports.
        publisher.announce(info.as_ref(), &state).await?;
        publisher.set_availability(true).await?;
        publisher.publish_state(&state).await?;
    }
}
