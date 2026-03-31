# Deepsurge Project Page — Design Spec

**Date:** 2026-03-31
**Status:** Approved

---

## Context

SENTINEL is submitted to the EVE Frontier x Sui Hackathon 2026 via Deepsurge.
The project page requires a logo, a description, and a demo video (deferred).

Judging is split: 75% from a panel of three judges, 25% from public player votes.
Judge criteria include: Concept & Feasibility, Mod Design, Concept Implementation,
Player Utility, EVE Frontier Relevance & Vibe, Creativity & Originality,
UX & Usability, Visual Presentation & Demo.

---

## Color Palette

**White + Deep Violet** (F1). Deliberately contrasts with the all-black/hacker
aesthetic of competing entries.

| Role            | Value     |
| --------------- | --------- |
| Background      | `#ffffff` |
| Brand dark      | `#2d0a6e` |
| Brand primary   | `#5b21b6` |
| Brand mid       | `#7c3aed` |
| Brand light     | `#a78bfa` |
| Surface tint    | `#faf9ff` |
| Border          | `#ede8ff` |
| Danger/CRITICAL | `#dc2626` |

This palette applies to both the Deepsurge page assets and the live dashboard.

---

## Typography

| Role | Font | Weights |
| ---- | ---- | ------- |
| Logo wordmark, UI headings | **Space Grotesk** | 700, 900 |
| Body text, data labels, secondary UI | **Inter** | 400, 500, 600 |

Both available on Google Fonts.

**Space Grotesk** was chosen for its geometric, slightly technical character that
complements the H3-A shield icon. It holds up at large display sizes with wide
letter-spacing. **Inter** is the standard for high-legibility product UIs —
excellent at small sizes for stats, labels, and event feed text.

**SVG logo note:** The branding SVGs specify `font-family="Space Grotesk, system-ui, sans-serif"`.
When opened directly in a browser they will use Space Grotesk if the font is
installed or loaded. When used as an `<img>` tag or exported to PNG, embed the
font or convert text to paths first.

---

## Logo

**Style:** H3-A — pure outline corp insignia. Angular rune inside a shield
silhouette. Geometric, EVE faction-emblem energy.

**Files:**

- `branding/sentinel-icon.svg` — icon only (400×420, scalable)
- `branding/sentinel-lockup.svg` — horizontal lockup with wordmark + tagline

**Usage:**

- Deepsurge project thumbnail: use `sentinel-icon.svg`
- Any wide-format context: use `sentinel-lockup.svg`
- Export to PNG at 512×512 for platforms that don't accept SVG

**Do not** use filled or dark-background variants for the Deepsurge page.
The outline-on-white version is the canonical logo.

---

## Description

In the Frontier, information is the deadliest weapon. SENTINEL is a decentralized threat intelligence network that watches the chain so you don't have to.

Every kill, bounty, and Smart Gate transit is ingested in real time via Sui's gRPC checkpoint stream and fed into a threat scoring engine. Each pilot's score — factoring recency, kill count, K/D ratio, bounties, and movement patterns — is published on-chain every 30 seconds. Smart Gates read those scores and block high-threat pilots before they reach you.

Fleet commanders get a live leaderboard of the most dangerous pilots in the network. Corp members get Discord alerts the moment a **CRITICAL**-tier threat is detected. The dashboard shows earned titles (**Apex Predator**, **Bounty Hunter**), system hotspots, and a streaming event feed updated as each checkpoint lands.

**Tech stack:** Rust/Tokio backend, Solid.js dashboard, Move smart contracts on Sui, deployed on AWS. The intelligence network is live on Stillness.

**Tone:** EVE-flavored lore hook, then concrete system description, then
tech credibility signal. Hits: Player Utility, Mod Design, EVE Relevance,
Technical Implementation, Creativity.

**If character-limited:** cut the final tech paragraph — the first three
paragraphs stand alone.

---

## Dashboard UX (related change)

The live event feed was briefly the default landing view. Decision: revert
to the main dashboard (stats bar + threat leaderboard) as the first view.

**Rationale:** judges and players need to understand threat scores within
3 seconds of landing. The leaderboard with CRITICAL/HIGH/MODERATE/LOW tiers
communicates the concept immediately. The event feed works as a secondary
tab once the user understands what drives the scores.

See TODO for the implementation task.

---

## Demo Video (deferred)

To be recorded after remaining changes are complete. The first 5 seconds
must show the threat leaderboard — not the event feed. Key beats to cover:

1. Live leaderboard with threat tiers and earned titles
2. Real-time event feed updating as checkpoints land
3. Discord bot slash commands and CRITICAL alert
4. On-chain registry / publisher
5. Smart Gate blocking a CRITICAL-tier pilot (if achievable)

---

## TODOs surfaced during this session

Added to `TODO.md`:

- **Restore dashboard as default view** (High priority)
- **Get live on Stillness with real bounties** (Critical — required for
  "Best Live Frontier Integration" bonus and Stillness Deployment bonus)
