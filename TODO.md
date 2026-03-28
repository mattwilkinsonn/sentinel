# SENTINEL — Hackathon TODO

Deadline: March 31, 2026 23:59 UTC

## Critical (must do)

- [x] ~~Fix SST deploy~~ — NEON_ORG_ID, historyRetention,
      Cloudflare DNS, ARM containers on GHCR
- [x] ~~Complete on-chain publisher~~ — built, but RPC returns
      "Invalid value" — needs BCS serialization debugging
- [x] ~~Verify live events flowing~~ — gRPC stream connected,
      Stillness world package ID confirmed, diagnostic logging added
- [ ] **Debug publisher RPC error** — `sui_executeTransactionBlock`
      rejects our BCS payload. May need to use `SimulateTransaction`
      first, or switch to the GraphQL `executeTransactionBlock` API.
- [ ] **Deploy to production** — SST deploy blocked by Cloudflare
      StaticSite bug, switched to AWS StaticSite. Needs clean deploy.
- [ ] **Submit on Deepsurge** — register, submit repo link + materials

## High priority

- [x] ~~Frontend: show new fields~~ — titles, system names, tribe
      names, threat tiers all displaying
- [x] ~~Earned titles~~ — 14 title types computed from profile stats
- [x] ~~Historical data loading~~ — killmails, character names,
      jump events, character creation events from GraphQL
- [x] ~~New Pilots view~~ — separate page with 24h count
- [x] ~~Feed improvements~~ — separate gameplay/new_pilot events,
      gate names resolved, Smart Gate jump labels
- [ ] **Discord bot** — TS bot (discord.js) that polls /api/data.
      Commands: `/threat <pilot>`, `/leaderboard`, `/alerts`.
      ~2 hours. Judges love seeing integrations.
- [ ] **Demo video** — clear walkthrough: live events flowing,
      scoring, dashboard, threat tiers, titles, on-chain registry,
      gate blocking. Directly affects "Visual Presentation" criterion.

## Medium priority

- [ ] **Time filters** — reusable dropdown component for 1h/24h/7d/all.
      Wire into: events feed, systems view, tracked pilots, kills view.
      Custom date range picker as stretch goal.
- [ ] **Secondary kills view** — time-filtered kill leaderboard
      (1h/24h/7d/custom) as alternate to the fixed 24h + all-time view.
- [ ] **Pilot search** — search for a specific pilot by name or ID
      across all tracked pilots (not just active ones).
- [ ] **System map** — fetch all 24,502 systems from World API,
      render as 3D Three.js scatter plot (like ef-map.com).
      Color by threat density, animate live events. ~4 hours.
- [ ] **Smart Gate demo** — deploy a gate on Stillness, authorize
      SENTINEL, demonstrate a pilot being blocked. Even a video of
      this scores high on "Best Live Frontier Integration" ($11K).

## Nice to have

- [ ] **AI narrative feed** — use Claude API to generate story-style
      descriptions of events. Template fallback if API unavailable.
- [ ] **Earned titles expansion** — add pattern-based titles:
      "Gate Camper", "Fleet Commander", "Night Stalker".
- [ ] **Alt detection** — behavioral fingerprint comparison to flag
      likely alternate accounts.
- [ ] **Webhook/alert system** — Discord webhooks for kills, bounties,
      gate blocks. Push notifications for high-threat events.
- [ ] **Migrate to SolidStart** — SSR, file-based routing, API routes.
      Post-hackathon improvement for SEO and initial load performance.
- [ ] **StatusChangedEvent / GateCreatedEvent** — track assembly
      online/offline and new gate deployments from chain events.

## Done (completed this session)

- [x] Postgres persistence with SQLx + migrations
- [x] Docker Compose for local dev
- [x] World REST API integration (system names, tribes)
- [x] Historical killmail loading from Sui GraphQL
- [x] Character name resolution from GraphQL
- [x] Jump event loading with gate name resolution
- [x] New Pilots separate view and API key
- [x] Earned titles (14 types) with badges on leaderboard
- [x] On-chain publisher (built, needs RPC debug)
- [x] Pre-commit hooks (biome, cargo fmt, yamllint, markdownlint)
- [x] CI/CD with GitHub Actions (ARM builds, GHCR, SST deploy)
- [x] Cloudflare DNS + AWS CloudFront for frontend
- [x] Demo data with 25 pilots, realistic score distribution
- [x] Event dedup (killmail objects, DB sync, separate deques)
- [x] Background historical loading (API responsive immediately)
- [x] All 50 backend tests + 43 frontend tests passing
- [x] Zero warnings in Rust, zero biome errors in TypeScript

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
- Stillness world package:
  `0x28b497559d65ab320d9da4613bf2498d5946b2c0ae3597ccfda3072ce127448c`
- Killmail registry:
  `0x7fd9a32d0bbe7b1cfbb7140b1dd4312f54897de946c399edb21c3a12e52ce283`
- EVE Frontier event fields use nested format:
  `{"item_id": "123", "tenant": "stillness"}`
- Kill timestamps are in seconds (not ms) — convert on ingest
- Smart Gate jumps only — regular jumps are server-side only
- Gate objects have `metadata.name` but no solar system ID
