# Cindertide — Implementation Roadmap

Current status: the game is feature-complete for a playable single-player campaign experience. All core systems are implemented. What remains is polish, content, and multiplayer hardening.

---

## What Is Built

### Engine & Scaffolding
- Bevy 0.15 with `DefaultPlugins`, isometric orthographic 3D camera
- WASD + edge-scroll camera pan; scroll wheel zoom; proper isometric angle
- Feature-gated render module; `MinimalPlugins` path retained for headless tests
- BRP (Bevy Remote Protocol) scaffold on port 15703 for headless test access
- Save/load: in-memory slots, snapshot/restore of factions/units/tiles

### Map & World
- `Tile`, `Zone`, `ControlPoint`, `GridPos` ECS components; full terrain type set
- 8-directional A* pathfinding with diagonal movement and corner-cutting prevention
- Baked NavMesh with flat cost arrays; incremental update when buildings are placed
- Control point capture (contested rules, per-faction trickle bonuses)
- Fog of war with per-unit vision radius from `UnitStats`

### Economy
- Fuel / Scrap / Manpower per faction; trickle rates, caps, spend/refund
- 16 building types with grid-snap placement, construction time, health
- Per-building production queues (cap 5); manpower gates unit emergence
- Population cap: base 20 + 10 per built SupplyDepot; heroes excluded
- RepairBay passive-heals nearby allied vehicles (scrap-gated)

### Units & Combat
- Four unit types: Riflemen, HeavyWeapons, LightVehicle, HeavyArmor
- Health, AttackDamage, range check, cover damage reduction
- Cover (`InCover`), flanking/facing (`Facing`, `AttackAngle`), suppression/morale (`Pinned`, `Routing`, `MoraleState`)
- Heroes: aura suppression-resist, charge meter, Rally/AreaDamage abilities; `HeroDowned` state
- Tech tree: Tier 1/2/3 at Command Bunker; doctrine branch (Assault/Fortification/Salvage) at Tier 2

### AI
- Strategic AI: phase-aware build orders, 90-second attack waves, scouting, defensive intercept, retreat at < 25% HP
- Unit AI: threat response (return fire), routing retreat via pathfinding to HomeBase
- Hero AI: ability fires when charge full and 3+ enemies in range; retreats below 25% HP

### Procedural Map Generation
- Archetype system: S-expression `.lisp` files in `assets/archetypes/`
- No-recompile extensibility: drop a new `.lisp` to add an archetype
- 10 step types: fill, river, bridges, forests, rubble, urban-ruins, ridgelines, islands, spawn-zones, resources
- 6 spawn layouts: corners, sides, triangle, ring, mirror, ffa-center
- Map sizes keyed to mission type: Defense/Survival 80×52, Assault/Control 128×80, Extraction 160×100

### Campaign & Mission Systems
- Linear faction campaign: 5 missions per faction, played in order
- Combine, Ironborn, and Architect campaigns; Architect unlocks after completing both base campaigns
- Campaign progress saved to `~/.cindertide/progress.toml`
- Campaign TOML files in `assets/campaigns/` — no code changes to add a campaign
- 15 hand-authored mission maps in `assets/maps/` with pre-placed units, buildings, and scripted events
- Mission win/loss conditions per type; terminal Won/Lost state

### Mission Script System
- TOML scripts in `assets/scripts/`; loaded by faction + mission name
- Triggers: `time` (elapsed seconds), `beat` (HeroGoesDown, LastStand, AncientUnification)
- Actions: dialogue (bottom bar, 4s display with queue), spawn_units, objective/change_objective, win_mission, lose_mission

### Authored Beats
- `HeroGoesDown`, `LastStand`, `AncientUnification` beat IDs
- State watchers fire beats from ECS observers; beat outcomes consumed by script system

### Hollow Faction
- Corruption zone spawners; spawn rate scales with map corruption coverage
- Act 3 `AncientUnification` beat doubles spawn rate and enables hero priority targeting

### 3D Client (full feature set)
- Full campaign flow: title → campaign picker → briefing → in-mission → debrief → game over
- Standard RTS controls: box/drag select, shift-click toggle, right-click move/attack, A+click attack-move, S stop, H hold, Space pause, Tab cycle units
- Unit facing (smooth rotation), death flash effects, building rubble on destruction
- HUD: resource bar, unit info panel, production queue, minimap, mission objectives
- Unit ability (Q key), tech tree panel (T key)
- Audio event infrastructure ready (no .ogg assets yet)
- LAN multiplayer MVP: host (H) or join (J) from title, TCP state sync at 10 Hz
- `CINDERTIDE_MODEL_PATH` loads GLB models; falls back to colored cubes

### Map Editor
- Access via E (title) or Space→E (in-game pause); Escape returns to paused game
- Tools: paint terrain, place unit, place building, erase, script editor, campaign editor
- G: generate from archetype panel (8 archetypes, adjustable size/seed)
- Ctrl+S save / Ctrl+L load map TOML
- P: playtest current map as live mission

---

## What Remains

Priority order — highest first.

### High Priority

| Item | Notes |
|---|---|
| Real audio assets | Infrastructure exists; needs `.ogg` files in `assets/audio/` |
| Unit animations | Walk and attack cycles; units are currently static cubes/models |
| Balance pass | Unit stats exist but are not tuned for fun |

### Medium Priority

| Item | Notes |
|---|---|
| Multiplayer lobby hardening | TCP MVP works; needs slot-based team assignment and reconnect handling |
| Real 3D model assets | Pipeline exists via `CINDERTIDE_MODEL_PATH`; no models converted yet |
| Architect finale text adaptation | Unlock logic exists; finale text branching on which faction was beaten first is stubbed, not authored |
| Save/load mid-mission resume | In-memory slots work; mid-mission resume not fully tested |

### Lower Priority

| Item | Notes |
|---|---|
| Co-op player controls | Architecture supports it; second-player controls not wired |
| Faction asymmetry depth | Combine fuel-radius supply system and Ironborn Foundry scrap recycling are structural, not yet mechanically distinct at depth |
| Hero recruitment variety | Per-faction acquisition paths; currently Rifleman bundle model for all |
| Additional campaigns | Data-driven; just needs TOML + maps |
| Additional archetypes | Drop a `.lisp` in `assets/archetypes/` |

---

## How to Run

```sh
# Default: 3D client, colored cube placeholder mode
cargo run --bin cindertide

# With GLB models
CINDERTIDE_MODEL_PATH=/path/to/models cargo run --bin cindertide
```

Headless tests use `MinimalPlugins` and remain independent of the render path:

```sh
cargo test
```
