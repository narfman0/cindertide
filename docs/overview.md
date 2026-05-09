# Cindertide — Game Design Overview

## Elevator Pitch
A dieselpunk RTS with three asymmetric factions, a five-mission linear campaign playable from each faction's perspective, procedurally generated or hand-authored maps, and a full 3D isometric Bevy 0.15 client with standard RTS controls.

---

## Genre & Inspirations
- **Genre:** Real-time strategy, single-player / local cooperative
- **Inspirations:** Dawn of War (strategic points, army identity), Age of Empires (gathering economy), Supreme Commander (salvage loop), Bleach (hero visual language, spiritual pressure)

---

## Setting
A dieselpunk world — 1940s-adjacent industrial aesthetics, proto-mechs, tesla weapons, oil and steel and smoke. Two factions fight over resources and territory. A third party, The Architect (The Covenant), has been engineering the conflict as a ritual to awaken something ancient beneath the ruins. The robots awaken mid-campaign.

---

## Aesthetic
- Base palette: desaturated — rust, soot, olive, concrete
- Heroes: oversized silhouettes, signature colors
- Ancient faction units: wrong colors — bioluminescent, too vivid
- Tone: serious world, costly battles, cinematic hero moments
- Placeholder mode (default): colored cuboids. Production mode: GLB models via `CINDERTIDE_MODEL_PATH`

---

## Core Pillars
1. **Familiar RTS loop** — base building, resource gathering, tech trees, unit production
2. **Hero identity** — persistent heroes with aura effects and signature abilities
3. **Data-driven campaign** — authored narrative arc delivered through TOML campaigns, maps, and scripts
4. **Faction asymmetry** — Combine and Ironborn have different building sets and resource emphases
5. **Open tooling** — map editor, archetype generator, and script editor are in-game

---

## Resources
- **Fuel** — primary resource; gathered from Refineries and contested depots
- **Scrap** — salvaged from environment and destroyed units; Ironborn-favored
- **Manpower** — trickles from held territory; limits replacement rate and gates production

---

## Factions

### Combine
Corporate military. Heavy on paperwork and precision. Primary resource emphasis: Fuel. Buildings: CommandBunker, Barracks, Refinery, MotorPool, Workshop, ResearchLab, SupplyDepot, Watchtower, RepairBay, Pillbox, AAGun, TankTrap.

### Ironborn
Independent salvagers-turned-soldiers. Stubborn, pragmatic. Primary resource emphasis: Scrap. Buildings include Foundry and Scrapyard in addition to the standard set.

### The Architect (Covenant / Handler)
Not a conventional faction. Only playable in the third campaign. Plays as the hidden hand that engineered the war — a ritual-builder, not a soldier.

---

## Heroes
- Recruited like units, persist across campaign missions
- Passive aura modifies nearby allied units (suppression resistance)
- One signature ability (Q key) — charge-based, not spammable
- Heroes go **down**, not dead — incapacitated state, recoverable
- Death flash visual; rubble replaces destroyed buildings

---

## Unit Types
| Type | Role |
|---|---|
| Riflemen | Standard infantry |
| HeavyWeapons | Suppression specialists, anti-armor |
| LightVehicle | Fast flankers |
| HeavyArmor | Frontline T3 armor |

---

## Combat Model
- Range-checked attacks with cover damage reduction
- Flanking and facing — rear/flank attacks bypass cover
- Suppression and morale: `Suppressed`, `Routing`, `MoraleState` components
- 8-directional A* pathfinding with diagonal movement and corner-cutting prevention
- Baked NavMesh with flat cost arrays for fast lookups; incrementally updated when buildings are placed

---

## Tech Tree
- Tier 1 / 2 / 3 progression at Command Bunker
- Doctrine branch (Assault / Fortification / Salvage) chosen at Tier 2 — mutually exclusive
- Gates unit and building availability

---

## Production
- Per-building queues, cap 5, cost paid up-front
- Manpower gates emergence rate
- Population cap: base 20 + 10 per built SupplyDepot; heroes excluded

---

## AI
- **Strategic AI:** phase-aware build orders (early/mid/late game), 90-second attack waves, scouting, defensive intercept, retreat at < 25% HP
- **Unit AI:** threat response (return fire), routing retreat via pathfinding
- AI plays by the same rules as the player — no stat inflation

---

## Map Generation

### Archetype System (S-expression)
Eight archetypes in `assets/archetypes/*.lisp`:

| Archetype | Description |
|---|---|
| choke_point | Single-corridor engagement |
| river_crossing | Winding river, bridge choke |
| urban_ruin | Dense rubble and cover |
| open_steppe | Flat terrain, vehicle-favored |
| industrial_complex | Industrial cluster map |
| highland_passes | Ridgeline terrain |
| island_chain | Segmented land masses with road bridges |
| fortress_valley | Defensive position emphasis |

Drop a new `.lisp` file into `assets/archetypes/` to add an archetype — no recompile required.

**Step types:** `fill`, `river`, `bridges`, `forests`, `rubble`, `urban-ruins`, `ridgelines`, `islands`, `spawn-zones`, `resources`

**Spawn layouts:** `corners`, `sides`, `triangle`, `ring`, `mirror`, `ffa-center`

### Procedural Map Sizes
| Mission type | Dimensions |
|---|---|
| Defense / Survival | 80 × 52 |
| Assault / Control | 128 × 80 |
| Extraction | 160 × 100 |

### Hand-authored Maps
15 pre-built TOML maps in `assets/maps/` (5 per faction), each with pre-placed units, buildings, briefing text, win/loss text, and scripted events.

---

## Mission Script System
- TOML scripts in `assets/scripts/` loaded by name (e.g. `combine_m0.toml`)
- **Triggers:** `time` (elapsed seconds), `beat` (HeroGoesDown, LastStand, AncientUnification)
- **Actions:** `dialogue` (bottom bar, 4s display), `spawn_units`, `objective` / `change_objective`, `win_mission`, `lose_mission`
- Dialogue queues one message at a time with automatic advancement

---

## Campaign System
- Campaigns defined as TOML files in `assets/campaigns/` — no code changes to add a new campaign
- Three built-in campaigns: Combine, Ironborn, Architect
- Campaign progress saved to `~/.cindertide/progress.toml`
- Architect unlocks after beating both Combine and Ironborn

---

## 3D Client

### Controls
| Action | Input |
|---|---|
| Camera pan | WASD or edge scroll |
| Zoom | Mouse wheel |
| Box select | Left drag |
| Shift-click toggle | Shift + left click |
| Move / attack | Right click |
| Attack-move | A + left click |
| Stop | S |
| Hold | H |
| Pause | Space |
| Cycle units | Tab |
| Unit ability | Q |
| Tech tree | T |
| Editor (title) | E |
| Editor (in-game) | Space → E (from pause) |
| Multiplayer | M (from title) |

### HUD
- Resource bar (Fuel / Scrap / Manpower per faction)
- Unit info panel: health, range, speed (from UnitStats)
- Production queue panel
- Minimap (refreshes on a timer)
- Mission objectives (driven by script system)

### Fog of War
- Per-unit vision radius from `UnitStats`
- Tiles outside any allied unit's vision are darkened

### Visual Effects
- Unit facing: smooth rotation toward movement target or attack target
- Death flash on unit death
- Building rubble spawned on destruction

### LAN Multiplayer (MVP)
- M from title → H to host, J to join
- TCP state sync at 10 Hz
- Net ID counter for entity reconciliation
- Full lobby / team-slot assignment not yet implemented

---

## Map Editor
Accessed via E from the title screen, or Space → E from the in-game pause menu. Escape returns to the paused game if entered mid-mission.

| Key | Tool |
|---|---|
| 1 | Paint terrain |
| 2 | Place unit |
| 3 | Place building |
| 4 | Erase |
| 5 | Script editor |
| 6 | Campaign editor |
| G | Generate from archetype (panel with 8 archetypes, adjustable size/seed) |
| Ctrl+S | Save map TOML |
| Ctrl+L | Load map TOML |
| P | Playtest current map as live mission |

---

## 3D Model Assets

The client runs in **placeholder mode** by default — all units and buildings appear as colored cuboids. This requires no setup and is fully playable.

### Enabling GLB Models

Set the `CINDERTIDE_MODEL_PATH` environment variable to a directory containing `.glb` model files:

```sh
CINDERTIDE_MODEL_PATH=/path/to/models cargo run --bin cindertide
```

The game logs either `3D models loaded from: <path>` or `CINDERTIDE_MODEL_PATH not set — using placeholder geometry` on startup.

### File Naming Convention

| Unit type | File |
|---|---|
| Riflemen | `unit_riflemen.glb` |
| HeavyWeapons | `unit_heavy_weapons.glb` |
| LightVehicle | `unit_light_vehicle.glb` |
| HeavyArmor | `unit_heavy_armor.glb` |

| Building type | File |
|---|---|
| CommandBunker | `building_command_bunker.glb` |
| Barracks | `building_barracks.glb` |
| Refinery | `building_refinery.glb` |
| Scrapyard | `building_scrapyard.glb` |
| RecruitmentOffice | `building_recruitment_office.glb` |
| MotorPool | `building_motor_pool.glb` |
| Foundry | `building_foundry.glb` |
| Airfield | `building_airfield.glb` |
| Workshop | `building_workshop.glb` |
| ResearchLab | `building_research_lab.glb` |
| SupplyDepot | `building_supply_depot.glb` |
| Watchtower | `building_watchtower.glb` |
| RepairBay | `building_repair_bay.glb` |
| Pillbox | `building_pillbox.glb` |
| AAGun | `building_aa_gun.glb` |
| TankTrap | `building_tank_trap.glb` |

Missing files fall back to placeholder cuboids silently. The `assets/models/` directory is gitignored.
