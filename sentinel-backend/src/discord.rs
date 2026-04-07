//! Discord bot integration — slash commands + real-time CRITICAL alerts.
//! Gated behind the `discord` feature flag.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serenity::Client;
use serenity::all::{
    AutocompleteChoice, ChannelId, ChannelType, Command, CommandDataOptionValue,
    CommandInteraction, CommandOptionType, CreateAutocompleteResponse, CreateCommand,
    CreateCommandOption, CreateEmbed, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateMessage, EventHandler, GatewayIntents, Interaction, Permissions, Ready,
};
use serenity::async_trait;
use sqlx::PgPool;
use tokio::sync::{RwLock, broadcast};

use crate::config::AppConfig;
use crate::threat_engine;
use crate::types::{AppState, ThreatProfile};

/// Per-guild alert channel config, loaded from DB at startup and kept in sync.
type AlertChannels = Arc<RwLock<HashMap<u64, u64>>>;

/// Tier color for embeds.
fn tier_color(tier: &str) -> u32 {
    match tier {
        "LOW" => 0x44FF88,
        "MODERATE" => 0xFFD700,
        "HIGH" => 0xFF8C00,
        "CRITICAL" => 0xFF4444,
        _ => 0x808080,
    }
}

/// Tier emoji.
fn tier_emoji(tier: &str) -> &'static str {
    match tier {
        "LOW" => "🟢",
        "MODERATE" => "🟡",
        "HIGH" => "🟠",
        "CRITICAL" => "🔴",
        _ => "⚪",
    }
}

/// Build a visual progress bar (20 chars wide).
fn score_bar(score: u64) -> String {
    let filled = ((score as f64 / 10000.0) * 20.0).round() as usize;
    let empty = 20_usize.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// Build the threat profile embed for a single pilot.
fn threat_embed(profile: &ThreatProfile) -> CreateEmbed {
    let tier = threat_engine::threat_tier(profile.threat_score);
    let titles = threat_engine::earned_titles(profile);
    let kd = if profile.death_count > 0 {
        format!(
            "{:.2}",
            profile.kill_count as f64 / profile.death_count as f64
        )
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

    let last_seen = if profile.last_seen_system_name.is_empty() {
        if profile.last_seen_system.is_empty() {
            "Unknown".to_string()
        } else {
            profile.last_seen_system.clone()
        }
    } else {
        profile.last_seen_system_name.clone()
    };

    let tribe = if profile.tribe_name.is_empty() {
        "None".to_string()
    } else {
        profile.tribe_name.clone()
    };

    let title_str = if titles.is_empty() {
        "None earned".to_string()
    } else {
        titles.join(", ")
    };

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
        .field(
            "Threat Tier",
            format!("{} {}", tier_emoji(tier), tier),
            true,
        )
        .field("Kill Count", profile.kill_count.to_string(), true)
        .field("Death Count", profile.death_count.to_string(), true)
        .field("K/D Ratio", kd, true)
        .field("Bounties", profile.bounty_count.to_string(), true)
        .field("Systems Visited", profile.systems_visited.to_string(), true)
        .field("Last Seen", last_seen, true)
        .field("Tribe", tribe, true)
        .field("Titles", title_str, false)
        .footer(serenity::all::CreateEmbedFooter::new(
            "SENTINEL — EVE Frontier Threat Intelligence",
        ))
        .timestamp(serenity::model::timestamp::Timestamp::now())
}

/// Build the leaderboard embed.
fn leaderboard_embed(
    profiles: &[&ThreatProfile],
    stats: &crate::types::AggregateStats,
) -> CreateEmbed {
    let mut desc = String::new();
    for (i, p) in profiles.iter().enumerate().take(10) {
        let tier = threat_engine::threat_tier(p.threat_score);
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

    CreateEmbed::new()
        .title("🏆 SENTINEL Threat Leaderboard")
        .color(0xFF4444)
        .description(desc)
        .field("Pilots Tracked", stats.total_tracked.to_string(), true)
        .field(
            "Avg Score",
            format!("{:.2}", stats.avg_score as f64 / 100.0),
            true,
        )
        .field("Kills (24h)", stats.kills_24h.to_string(), true)
        .field(
            "Hottest System",
            if stats.top_system.is_empty() {
                "—"
            } else {
                &stats.top_system
            },
            true,
        )
        .field("Events (24h)", stats.total_events.to_string(), true)
        .footer(serenity::all::CreateEmbedFooter::new(
            "SENTINEL — EVE Frontier Threat Intelligence",
        ))
        .timestamp(serenity::model::timestamp::Timestamp::now())
}

/// Build an alert embed for a CRITICAL event.
fn alert_embed(event_type: &str, profile: &ThreatProfile, event_desc: &str) -> CreateEmbed {
    let tier = threat_engine::threat_tier(profile.threat_score);
    CreateEmbed::new()
        .title("⚠️ CRITICAL THREAT ALERT")
        .color(0xFF4444)
        .description(event_desc)
        .field(
            "Pilot",
            profile.name.as_deref().unwrap_or("Unknown Pilot"),
            true,
        )
        .field(
            "Threat Score",
            format!(
                "{} {:.2}",
                tier_emoji(tier),
                profile.threat_score as f64 / 100.0
            ),
            true,
        )
        .field("Event", event_type, true)
        .field("Kills", profile.kill_count.to_string(), true)
        .field("Bounties", profile.bounty_count.to_string(), true)
        .field(
            "Last Seen",
            if profile.last_seen_system_name.is_empty() {
                &profile.last_seen_system
            } else {
                &profile.last_seen_system_name
            },
            true,
        )
        .footer(serenity::all::CreateEmbedFooter::new(
            "SENTINEL — EVE Frontier Threat Intelligence",
        ))
        .timestamp(serenity::model::timestamp::Timestamp::now())
}

struct Handler {
    config: AppConfig,
    state: Arc<RwLock<AppState>>,
    alert_channels: AlertChannels,
    db_pool: PgPool,
    commands_run: Arc<AtomicU64>,
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
                        "Pilot name to search for",
                    )
                    .required(true)
                    .set_autocomplete(true),
                ),
            CreateCommand::new("leaderboard").description("Show the top 10 most dangerous pilots"),
            CreateCommand::new("kills").description("Recent kills feed"),
            CreateCommand::new("systems").description("Most active solar systems right now"),
            CreateCommand::new("events").description("Recent event feed (kills, jumps, bounties)"),
            CreateCommand::new("stats").description("Aggregate threat network statistics"),
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
                    "clear",
                    "Stop sending alerts in this server",
                ))
                .add_option(CreateCommandOption::new(
                    CommandOptionType::SubCommand,
                    "status",
                    "Show current alert configuration",
                ))
                .default_member_permissions(Permissions::MANAGE_CHANNELS),
        ];

        if let Err(e) = Command::set_global_commands(&ctx.http, commands).await {
            tracing::warn!(error = %e, "Failed to register slash commands: {e}");
        } else {
            tracing::info!("Discord slash commands registered");
        }

        let total = self.state.read().await.live.profiles.len();
        ctx.set_activity(Some(serenity::all::ActivityData::watching(format!(
            "{total} pilots"
        ))));
    }

    async fn interaction_create(&self, ctx: serenity::all::Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(cmd) => {
                self.handle_command(&ctx, &cmd).await;
            }
            Interaction::Autocomplete(auto) => {
                self.handle_autocomplete(&ctx, &auto).await;
            }
            _ => {}
        }
    }
}

impl Handler {
    /// Route an incoming slash command to the appropriate handler and reply with an embed.
    async fn handle_command(&self, ctx: &serenity::all::Context, cmd: &CommandInteraction) {
        let command_name = cmd.data.name.as_str();
        let guild_id = cmd.guild_id.map(|g| g.get());
        let user = cmd.user.name.as_str();
        let arg = cmd.data.options.first().and_then(|o| match &o.value {
            CommandDataOptionValue::String(s) => Some(s.as_str().to_string()),
            CommandDataOptionValue::Integer(i) => Some(i.to_string()),
            CommandDataOptionValue::SubCommand(_) => Some(o.name.clone()),
            _ => None,
        });
        tracing::debug!(
            command = command_name,
            guild_id,
            user,
            arg = arg.as_deref(),
            "/{command_name} from {user}{}",
            arg.as_deref()
                .map(|a| format!(" ({a})"))
                .unwrap_or_default()
        );
        self.commands_run.fetch_add(1, Ordering::Relaxed);

        let result = match command_name {
            "threat" => self.cmd_threat(cmd).await,
            "leaderboard" => self.cmd_leaderboard().await,
            "kills" => self.cmd_kills().await,
            "systems" => self.cmd_systems().await,
            "events" => self.cmd_events().await,
            "stats" => self.cmd_stats().await,
            "alerts" => {
                self.cmd_alerts(ctx, cmd).await;
                return;
            }
            _ => None,
        };

        if let Some(embed) = result {
            let msg = CreateInteractionResponseMessage::new().embed(embed);
            let response = CreateInteractionResponse::Message(msg);
            if let Err(e) = cmd.create_response(&ctx.http, response).await {
                tracing::warn!(error = %e, "Failed to respond to command: {e}");
            }
        }
    }

    /// `/threat <pilot>` — look up a pilot by name (case-insensitive prefix match).
    /// Returns a rich embed with score, tier, K/D, titles, and location.
    /// Shows top 5 partial matches if no exact match is found.
    async fn cmd_threat(&self, cmd: &CommandInteraction) -> Option<CreateEmbed> {
        let pilot_name = cmd.data.options.first()?.value.as_str()?;

        let state = self.state.read().await;
        let query = pilot_name.to_lowercase();

        // Exact match first
        if let Some(profile) = state
            .live
            .profiles
            .values()
            .find(|p| p.name.as_deref().unwrap_or("").to_lowercase() == query)
        {
            return Some(threat_embed(profile));
        }

        // Partial matches
        let mut matches: Vec<&ThreatProfile> = state
            .live
            .profiles
            .values()
            .filter(|p| {
                p.name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        matches.sort_by(|a, b| b.threat_score.cmp(&a.threat_score));

        if matches.len() == 1 {
            return Some(threat_embed(matches[0]));
        }

        if matches.len() > 1 {
            let list: String = matches
                .iter()
                .take(5)
                .map(|p| {
                    let tier = threat_engine::threat_tier(p.threat_score);
                    format!(
                        "{} **{}** — `{:.2}` ({})",
                        tier_emoji(tier),
                        p.name.as_deref().unwrap_or("Unknown Pilot"),
                        p.threat_score as f64 / 100.0,
                        tier
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            return Some(
                CreateEmbed::new()
                    .title("Multiple matches found")
                    .color(0xFFD700)
                    .description(format!(
                        "Found {} matches for **{pilot_name}**:\n\n{list}",
                        matches.len()
                    ))
                    .footer(serenity::all::CreateEmbedFooter::new(
                        "Use a more specific name to get the full profile",
                    )),
            );
        }

        Some(
            CreateEmbed::new()
                .title("Pilot not found")
                .color(0x808080)
                .description(format!(
                    "No pilot matching **{pilot_name}** found in threat database."
                )),
        )
    }

    /// `/leaderboard` — top 10 pilots by threat score with tier medals and aggregate stats.
    async fn cmd_leaderboard(&self) -> Option<CreateEmbed> {
        let state = self.state.read().await;
        let mut profiles: Vec<&ThreatProfile> = state.live.profiles.values().collect();
        profiles.sort_by(|a, b| b.threat_score.cmp(&a.threat_score));
        let stats = state.live.compute_stats(self.config.max_recent_events);
        Some(leaderboard_embed(&profiles, &stats))
    }

    /// `/kills` — last 10 kills from in-memory event buffer.
    async fn cmd_kills(&self) -> Option<CreateEmbed> {
        let state = self.state.read().await;
        let kills: Vec<_> = state
            .live
            .recent_events
            .iter()
            .filter(|e| e.event_type == "kill" || e.event_type == "structure_destroyed")
            .take(10)
            .cloned()
            .collect();

        if kills.is_empty() {
            return Some(
                CreateEmbed::new()
                    .title("Recent Kills")
                    .color(0x808080)
                    .description("No kills recorded yet."),
            );
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let lines: String = kills
            .iter()
            .map(|e| {
                let killer_id = e.data["killer_character_id"].as_u64().unwrap_or(0);
                let victim_id = e.data["target_item_id"].as_u64().unwrap_or(0);
                let system = e.data["solar_system_id"].as_str().unwrap_or("").to_string();
                let killer_name = state
                    .live
                    .name_cache
                    .get(&killer_id)
                    .cloned()
                    .unwrap_or_else(|| format!("#{killer_id}"));
                let victim_name = state
                    .live
                    .name_cache
                    .get(&victim_id)
                    .cloned()
                    .unwrap_or_else(|| format!("#{victim_id}"));
                let system_name = state
                    .live
                    .system_name_cache
                    .get(&system)
                    .cloned()
                    .unwrap_or(system);
                let ago_secs = (now_ms.saturating_sub(e.timestamp_ms)) / 1000;
                let ago = if ago_secs < 60 {
                    format!("{ago_secs}s ago")
                } else if ago_secs < 3600 {
                    format!("{}m ago", ago_secs / 60)
                } else {
                    format!("{}h ago", ago_secs / 3600)
                };
                let emoji = if e.event_type == "structure_destroyed" {
                    "💥"
                } else {
                    "☠️"
                };
                format!(
                    "{emoji} **{killer_name}** killed **{victim_name}** in {system_name} — *{ago}*"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        Some(
            CreateEmbed::new()
                .title("Recent Kills")
                .color(0xFF4444)
                .description(lines),
        )
    }

    /// `/systems` — top 10 most active solar systems by pilot count.
    async fn cmd_systems(&self) -> Option<CreateEmbed> {
        let state = self.state.read().await;

        let mut counts: std::collections::HashMap<&str, (u64, &str)> =
            std::collections::HashMap::new();
        for p in state.live.profiles.values() {
            if p.last_seen_system.is_empty() {
                continue;
            }
            let entry = counts
                .entry(&p.last_seen_system)
                .or_insert((0, &p.last_seen_system_name));
            entry.0 += 1;
        }

        let mut systems: Vec<(&str, u64, &str)> = counts
            .into_iter()
            .map(|(id, (count, name))| (id, count, name))
            .collect();
        systems.sort_by(|a, b| b.1.cmp(&a.1));

        if systems.is_empty() {
            return Some(
                CreateEmbed::new()
                    .title("Active Systems")
                    .color(0x808080)
                    .description("No pilot location data yet."),
            );
        }

        let lines: String = systems
            .iter()
            .take(10)
            .enumerate()
            .map(|(i, (id, count, name))| {
                let display = if name.is_empty() { id } else { name };
                let pilot_label = if *count == 1 { "pilot" } else { "pilots" };
                format!("{}. **{display}** — {count} {pilot_label}", i + 1)
            })
            .collect::<Vec<_>>()
            .join("\n");

        Some(
            CreateEmbed::new()
                .title("Most Active Systems")
                .color(0x4488FF)
                .description(lines),
        )
    }

    /// `/events` — last 10 events of any type from the live feed.
    async fn cmd_events(&self) -> Option<CreateEmbed> {
        let state = self.state.read().await;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let events: Vec<&crate::types::RawEvent> =
            state.live.recent_events.iter().take(10).collect();

        if events.is_empty() {
            return Some(
                CreateEmbed::new()
                    .title("Live Event Feed")
                    .color(0x808080)
                    .description("No events recorded yet."),
            );
        }

        let lines: String = events
            .iter()
            .map(|e| {
                let ago_secs = (now_ms.saturating_sub(e.timestamp_ms)) / 1000;
                let ago = if ago_secs < 60 {
                    format!("{ago_secs}s ago")
                } else if ago_secs < 3600 {
                    format!("{}m ago", ago_secs / 60)
                } else {
                    format!("{}h ago", ago_secs / 3600)
                };
                let system_id = e
                    .data
                    .get("solar_system_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let system_name = if system_id.is_empty() {
                    String::new()
                } else {
                    state
                        .live
                        .system_name_cache
                        .get(system_id)
                        .cloned()
                        .unwrap_or_else(|| system_id.to_string())
                };
                match e.event_type.as_str() {
                    "kill" | "structure_destroyed" => {
                        let killer_id = e.data["killer_character_id"].as_u64().unwrap_or(0);
                        let victim_id = e.data["target_item_id"].as_u64().unwrap_or(0);
                        let killer = state.live.name_cache.get(&killer_id).cloned().unwrap_or_else(|| format!("#{killer_id}"));
                        let victim = state.live.name_cache.get(&victim_id).cloned().unwrap_or_else(|| format!("#{victim_id}"));
                        let emoji = if e.event_type == "structure_destroyed" { "💥" } else { "☠️" };
                        if system_name.is_empty() {
                            format!("{emoji} **{killer}** killed **{victim}** — *{ago}*")
                        } else {
                            format!("{emoji} **{killer}** killed **{victim}** in {system_name} — *{ago}*")
                        }
                    }
                    "jump" => {
                        let char_id = e.data["character_id"].as_u64().unwrap_or(0);
                        let pilot = state.live.name_cache.get(&char_id).cloned().unwrap_or_else(|| format!("#{char_id}"));
                        let dest = e.data["dest_gate"].as_str().unwrap_or("").to_string();
                        if system_name.is_empty() {
                            format!("🚀 **{pilot}** jumped — *{ago}*")
                        } else if dest.is_empty() || dest == system_id {
                            format!("🚀 **{pilot}** jumped to {system_name} — *{ago}*")
                        } else {
                            format!("🚀 **{pilot}** jumped to {system_name} via {dest} — *{ago}*")
                        }
                    }
                    "bounty" => {
                        let target_id = e.data["target_item_id"].as_u64().unwrap_or(0);
                        let target = state.live.name_cache.get(&target_id).cloned().unwrap_or_else(|| format!("#{target_id}"));
                        format!("💰 Bounty on **{target}** — *{ago}*")
                    }
                    _ => format!("• {} — *{ago}*", e.event_type),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");

        Some(
            CreateEmbed::new()
                .title("Live Event Feed")
                .color(0x44FF88)
                .description(lines),
        )
    }

    /// `/stats` — aggregate threat network statistics.
    async fn cmd_stats(&self) -> Option<CreateEmbed> {
        let state = self.state.read().await;
        let stats = state.live.compute_stats(self.config.max_recent_events);

        let critical = state
            .live
            .profiles
            .values()
            .filter(|p| threat_engine::threat_tier(p.threat_score) == "CRITICAL")
            .count();
        let high = state
            .live
            .profiles
            .values()
            .filter(|p| threat_engine::threat_tier(p.threat_score) == "HIGH")
            .count();
        let active = state
            .live
            .profiles
            .values()
            .filter(|p| p.kill_count > 0 || p.death_count > 0)
            .count();

        let top_system = if stats.top_system.is_empty() {
            "Unknown".to_string()
        } else {
            stats.top_system.clone()
        };

        Some(
            CreateEmbed::new()
                .title("SENTINEL Network Stats")
                .color(0xFF8C00)
                .field("Tracked Pilots", stats.total_tracked.to_string(), true)
                .field("Active Pilots", active.to_string(), true)
                .field(
                    "Avg Threat Score",
                    format!("{:.2}", stats.avg_score as f64 / 100.0),
                    true,
                )
                .field("Kills (24h)", stats.kills_24h.to_string(), true)
                .field("Events (24h)", stats.total_events.to_string(), true)
                .field("Hottest System", top_system, true)
                .field(
                    "By Tier",
                    format!("🔴 CRITICAL: {critical}\n🟠 HIGH: {high}"),
                    false,
                ),
        )
    }
    /// `/alerts set|clear|status` — manage the guild's CRITICAL alert channel.
    /// Requires MANAGE_CHANNELS permission. Config is persisted to Postgres.
    async fn cmd_alerts(&self, ctx: &serenity::all::Context, cmd: &CommandInteraction) {
        let guild_id = match cmd.guild_id {
            Some(id) => id.get(),
            None => {
                let embed = CreateEmbed::new()
                    .title("Server only")
                    .color(0xFF4444)
                    .description("Alert configuration is only available in servers, not DMs.");
                let msg = CreateInteractionResponseMessage::new().embed(embed);
                let _ = cmd
                    .create_response(&ctx.http, CreateInteractionResponse::Message(msg))
                    .await;
                return;
            }
        };

        let subcommand = cmd.data.options.first().map(|o| o.name.as_str());

        let embed = match subcommand {
            Some("set") => {
                let channel_id = cmd.data.options.first().and_then(|sub| {
                    if let CommandDataOptionValue::SubCommand(opts) = &sub.value {
                        opts.first().and_then(|opt| opt.value.as_channel_id())
                    } else {
                        None
                    }
                });

                match channel_id {
                    Some(ch) => {
                        let ch_id = ch.get();
                        // Persist to DB
                        if let Err(e) =
                            crate::db::set_alert_channel(&self.db_pool, guild_id, ch_id).await
                        {
                            tracing::error!(error = %e, "Failed to save alert channel: {e}");
                            CreateEmbed::new()
                                .title("Error")
                                .color(0xFF4444)
                                .description("Failed to save alert configuration.")
                        } else {
                            // Update in-memory map
                            self.alert_channels.write().await.insert(guild_id, ch_id);
                            tracing::info!(guild = %guild_id, channel = %ch_id, "Alert channel set: guild={guild_id} channel={ch_id}");
                            CreateEmbed::new()
                                .title("🔔 Alerts configured")
                                .color(0x44FF88)
                                .description(format!(
                                    "CRITICAL threat alerts will now be posted to <#{ch_id}>.\n\n\
                                     **Triggers:**\n\
                                     • Kill events involving CRITICAL-tier pilots (score > 75.00)\n\
                                     • Rate limited to 1 alert per pilot per 5 minutes"
                                ))
                        }
                    }
                    None => CreateEmbed::new()
                        .title("Error")
                        .color(0xFF4444)
                        .description("Please specify a channel."),
                }
            }
            Some("clear") => {
                if let Err(e) = crate::db::clear_alert_channel(&self.db_pool, guild_id).await {
                    tracing::error!(error = %e, "Failed to clear alert channel: {e}");
                    CreateEmbed::new()
                        .title("Error")
                        .color(0xFF4444)
                        .description("Failed to clear alert configuration.")
                } else {
                    self.alert_channels.write().await.remove(&guild_id);
                    tracing::info!(guild = %guild_id, "Alert channel cleared: guild={guild_id}");
                    CreateEmbed::new()
                        .title("🔕 Alerts disabled")
                        .color(0x808080)
                        .description(
                            "CRITICAL threat alerts have been disabled for this server.\n\
                             Use `/alerts set #channel` to re-enable.",
                        )
                }
            }
            Some("status") | _ => {
                let channels = self.alert_channels.read().await;
                if let Some(&ch_id) = channels.get(&guild_id) {
                    CreateEmbed::new()
                        .title("🔔 SENTINEL Alert Status")
                        .color(0x44FF88)
                        .description(format!(
                            "CRITICAL threat alerts are active in <#{ch_id}>.\n\n\
                             **Triggers:**\n\
                             • Kill events involving CRITICAL-tier pilots (score > 75.00)\n\
                             • Rate limited to 1 alert per pilot per 5 minutes\n\n\
                             Use `/alerts clear` to disable."
                        ))
                } else {
                    CreateEmbed::new()
                        .title("🔕 SENTINEL Alert Status")
                        .color(0x808080)
                        .description(
                            "No alert channel configured for this server.\n\
                             Use `/alerts set #channel` to enable CRITICAL threat alerts.",
                        )
                }
            }
        };

        let msg = CreateInteractionResponseMessage::new().embed(embed);
        if let Err(e) = cmd
            .create_response(&ctx.http, CreateInteractionResponse::Message(msg))
            .await
        {
            tracing::warn!(error = %e, "Failed to respond to alerts command: {e}");
        }
    }

    /// Handle autocomplete for `/threat` — returns up to 25 pilot names matching the partial input.
    async fn handle_autocomplete(
        &self,
        ctx: &serenity::all::Context,
        auto: &serenity::all::CommandInteraction,
    ) {
        let query = auto
            .data
            .options
            .first()
            .and_then(|o| o.value.as_str())
            .unwrap_or("")
            .to_lowercase();

        let state = self.state.read().await;
        let mut matches: Vec<(&str, u64)> = state
            .live
            .profiles
            .values()
            .filter(|p| {
                p.name.is_some()
                    && (query.is_empty()
                        || p.name
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains(&query))
            })
            .map(|p| (p.name.as_deref().unwrap_or(""), p.threat_score))
            .collect();
        matches.sort_by(|a, b| b.1.cmp(&a.1));

        let choices: Vec<AutocompleteChoice> = matches
            .into_iter()
            .take(25)
            .map(|(name, score)| {
                let tier = threat_engine::threat_tier(score);
                AutocompleteChoice::new(
                    format!(
                        "{} {} — {:.2}",
                        tier_emoji(tier),
                        name,
                        score as f64 / 100.0
                    ),
                    name,
                )
            })
            .collect();

        let response = CreateAutocompleteResponse::new().set_choices(choices);
        if let Err(e) = auto
            .create_response(&ctx.http, CreateInteractionResponse::Autocomplete(response))
            .await
        {
            tracing::warn!(error = %e, "Failed to send autocomplete: {e}");
        }
    }
}

/// Alert listener — watches SSE events and broadcasts to all guilds with alert channels.
async fn alert_loop(
    http: Arc<serenity::http::Http>,
    state: Arc<RwLock<AppState>>,
    alert_channels: AlertChannels,
    mut sse_rx: broadcast::Receiver<String>,
) {
    // Per-guild, per-pilot rate limit: guild_id -> (char_id -> last_alert_time)
    let mut rate_limits: HashMap<u64, HashMap<u64, Instant>> = HashMap::new();
    let cooldown = Duration::from_secs(300);

    tracing::info!("Discord alert listener started");

    loop {
        match sse_rx.recv().await {
            Ok(json_str) => {
                let Ok(event) = serde_json::from_str::<serde_json::Value>(&json_str) else {
                    continue;
                };

                let event_type = event["event_type"].as_str().unwrap_or_default();
                if event_type != "kill" {
                    continue;
                }

                // Get current alert channels
                let channels = alert_channels.read().await;
                if channels.is_empty() {
                    continue;
                }

                let data = &event["data"];
                let char_ids: Vec<u64> = ["killer_character_id", "killer_id"]
                    .iter()
                    .filter_map(|key| data[key].as_u64())
                    .collect();

                let app_state = state.read().await;
                for char_id in char_ids {
                    let Some(profile) = app_state.live.profiles.get(&char_id) else {
                        continue;
                    };
                    if threat_engine::threat_tier(profile.threat_score) != "CRITICAL" {
                        continue;
                    }

                    let victim_id = data["target_item_id"]
                        .as_u64()
                        .or_else(|| data["victim_id"].as_u64());
                    let victim_name = victim_id
                        .and_then(|id| app_state.live.profiles.get(&id))
                        .and_then(|p| p.name.as_deref())
                        .unwrap_or("Unknown");
                    let system = if !profile.last_seen_system_name.is_empty() {
                        &profile.last_seen_system_name
                    } else {
                        &profile.last_seen_system
                    };

                    let desc = format!(
                        "**{}** killed **{}** in **{}**",
                        profile.name.as_deref().unwrap_or("Unknown Pilot"),
                        victim_name,
                        system
                    );
                    let embed = alert_embed(event_type, profile, &desc);

                    for (&guild_id, &channel_id) in channels.iter() {
                        // Per-guild rate limit
                        let guild_limits = rate_limits.entry(guild_id).or_default();
                        if let Some(last) = guild_limits.get(&char_id) {
                            if last.elapsed() < cooldown {
                                continue;
                            }
                        }
                        guild_limits.insert(char_id, Instant::now());

                        let channel = ChannelId::new(channel_id);
                        let msg = CreateMessage::new().embed(embed.clone());
                        if let Err(e) = channel.send_message(&http, msg).await {
                            tracing::warn!(
                                guild = guild_id,
                                channel = channel_id,
                                error = %e,
                                "Failed to send alert to guild={guild_id} channel={channel_id}: {e}"
                            );
                        }
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(
                    lagged_events = n,
                    "Discord alert listener lagged by {n} events"
                );
            }
            Err(broadcast::error::RecvError::Closed) => {
                tracing::info!("SSE broadcast channel closed, stopping alert listener");
                break;
            }
        }
    }
}

/// Start the Discord bot. Runs forever — call from a `tokio::spawn`.
pub async fn run_discord_bot(
    config: AppConfig,
    state: Arc<RwLock<AppState>>,
    sse_tx: broadcast::Sender<String>,
    db_pool: PgPool,
    commands_run: Arc<AtomicU64>,
) {
    let token = config.discord_token.clone();

    // Load alert channels from DB
    let alert_map = match crate::db::load_alert_channels(&db_pool).await {
        Ok(map) => {
            if !map.is_empty() {
                tracing::info!(
                    guild_count = map.len(),
                    "Loaded {} guild alert channel(s) from database",
                    map.len()
                );
            }
            map
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to load alert channels from DB: {e}");
            HashMap::new()
        }
    };
    let alert_channels: AlertChannels = Arc::new(RwLock::new(alert_map));

    let intents = GatewayIntents::empty();

    let handler = Handler {
        config: config.clone(),
        state: state.clone(),
        alert_channels: alert_channels.clone(),
        db_pool,
        commands_run,
    };

    let mut client = match Client::builder(&token, intents)
        .event_handler(handler)
        .await
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to create Discord client: {e}");
            return;
        }
    };

    // Spawn alert listener
    let http = client.http.clone();
    let alert_state = state.clone();
    let sse_rx = sse_tx.subscribe();
    tokio::spawn(async move {
        alert_loop(http, alert_state, alert_channels, sse_rx).await;
    });

    if let Err(e) = client.start().await {
        tracing::warn!(error = %e, "Discord client error: {e}");
    }
}
