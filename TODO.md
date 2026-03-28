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

- [ ] **Auto-update WORLD_PACKAGE_ID** — query `evefrontier/world-contracts`
      GitHub releases or chain state at deploy time to get the latest
      Stillness world package ID. Currently hardcoded in `sst.config.ts`.
- [ ] **Frontend error logging (Sentry)** — catch unresolved character
      names, failed API calls, and other edge cases. Would surface issues
      where the GraphQL resolver consistently fails for certain characters.
- [ ] **Investigate SST Cloudflare StaticSite bug** — `Could not resolve "sst"`
      in worker.ts:480 during `Runtime.Build`. Affects all Cloudflare Worker-based
      components on SST 4.5.12. Static env vars don't help — bug is in the
      component itself. File issue on github.com/sst/sst if not already reported.
      Currently using `sst.aws.StaticSite` with `sst.cloudflare.dns()` as workaround.
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
