# Cindertide — Implementation Roadmap

Incremental build order. Every step ships **headless-testable via BRP** —
no rendering required for any step in this roadmap. Each step adds pure
functions (math), ECS components/systems, BRP method(s), unit tests, and at
least one BRP integration test.

Where a step references existing design docs, those are the source of truth
for the *what*; this roadmap defines the *order* and the *acceptance shape*.

---

## Completed

| Step | Title | Notes |
|---|---|---|
| 1 | Bevy + BRP scaffold | port 15703, MinimalPlugins, hot-reload-friendly |
| 2 | Map ECS + terrain | `Tile`, `Zone`, `ControlPoint`, `GridPos`, terrain types |
| 3 | A* pathfinding + unit movement | infantry/vehicle distinction, terrain costs |
| 4 | Basic combat | `Health`, `AttackDamage`, range check, cover damage reduction |
| 5 | Cover on units | `InCover` component, `update_cover_system` |
| 6 | Flanking + facing | `Facing`, `AttackAngle`, cover bypass for rear/flank |
| 7 | Suppression + morale | `Pinned`, `Routing`, `MoraleState` |
| 7.5 | Integration polish | `RiflemanBundle`, `unit/spawn`/`unit/status` BRP, ParamSet fix |
| 8 | Resources | Fuel/Scrap/Manpower per faction; trickle, cap, spend |
| 9 | Control point capture | radius capture, contested rules, Combine/Ironborn trickle bonuses |
| 10 | Buildings + grid placement | 16 building types, costs, construction progress, `dev/reset` |
| 11 | Production queues | Per-building, cap 5, cost up-front |
| 12 | Unit variety | HeavyWeapons / LightVehicle / HeavyArmor bundles |
| 13 | Heroes | aura suppression-resist, charge meter, Rally / AreaDamage abilities |
| 14 | Tech tree | Tier 1/2/3 + doctrine choice, ResearchInProgress |
| 15 | Unit AI | threat response, routing retreat to HomeBase |
| 16 | Repair Bay | passive heal of nearby allied vehicles, scrap-gated |
| 17 | Pop cap | base 20 + 10 per Built SupplyDepot, gates production |
| 18-22 | AI opponent | economic / production / tactical / hero / doctrine systems |
| 23 | Procedural map gen | LCG seed, mission-shaped layout, base/chokepoint coords |
| 24 | Missions + win conditions | per-type rules, terminal Won/Lost |
| 25 | Campaign / living world | 4-zone state, mission outcome → territory shifts |
| 26 | Authored beats | HeroGoesDown, LastStand, AncientUnification |
| 27 | Hollow faction | corruption-zone spawners, Act-3 United Voice rate doubling |
| 28 | Level editor | tile JSON save/load, dimension validation |
| 29 | Save / load | in-memory slots, snapshot/restore of factions/units/tiles |
| 30 | Render scaffolding | feature-gated `render` module; full pipeline pending Synty work |
| 31 | Campaign MVP loop | `GameState` + `CampaignRun`; title menu BRP (`game/new/load/save/exit`); `mission/select` + auto-resolve back to Campaign; victory = all zones, defeat = none |
| 32 | TUI client | `cindertide-play` binary; ratatui TUI with briefing/debrief/finale screens; single-command boot (server thread + TUI on main thread) |
| 33 | Linear faction campaign | Replaced territory system with 5-mission linear structure; Combine/Ironborn/Architect factions; `GlobalProgress` tracking; Architect handler unlock after either base campaign |
| 34 | Narrative content | `assets/narrative.toml` data file with all mission briefings and debriefs; BRP endpoints `narrative/mission`, `narrative/debrief`, `narrative/finale` |

---

## Phase 6 — Playable surface (steps 32–34)

_Steps 32–34 completed. See Completed table above._

---

## Phase 1 — RTS economy loop (steps 8–12)

Goal: a headless match in which two factions can gather, build, and fight.
At the end of phase 1, an automated test can run a small skirmish entirely
through BRP and observe a winner emerge.

### Step 8 — Resources
- `Resources { fuel, scrap, manpower }` per faction (component on a Faction entity)
- Pure functions: `can_afford(cost, pool)`, `spend(cost, pool)`, `refund`
- Trickle rates configurable; manpower caps production rate
- BRP: `resources/status { faction }` → totals + trickle rates

### Step 9 — Control point capture
- Capture progress over time when only one side has units on the point
- Contested → both sides present → no progress either way
- Held point adds a trickle to that faction's resources (wires steps 2 + 8)
- Per-faction bonus rates per `mechanics.md` (Combine fuel, Ironborn scrap)
- BRP: `point/status { entity }` → owner, progress, type

### Step 10 — Buildings & grid placement
- Building components per `mechanics.md` table (Refinery, Barracks, Motor Pool, …)
- Grid-snap placement validation: not on void, not on water, not overlapping
- Construction time + resource cost; building has its own `Health`
- Adjacency bonus hooks (motor pool ↔ barracks) — placeholder, no effect yet
- BRP: `building/place`, `building/status`

### Step 11 — Production queues
- Per-building queues, cap 5
- Manpower trickle gates how fast queued units actually emerge
- Unit spawns at building's exit tile
- BRP: `production/enqueue { building, unit_type }`, `production/queue_status`

### Step 12 — Unit variety
- Heavy weapons (suppression specialists, high `suppression_value`)
- Light vehicles (fast, flank-oriented)
- Heavy armor (T3, frontline)
- Reuse `RiflemanBundle` pattern for each
- Stats per `units.md` (currently a stub — populate as part of this step if needed)

---

## Phase 2 — Combat depth (steps 13–17)

### Step 13 — Heroes
- `Hero` marker, `SignatureAbility { charge, max, ability_kind }`, `Aura { radius, effects }`
- Aura applies passive modifiers to nearby allied units (e.g. suppression resist)
- Charge meter accumulates over time and from unit kills nearby
- "Goes down" state — `HeroDowned` instead of `Dead`; recoverable
- BRP: `hero/ability_use { entity }`, `hero/status { entity }`

### Step 14 — Tech tree
- Tier 1/2/3 progression at Command Bunker (resource cost + research time)
- Doctrine branch chosen at tier 2 (Assault / Fortification / Salvage), mutually exclusive
- Gates unit and building availability per `mechanics.md`
- BRP: `tech/research { branch }`, `tech/status { faction }`

### Step 15 — Unit AI (autonomous behavior)
- Threat response: return fire when attacked and no active order
- Cover seek: under fire with no order → move to nearest cover within short range
- Morale rout: broken units already gain `Routing` (step 7) — this step makes them actually move toward their base
- Attack-move targeting: nearest enemy by default, hero target overrides

### Step 16 — Repair & reinforcement
- `RepairBay` building heals nearby vehicles/robots over time (scrap cost)
- Engineer unit can field-repair (scrap cost, manual order)
- Barracks reinforce squads back to full size
- BRP: `repair/start { entity }`, `reinforce/start { squad }`

### Step 17 — Population cap
- Supply Depot raises cap by a fixed amount (per `mechanics.md`)
- Cap shared across infantry, vehicles, robots
- Heroes excluded
- Production blocks (queue freezes, doesn't reject) when cap reached

---

## Phase 3 — AI opponent (steps 18–22)

Per `agents.md`. AI plays by player rules — no stat inflation, only smarter
decisions and faster reactions.

### Step 18 — Economic AI
- Prioritizes nearby fuel/scrap nodes early
- Expands toward control points: fuel depot → strategic → high ground
- Builds Supply Depots in step with production
- Salvages battlefield scrap when safe

### Step 19 — Production AI
- Maintains unit-type ratios per chosen doctrine
- Reacts to opponent composition: heavy armor seen → anti-tank; heavy infantry → suppression
- Never idles a production building if resources allow

### Step 20 — Tactical AI
- 3 groups: **attack**, **defend**, **harass**
- Attack pushes opponent's weakest front; defend holds base + key points; harass targets resource nodes
- Rebalance every 60 sim seconds
- Mortar/MG suppress before infantry advances; flank with light vehicles

### Step 21 — Hero AI
- Hero attached to attack group
- Ability fires when charge full and 3+ enemies in range
- Retreats to Repair Bay below 25% HP

### Step 22 — Doctrine consistency
- Doctrine chosen at match start, never switched
- Tier advancement at fixed resource thresholds
- Research priority order within doctrine is static

---

## Phase 4 — Campaign & procedural (steps 23–27)

### Step 23 — Procedural map generation
- 3–5 zones per map per `world.md`
- Authored chokepoints (always 1–2) — flagged for AI
- Mission-type shapes the layout: linear (assault) / open (control) / radial (defense) / asymmetric (extraction) / compact (survival)
- BRP: `map/generate { seed, mission_type }` returns map summary

### Step 24 — Mission types & win conditions
- Per-type win/lose triggers wired to ECS observers
- Defeat = base destroyed (assault), points lost (control), waves survived (survival), etc.
- BRP: `mission/status` returns objective progress

### Step 25 — Living world
- Campaign-level territory grid; faction borders shift on outcomes
- Hollow corruption spreads one zone per mission in Act 2+
- Mission options generated from current territory state (2–3 per turn)

### Step 26 — Authored beat triggers
- State watchers per the table in `agents.md` (robot save, betrayal, etc.)
- Beat outcomes flex based on win/loss path per `campaign.md`
- BRP: `campaign/state` returns acts, flags, fired beats

### Step 27 — Hollow faction
- Corruption zones spawn Hollow units on a timer
- Spawn rate scales with map corruption coverage
- Behavioral modes match zone type: consuming / subsuming / indifferent
- Act 3: United Voice — all modes simultaneous, doubled spawn rate, hero priority targeting

---

## Phase 5 — Tools & presentation (steps 28–30)

### Step 28 — Level editor (data-driven)
- Map data as JSON or RON; no code in maps (per `world.md`)
- BRP methods: `editor/set_tile`, `editor/place_object`, `editor/save_map`, `editor/load_map`
- Validation rules from `world.md`: 3–6 zones, 1–2 chokepoints, defensible bases

### Step 29 — Save / load
- Serialize world state (entities, components, queued production, campaign state)
- Resume mid-mission and mid-campaign
- BRP: `save/write { slot }`, `save/read { slot }`

### Step 30 — Rendering pass
- Add a non-headless run mode using `DefaultPlugins`
- Tests stay on `MinimalPlugins`
- Synty assets per `world.md`
- This is the *only* step that requires non-headless work

---

## Phase 7 — Player agency (steps 35–37)

### Step 35 — In-mission player controls
- Unit selection, move/attack orders, and building construction via TUI
- Player can issue commands to their faction; AI continues to drive the opponent

### Step 36 — Faction asymmetry
- Combine fuel-radius supply system
- Ironborn scrap recycling via Foundry
- Covenant morale aura damage modifier

### Step 37 — Hero recruitment and abilities
- Per-faction acquisition paths
- Signature abilities per hero

---

## How to use this roadmap

- One step per commit; commit message format `step N: short title`.
- Each step adds: pure functions + tests, ECS components/systems, BRP method(s), and at least one BRP integration test in `tests/`.
- If a step grows past ~500 lines diff, split it.
- If a step needs a foundation that hasn't been built, build the foundation first as its own step — don't inline it.
- Update this file when a step lands (move it from "next" into "completed").
