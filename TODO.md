# SENTINEL — Hackathon TODO

Deadline: March 31, 2026 23:59 UTC

## Critical (must do)

- [ ] **Fix SST deploy** — add NEON_ORG_ID secret, verify full deploy works
- [ ] **Complete on-chain publisher** — build ProgrammableTransaction
      for `threat_registry::batch_update`, resolve object refs via
      LedgerService.GetObject, sign with admin key, submit via gRPC.
      This is THE differentiator — scores must be verifiable on-chain.
- [ ] **Verify live events flowing** — check event sample logs, confirm
      Stillness world package ID matches. May need upgraded package ID
      if world contracts were upgraded.
- [ ] **Submit on Deepsurge** — register, submit repo link + materials

## High priority

- [ ] **Frontend: show new fields** — display `last_seen_system_name`,
      `tribe_name`, `titles`, `threat_tier` from API response.
      Titles should render as badges on threat cards.
- [ ] **Discord bot** — Rust bot (discord.js) that polls /api/data.
      Commands: `/threat <pilot>`, `/leaderboard`, `/alerts <channel>`.
      ~2 hours. Judges love seeing integrations.
- [ ] **Demo video** — clear walkthrough: live events flowing, scoring,
      dashboard, threat tiers, titles, on-chain registry, gate blocking.
      This directly affects the "Visual Presentation & Demo" criterion.

## Medium priority

- [ ] **System map** — fetch all 24,502 systems from World API
      (`GET /v2/solarsystems`), render as 2D Canvas scatter plot.
      Color by threat density. ~3-4 hours.
- [ ] **Smart Gate demo** — deploy a gate on Stillness, authorize
      SENTINEL, demonstrate a pilot being blocked. Even a video of
      this would score high on "Best Live Frontier Integration" ($11K).
- [ ] **Historical killmail dedup** — check if we're double-counting
      kills that are both in DB and GraphQL on restart.

## Nice to have

- [ ] **AI narrative feed** — use Claude API to generate story-style
      descriptions of events. Template fallback if API unavailable.
      WatchTower does this — would match their polish.
- [ ] **Earned titles expansion** — add more titles based on patterns:
      "Gate Camper" (kills near same system repeatedly),
      "Fleet Commander" (kills at same timestamp as others),
      "Night Stalker" (kills during off-peak hours).
- [ ] **Alt detection** — WatchTower does behavioral fingerprint
      comparison. Could flag accounts with similar kill patterns
      or movement as likely alts. Complex, probably post-hackathon.
- [ ] **Webhook/alert system** — push notifications for high-threat
      events. Discord webhooks for kills, bounties, gate blocks.
      Monolith does this well.
- [ ] **Subscription model** — on-chain paid tiers via Move contract.
      WatchTower has Scout/Oracle/Spymaster tiers. Judges might like
      seeing a business model, but risks looking extractive.

## Ideas from competitor projects

### From WatchTower (AreteDriver/watchtower)

- Behavioral fingerprinting (solo vs fleet, timezone clustering)
- Kill network graphs (attacker-victim relationships)
- AI-generated story feed with Claude API
- Discord bot with 21 slash commands
- On-chain subscription tiers
- OPSEC scoring
- 822+ tests, 80% coverage

### From Monolith (AreteDriver/monolith)

- Canvas2D system map with 24K systems at 60fps
- Anomaly detection rules engine (39 rules)
- Bug report generation with chain evidence
- GitHub auto-filing for critical detections
- Item ledger tracking (mints, transfers, destructions)

### From Frontier Tribe OS (AreteDriver/frontier-tribe-os)

- AI-powered threat briefings ("Warden" module)
- Census/roster management with World API sync
- Production job queue tracking
- Non-custodial treasury monitoring via Sui wallet

### From other hackathon projects

- Powerlay Frontier — tribe contract system, production planning
- CradleOS — ship fitting tool, character/tribe lookup via GraphQL
- KARUM — marketplace network for SSU owners
- Flappy Frontier — on-chain mini-game, sponsored gas for UX

## Architecture notes

- World API: `https://world-api-stillness.live.tech.evefrontier.com/v2`
- Sui GraphQL: `https://graphql.testnet.sui.io/graphql`
- Stillness world package: `0x28b497559d65ab320d9da4613bf2498d5946b2c0ae3597ccfda3072ce127448c`
- Killmail registry: `0x7fd9a32d0bbe7b1cfbb7140b1dd4312f54897de946c399edb21c3a12e52ce283`
- EVE Frontier event fields use nested format: `{"item_id": "123", "tenant": "stillness"}`
- Kill timestamps are in seconds (not ms) — convert on ingest
