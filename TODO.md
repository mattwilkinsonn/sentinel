# SENTINEL — Hackathon TODO

Deadline: March 31, 2026 23:59 UTC

## Critical (must do)

- [x] ~~Fix SST deploy~~ — NEON_ORG_ID, historyRetention,
      Cloudflare DNS, ARM containers on GHCR
- [x] ~~Complete on-chain publisher~~ — sig format fixed, shared
      object version fixed, InsufficientGas fixed (0.5 SUI budget)
- [x] ~~Verify live events flowing~~ — gRPC stream connected,
      Stillness world package ID confirmed
- [ ] **Debug 8 unresolved character names** — metadata resolver
      queries last 50 Character objects but these chars are older,
      not in that window. May need to query by specific item_ids
      via CharacterCreatedEvent or paginate more aggressively.
      Some may be structure entities misidentified as characters.
- [x] ~~Verify publisher succeeds~~ — working. First restart after
      adding published_score column re-publishes all (one-time).
- [x] ~~Published score persistence~~ — published 78 profiles across
      InsufficientGas, now increased budget + smaller batches.
      Need to confirm `Published X threat scores — tx: <digest>`.
- [ ] **Deploy to production** — using `sst.aws.StaticSite` with
      `sst.cloudflare.dns()`. ALB health check configured for
      `/api/health`. Public chain IDs in `CHAIN_IDS` config.
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
- [x] ~~Character name DB caching~~ — 4800+ names cached in Postgres,
      loaded instantly on restart
- [x] ~~Live name resolution~~ — metadata resolver fetches names
      for new characters via GraphQL every 10s
- [ ] **Migrate publisher to gRPC** — switch from JSON-RPC to gRPC
      `TransactionExecutionService.ExecuteTransaction` for on-chain
      publishing. Narrative: started with JSON-RPC for MVP, migrated
      to gRPC for performance. Helps "Best Technical" category.
- [ ] **Add Claude development section to README** — document AI-assisted
      development process with example prompts showing human direction
      of architecture, design decisions, and technical corrections.
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

- [ ] **Publisher gas monitoring** — check SUI balance before
      publishing, warn when below threshold. Auto-request from
      faucet or alert to manually top up.
- [ ] **Auto-update WORLD_PACKAGE_ID** — query chain state or
      `evefrontier/world-contracts` releases at deploy time.
      Currently hardcoded in `CHAIN_IDS` in `sst.config.ts`.
- [ ] **Frontend error logging (Sentry)** — catch unresolved
      character names, failed API calls, edge cases.
- [ ] **Investigate SST Cloudflare StaticSite bug** — `Could not
      resolve "sst"` in worker.ts:480. Affects all CF Worker
      components on SST 4.5.12. File issue on sst/sst.
      Using `sst.aws.StaticSite` + `sst.cloudflare.dns()` workaround.
- [ ] **AI narrative feed** — Claude API story-style event
      descriptions. Template fallback if unavailable.
- [ ] **Earned titles expansion** — pattern-based titles:
      "Gate Camper", "Fleet Commander", "Night Stalker".
- [ ] **Alt detection** — behavioral fingerprint comparison.
- [ ] **Webhook/alert system** — Discord webhooks for kills,
      bounties, gate blocks.
- [ ] **Migrate to SolidStart** — SSR, file-based routing.
- [ ] **Distinguish turret kills from PvP** — if `killer_id` is not
      in Character objects, it's a turret/structure. Use `reported_by`
      field to attribute kills to structure owners.
- [ ] **StatusChangedEvent / GateCreatedEvent** — track assembly
      online/offline and new gate deployments.

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
- Publisher wallet: ~0.89 SUI testnet balance
- Shared objects need `initial_shared_version`, not current version
