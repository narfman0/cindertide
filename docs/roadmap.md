# Cindertide — Implementation Roadmap

Current status: the game is feature-complete for a playable single-player campaign experience. All core systems are implemented. What remains is polish, content, and multiplayer hardening.

---

## What Is Built

### Engine & Scaffolding
- Bevy 0.16 with `DefaultPlugins`, isometric orthographic 3D camera
- `bevy_web_asset` registered before `DefaultPlugins` for `http://` asset URLs
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
- Actions: dialogue (bottom bar, 4s display with queue), spawn_units, objective/change_objective, win_mission, lose_mission, **camera_focus** (target home_base / position / unit + optional framing preset), **camera_release**, **camera_shake** (intensity + duration)

### Cinematic Camera System
- `FramingPreset` library with 6 named presets: isometric, low_angle_hero, close_up, over_shoulder, off_kilter, wide_establishing
- Per-faction defaults in `assets/factions/*.toml` (`cinematic_framing` field) — Combine over_shoulder, Ironborn low_angle_hero, Covenant close_up, Hollow off_kilter
- Per-unit/building override via `cinematic_framing` field on UnitDef / BuildingDef
- Resolver chain when script omits framing: explicit → CinematicFraming component → def field → faction default → "isometric"
- `CameraTarget` resource (Free / LookAt / Follow) tweens camera translation + orthographic scale exponentially
- Snap to player home base on mission start; user pan (WASD or edge-scroll) cancels scripted focus
- `attach_cinematic_framing` observer auto-inserts CinematicFraming component on unit/building spawn

### In-Mission Cutscene Editor
- F9 toggles right-side egui panel while in-mission
- Replay controls: Restart (clears Mission.elapsed + ScriptState.fired), Pause/Play (Time<Virtual>::pause), Speed (0.25x–4x via Time<Virtual>::relative_speed)
- Event list with fired/selected markers; per-event Jump-to-event resets elapsed and the fired set
- Inline editors per Action variant (dialogue speaker + multiline, spawn_units faction/type/count/coords, camera_focus target+framing dropdowns, camera_shake sliders)
- Add/remove/reorder events and actions via `+ Event` / `+ Add action` / `^` / `v` / `DEL` buttons
- Save / Save-as-edited / Reload — `Save` writes back via `toml::to_string_pretty`; `Save as edited` writes `<name>.edited.toml` to preserve hand-authored comments
- Kenney Mini Square Mono + Future Narrow fonts loaded into egui at startup from the prefetched asset cache

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
- Audio: 7 `AudioEvent` variants wired to kenney_aio OGGs via `process_audio_events`
- LAN multiplayer MVP: host (H) or join (J) from title, TCP state sync at 10 Hz
- 3D models: per-faction Synty GLBs (`model_file` in faction TOMLs), streamed from
  `http://srv:49200/assets` by default; falls back to colored cubes if asset source unreachable

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
| Unit animations | Walk and attack cycles; units are currently static cubes/models |
| Balance pass | Unit stats exist but are not tuned for fun |
| Expand audio coverage | Tier-1 events wired; per-faction unit acknowledgement voices + music loops still unmapped |

### Medium Priority

| Item | Notes |
|---|---|
| Multiplayer lobby hardening | TCP MVP works; needs slot-based team assignment and reconnect handling |
| HTTP asset cache on disk | `bevy_web_asset` fetches on every cold start; future ETag-aware on-disk cache to mitigate |
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
# Default: 3D client. Streams GLBs/OGGs from http://srv:49200/assets
cargo run --bin cindertide

# Point at a different HTTP asset server or a local mirror directory:
CINDERTIDE_ASSET_BASE=http://my-server:8080/assets cargo run --bin cindertide
CINDERTIDE_ASSET_BASE=/path/to/local/assets cargo run --bin cindertide

# Legacy: filesystem-only override (kept for offline dev)
CINDERTIDE_MODEL_PATH=/path/to/converted cargo run --bin cindertide
```

Asset resolution order: `CINDERTIDE_ASSET_BASE` → `CINDERTIDE_MODEL_PATH` →
`http://srv:49200/assets` (default). If none reach a valid source, the client
falls back to colored cuboids + silent audio.

Headless tests use `MinimalPlugins` and remain independent of the render path:

```sh
cargo test
```
