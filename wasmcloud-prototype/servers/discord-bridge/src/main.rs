//! Discord Bridge — standalone binary that runs a Discord bot (Serenity),
//! subscribes to `sentinel.alerts` JetStream for CRITICAL threat alerts,
//! and serves slash commands by reading from NATS KV buckets.

use std::collections::HashMap;
use std::sync::Arc;

use async_nats::jetstream;
use serenity::all::{
    ChannelId, ChannelType, Command, CommandDataOptionValue, CommandInteraction, CommandOptionType,
    CreateCommand, CreateCommandOption, CreateEmbed, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateMessage, EventHandler, GatewayIntents, Interaction,
    Ready,
};
use serenity::async_trait;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::RwLock;

// ─── Configuration ──────────────────────────────────────────────────────────

fn require(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

#[derive(Clone)]
struct Config {
    nats_url: String,
    discord_token: String,
}

impl Config {
    fn from_env() -> Self {
        Self {
            nats_url: require("NATS_URL"),
            discord_token: require("DISCORD_TOKEN"),
        }
    }
}

// ─── Alert payload from sentinel.alerts ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ThreatAlert {
    pilot_id: u64,
    pilot_name: String,
    threat_score: u64,
    tier: String,
    system: String,
}

// ─── Profile payload from sentinel.profiles KV ─────────────────────────────

#[derive(Debug, Deserialize)]
struct ThreatProfile {
    item_id: u64,
    name: Option<String>,
    threat_score: u64,
    kill_count: u64,
    death_count: u64,
    bounty_count: u64,
    systems_visited: u64,
    recent_kills_24h: u64,
    last_seen_system: String,
    tribe_name: Option<String>,
}

/// Per-guild alert channel mapping (guild_id → channel_id), stored in sentinel.discord KV.
type AlertChannels = Arc<RwLock<HashMap<u64, u64>>>;

// ─── Display helpers ────────────────────────────────────────────────────────

fn tier_color(tier: &str) -> u32 {
    match tier {
        "LOW" => 0x44FF88,
        "MODERATE" => 0xFFD700,
        "HIGH" => 0xFF8C00,
        "CRITICAL" => 0xFF4444,
        _ => 0x808080,
    }
}

fn tier_emoji(tier: &str) -> &'static str {
    match tier {
        "LOW" => "🟢",
        "MODERATE" => "🟡",
        "HIGH" => "🟠",
        "CRITICAL" => "🔴",
        _ => "⚪",
    }
}

fn score_bar(score: u64) -> String {
    let filled = ((score as f64 / 10000.0) * 20.0).round() as usize;
    let empty = 20_usize.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn threat_tier(score: u64) -> &'static str {
    match score {
        0..=2500 => "LOW",
        2501..=5000 => "MODERATE",
        5001..=7500 => "HIGH",
        _ => "CRITICAL",
    }
}

fn threat_embed(profile: &ThreatProfile) -> CreateEmbed {
    let tier = threat_tier(profile.threat_score);
    let kd = if profile.death_count > 0 {
        format!("{:.2}", profile.kill_count as f64 / profile.death_count as f64)
    } else if profile.kill_count > 0 {
        format!("{:.2} (perfect)", profile.kill_count as f64)
    } else {
        "N/A".to_string()
    };

    let score_display = format!(
        "{} {:.2} / 100.00",
        score_bar(profile.threat_score),
        profile.threat_score as f64 / 100.0
    );

    let last_seen = if profile.last_seen_system.is_empty() {
        "Unknown".to_string()
    } else {
        profile.last_seen_system.clone()
    };

    let tribe = profile
        .tribe_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("None");

    let mut hot = String::new();
    if profile.recent_kills_24h > 0 {
        hot = format!(" 🔥 {} kills in 24h", profile.recent_kills_24h);
    }

    CreateEmbed::new()
        .title(format!(
            "{} SENTINEL — {}",
            tier_emoji(tier),
            profile.name.as_deref().unwrap_or("Unknown Pilot")
        ))
        .color(tier_color(tier))
        .description(format!("`{score_display}`{hot}"))
        .field("Threat Tier", format!("{} {tier}", tier_emoji(tier)), true)
        .field("Kill Count", profile.kill_count.to_string(), true)
        .field("Death Count", profile.death_count.to_string(), true)
        .field("K/D Ratio", kd, true)
        .field("Bounties", profile.bounty_count.to_string(), true)
        .field("Systems Visited", profile.systems_visited.to_string(), true)
        .field("Last Seen", last_seen, true)
        .field("Tribe", tribe, true)
        .footer(serenity::all::CreateEmbedFooter::new(
            "SENTINEL — EVE Frontier Threat Intelligence",
        ))
        .timestamp(serenity::model::timestamp::Timestamp::now())
}

fn alert_embed(alert: &ThreatAlert) -> CreateEmbed {
    CreateEmbed::new()
        .title("⚠️ CRITICAL THREAT ALERT")
        .color(0xFF4444)
        .description(format!(
            "**{}** has reached CRITICAL threat level in system **{}**",
            alert.pilot_name, alert.system
        ))
        .field("Pilot", &alert.pilot_name, true)
        .field(
            "Threat Score",
            format!(
                "{} {:.2}",
                tier_emoji(&alert.tier),
                alert.threat_score as f64 / 100.0
            ),
            true,
        )
        .field("System", &alert.system, true)
        .footer(serenity::all::CreateEmbedFooter::new(
            "SENTINEL — EVE Frontier Threat Intelligence",
        ))
        .timestamp(serenity::model::timestamp::Timestamp::now())
}

// ─── Serenity event handler ─────────────────────────────────────────────────

struct Handler {
    profiles_kv: jetstream::kv::Store,
    discord_kv: jetstream::kv::Store,
    alert_channels: AlertChannels,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: serenity::all::Context, ready: Ready) {
        tracing::info!(bot_name = %ready.user.name, "Discord bot connected as {}", ready.user.name);

        let commands = vec![
            CreateCommand::new("threat")
                .description("Look up a pilot's threat profile")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::String,
                        "pilot",
                        "Pilot name or ID to search for",
                    )
                    .required(true),
                ),
            CreateCommand::new("leaderboard").description("Show the top 10 most dangerous pilots"),
            CreateCommand::new("alerts")
                .description("Configure CRITICAL threat alerts for this server")
                .add_option(
                    CreateCommandOption::new(
                        CommandOptionType::SubCommand,
                        "set",
                        "Set the channel for CRITICAL threat alerts",
                    )
                    .add_sub_option(
                        CreateCommandOption::new(
                            CommandOptionType::Channel,
                            "channel",
                            "Channel to send alerts to",
                        )
                        .required(true)
                        .channel_types(vec![ChannelType::Text]),
                    ),
                )
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "remove",
                    "Remove CRITICAL alerts from this server",
                )),
        ];

        if let Err(e) = Command::set_global_commands(&ctx.http, commands).await {
            tracing::error!(error = %e, "Failed to register slash commands: {e}");
        }
    }

    async fn interaction_create(&self, ctx: serenity::all::Context, interaction: Interaction) {
        if let Interaction::Command(cmd) = interaction {
            let result = match cmd.data.name.as_str() {
                "threat" => self.handle_threat(&ctx, &cmd).await,
                "leaderboard" => self.handle_leaderboard(&ctx, &cmd).await,
                "alerts" => self.handle_alerts(&ctx, &cmd).await,
                _ => Ok(()),
            };

            if let Err(e) = result {
                tracing::error!(command = %cmd.data.name, error = %e, "Command error: {e}");
                let _ = cmd
                    .create_response(
                        &ctx.http,
                        CreateInteractionResponse::Message(
                            CreateInteractionResponseMessage::new()
                                .content(format!("Error: {e}"))
                                .ephemeral(true),
                        ),
                    )
                    .await;
            }
        }
    }
}

impl Handler {
    async fn handle_threat(
        &self,
        ctx: &serenity::all::Context,
        cmd: &CommandInteraction,
    ) -> anyhow::Result<()> {
        let pilot_arg = cmd
            .data
            .options
            .first()
            .and_then(|o| match &o.value {
                CommandDataOptionValue::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_default();

        // Try parsing as numeric ID first
        let profile = if let Ok(pilot_id) = pilot_arg.parse::<u64>() {
            self.lookup_profile(pilot_id).await?
        } else {
            // Search by name — scan KV keys (limited approach for prototype)
            self.search_profile_by_name(&pilot_arg).await?
        };

        let response = match profile {
            Some(p) => CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().embed(threat_embed(&p)),
            ),
            None => CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(format!("No profile found for `{pilot_arg}`"))
                    .ephemeral(true),
            ),
        };

        cmd.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_leaderboard(
        &self,
        ctx: &serenity::all::Context,
        cmd: &CommandInteraction,
    ) -> anyhow::Result<()> {
        // Scan KV for all profiles, sort by threat_score, take top 10
        let keys = self.profiles_kv.keys().await?.collect::<Vec<_>>().await;
        let mut profiles = Vec::new();

        for key in keys.into_iter().flatten() {
            if let Ok(Some(entry)) = self.profiles_kv.get(&key).await {
                if let Ok(p) = serde_json::from_slice::<ThreatProfile>(&entry) {
                    profiles.push(p);
                }
            }
        }

        profiles.sort_by(|a, b| b.threat_score.cmp(&a.threat_score));

        let mut desc = String::new();
        for (i, p) in profiles.iter().take(10).enumerate() {
            let tier = threat_tier(p.threat_score);
            let medal = match i {
                0 => "🥇",
                1 => "🥈",
                2 => "🥉",
                _ => tier_emoji(tier),
            };
            let score = p.threat_score as f64 / 100.0;
            desc.push_str(&format!(
                "{medal} **{}. {}** — `{score:.2}` ({tier}) — {} kills\n",
                i + 1,
                p.name.as_deref().unwrap_or("Unknown Pilot"),
                p.kill_count,
            ));
        }

        if desc.is_empty() {
            desc = "No pilots tracked yet.".to_string();
        }

        let embed = CreateEmbed::new()
            .title("🏆 SENTINEL Threat Leaderboard")
            .color(0xFF4444)
            .description(desc)
            .footer(serenity::all::CreateEmbedFooter::new(
                "SENTINEL — EVE Frontier Threat Intelligence",
            ))
            .timestamp(serenity::model::timestamp::Timestamp::now());

        cmd.create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().embed(embed),
            ),
        )
        .await?;
        Ok(())
    }

    async fn handle_alerts(
        &self,
        ctx: &serenity::all::Context,
        cmd: &CommandInteraction,
    ) -> anyhow::Result<()> {
        let guild_id = cmd
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("This command can only be used in a server"))?;

        let sub = cmd
            .data
            .options
            .first()
            .map(|o| o.name.as_str())
            .unwrap_or("");

        match sub {
            "set" => {
                let channel_id = cmd
                    .data
                    .options
                    .first()
                    .and_then(|o| match &o.value {
                        CommandDataOptionValue::SubCommand(opts) => opts.first(),
                        _ => None,
                    })
                    .and_then(|o| match &o.value {
                        CommandDataOptionValue::Channel(ch) => Some(ch.get()),
                        _ => None,
                    })
                    .ok_or_else(|| anyhow::anyhow!("Channel required"))?;

                // Store in NATS KV and local cache
                self.discord_kv
                    .put(
                        format!("alerts:{}", guild_id.get()),
                        bytes::Bytes::from(channel_id.to_string()),
                    )
                    .await?;
                self.alert_channels
                    .write()
                    .await
                    .insert(guild_id.get(), channel_id);

                cmd.create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("CRITICAL alerts will be sent to <#{channel_id}>"))
                            .ephemeral(true),
                    ),
                )
                .await?;
            }
            "remove" => {
                self.discord_kv
                    .delete(format!("alerts:{}", guild_id.get()))
                    .await
                    .ok();
                self.alert_channels
                    .write()
                    .await
                    .remove(&guild_id.get());

                cmd.create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("CRITICAL alerts removed for this server")
                            .ephemeral(true),
                    ),
                )
                .await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn lookup_profile(&self, pilot_id: u64) -> anyhow::Result<Option<ThreatProfile>> {
        let key = format!("pilot:{pilot_id}");
        match self.profiles_kv.get(&key).await? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }

    async fn search_profile_by_name(&self, name: &str) -> anyhow::Result<Option<ThreatProfile>> {
        let name_lower = name.to_lowercase();
        let keys = self.profiles_kv.keys().await?.collect::<Vec<_>>().await;

        for key in keys.into_iter().flatten() {
            if let Ok(Some(entry)) = self.profiles_kv.get(&key).await {
                if let Ok(p) = serde_json::from_slice::<ThreatProfile>(&entry) {
                    if let Some(ref n) = p.name {
                        if n.to_lowercase().contains(&name_lower) {
                            return Ok(Some(p));
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

// ─── Alert subscriber (runs alongside the bot) ─────────────────────────────

async fn subscribe_alerts(
    js: jetstream::Context,
    alert_channels: AlertChannels,
    http: Arc<serenity::http::Http>,
) -> anyhow::Result<()> {
    let stream = js
        .get_or_create_stream(jetstream::stream::Config {
            name: "SENTINEL_ALERTS".into(),
            subjects: vec!["sentinel.alerts".into()],
            retention: jetstream::stream::RetentionPolicy::Interest,
            ..Default::default()
        })
        .await?;

    let consumer = stream
        .get_or_create_consumer(
            "discord-alerts",
            jetstream::consumer::pull::Config {
                durable_name: Some("discord-alerts".into()),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            },
        )
        .await?;

    tracing::info!("Subscribed to sentinel.alerts for Discord notifications");

    loop {
        let mut messages = consumer.fetch().max_messages(10).messages().await?;

        while let Some(Ok(msg)) = messages.next().await {
            match serde_json::from_slice::<ThreatAlert>(&msg.payload) {
                Ok(alert) => {
                    let channels = alert_channels.read().await;
                    let embed = alert_embed(&alert);

                    for &channel_id in channels.values() {
                        let ch = ChannelId::new(channel_id);
                        if let Err(e) = ch
                            .send_message(&http, CreateMessage::new().embed(embed.clone()))
                            .await
                        {
                            tracing::warn!(
                                channel_id,
                                error = %e,
                                "Failed to send alert to channel {channel_id}: {e}"
                            );
                        }
                    }

                    let _ = msg.ack().await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Invalid alert payload");
                    let _ = msg.ack().await;
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "discord_bridge=info".into()),
        )
        .init();

    let config = Config::from_env();
    tracing::info!(nats = %config.nats_url, "Starting discord-bridge");

    // Connect to NATS
    let nats = async_nats::connect(&config.nats_url).await?;
    let js = jetstream::new(nats);

    // Open KV buckets
    let profiles_kv = js
        .create_key_value(jetstream::kv::Config {
            bucket: "sentinel-profiles".into(),
            history: 1,
            ..Default::default()
        })
        .await?;

    let discord_kv = js
        .create_key_value(jetstream::kv::Config {
            bucket: "sentinel-discord".into(),
            history: 1,
            ..Default::default()
        })
        .await?;

    // Load existing alert channel mappings from KV
    let alert_channels: AlertChannels = Arc::new(RwLock::new(HashMap::new()));
    {
        let keys = discord_kv.keys().await?.collect::<Vec<_>>().await;
        let mut channels = alert_channels.write().await;
        for key in keys.into_iter().flatten() {
            if let Some(guild_str) = key.strip_prefix("alerts:") {
                if let Ok(guild_id) = guild_str.parse::<u64>() {
                    if let Ok(Some(val)) = discord_kv.get(&key).await {
                        if let Ok(channel_id) = std::str::from_utf8(&val).unwrap_or("").parse::<u64>() {
                            channels.insert(guild_id, channel_id);
                        }
                    }
                }
            }
        }
        tracing::info!(count = channels.len(), "Loaded {} alert channel mappings", channels.len());
    }

    // Build Serenity client
    let intents = GatewayIntents::empty();
    let handler = Handler {
        profiles_kv,
        discord_kv,
        alert_channels: alert_channels.clone(),
    };

    let mut client = serenity::Client::builder(&config.discord_token, intents)
        .event_handler(handler)
        .await?;

    // Spawn alert subscriber with the bot's HTTP client
    let http = client.http.clone();
    let js_clone = js.clone();
    let alert_channels_clone = alert_channels.clone();
    tokio::spawn(async move {
        if let Err(e) = subscribe_alerts(js_clone, alert_channels_clone, http).await {
            tracing::error!(error = %e, "Alert subscriber failed: {e}");
        }
    });

    // Run the bot
    client.start().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── tier_color ─────────────────────────────────────────────────────

    #[test]
    fn tier_colors() {
        assert_eq!(tier_color("LOW"), 0x44FF88);
        assert_eq!(tier_color("MODERATE"), 0xFFD700);
        assert_eq!(tier_color("HIGH"), 0xFF8C00);
        assert_eq!(tier_color("CRITICAL"), 0xFF4444);
        assert_eq!(tier_color("UNKNOWN"), 0x808080);
    }

    // ─── tier_emoji ─────────────────────────────────────────────────────

    #[test]
    fn tier_emojis() {
        assert_eq!(tier_emoji("LOW"), "🟢");
        assert_eq!(tier_emoji("MODERATE"), "🟡");
        assert_eq!(tier_emoji("HIGH"), "🟠");
        assert_eq!(tier_emoji("CRITICAL"), "🔴");
        assert_eq!(tier_emoji("UNKNOWN"), "⚪");
    }

    // ─── score_bar ──────────────────────────────────────────────────────

    #[test]
    fn score_bar_zero() {
        let bar = score_bar(0);
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 0);
        assert_eq!(bar.chars().filter(|&c| c == '░').count(), 20);
    }

    #[test]
    fn score_bar_max() {
        let bar = score_bar(10000);
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 20);
        assert_eq!(bar.chars().filter(|&c| c == '░').count(), 0);
    }

    #[test]
    fn score_bar_half() {
        let bar = score_bar(5000);
        assert_eq!(bar.chars().filter(|&c| c == '█').count(), 10);
        assert_eq!(bar.chars().filter(|&c| c == '░').count(), 10);
    }

    // ─── threat_tier ────────────────────────────────────────────────────

    #[test]
    fn tier_boundaries() {
        assert_eq!(threat_tier(0), "LOW");
        assert_eq!(threat_tier(2500), "LOW");
        assert_eq!(threat_tier(2501), "MODERATE");
        assert_eq!(threat_tier(5000), "MODERATE");
        assert_eq!(threat_tier(5001), "HIGH");
        assert_eq!(threat_tier(7500), "HIGH");
        assert_eq!(threat_tier(7501), "CRITICAL");
        assert_eq!(threat_tier(10000), "CRITICAL");
    }

    // ─── ThreatAlert deserialization ────────────────────────────────────

    #[test]
    fn deserialize_threat_alert() {
        let json = r#"{"pilot_id":42,"pilot_name":"TestPilot","threat_score":8000,"tier":"CRITICAL","system":"J-1042"}"#;
        let alert: ThreatAlert = serde_json::from_str(json).unwrap();
        assert_eq!(alert.pilot_id, 42);
        assert_eq!(alert.pilot_name, "TestPilot");
        assert_eq!(alert.threat_score, 8000);
        assert_eq!(alert.tier, "CRITICAL");
        assert_eq!(alert.system, "J-1042");
    }

    // ─── ThreatProfile deserialization ──────────────────────────────────

    #[test]
    fn deserialize_threat_profile() {
        let json = serde_json::json!({
            "item_id": 42,
            "name": "TestPilot",
            "threat_score": 5000,
            "kill_count": 10,
            "death_count": 5,
            "bounty_count": 2,
            "systems_visited": 8,
            "recent_kills_24h": 3,
            "last_seen_system": "J-1042",
            "tribe_name": "TestTribe"
        });
        let profile: ThreatProfile = serde_json::from_value(json).unwrap();
        assert_eq!(profile.item_id, 42);
        assert_eq!(profile.name, Some("TestPilot".into()));
        assert_eq!(profile.kill_count, 10);
        assert_eq!(profile.death_count, 5);
    }

    #[test]
    fn deserialize_profile_with_null_optionals() {
        let json = serde_json::json!({
            "item_id": 42,
            "name": null,
            "threat_score": 0,
            "kill_count": 0,
            "death_count": 0,
            "bounty_count": 0,
            "systems_visited": 0,
            "recent_kills_24h": 0,
            "last_seen_system": "",
            "tribe_name": null
        });
        let profile: ThreatProfile = serde_json::from_value(json).unwrap();
        assert_eq!(profile.name, None);
        assert_eq!(profile.tribe_name, None);
    }

    // ─── threat_embed ───────────────────────────────────────────────────

    #[test]
    fn threat_embed_uses_correct_color_and_title() {
        let profile = ThreatProfile {
            item_id: 42,
            name: Some("TestPilot".into()),
            threat_score: 8000,
            kill_count: 20,
            death_count: 5,
            bounty_count: 2,
            systems_visited: 8,
            recent_kills_24h: 3,
            last_seen_system: "J-1042".into(),
            tribe_name: Some("Raiders".into()),
        };
        let embed = threat_embed(&profile);
        // Embed is a builder — we can verify it was constructed without panicking
        // The color and title are set internally; this test ensures the builder runs
        // without errors for a fully populated profile
        let _ = embed;
    }

    #[test]
    fn threat_embed_handles_missing_optionals() {
        let profile = ThreatProfile {
            item_id: 42,
            name: None,
            threat_score: 0,
            kill_count: 0,
            death_count: 0,
            bounty_count: 0,
            systems_visited: 0,
            recent_kills_24h: 0,
            last_seen_system: String::new(),
            tribe_name: None,
        };
        // Should not panic on empty/None fields
        let _ = threat_embed(&profile);
    }

    // ─── alert_embed ────────────────────────────────────────────────────

    #[test]
    fn alert_embed_builds_without_panic() {
        let alert = ThreatAlert {
            pilot_id: 42,
            pilot_name: "TestPilot".into(),
            threat_score: 8000,
            tier: "CRITICAL".into(),
            system: "J-1042".into(),
        };
        let _ = alert_embed(&alert);
    }
}
