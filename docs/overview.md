# Cindertide — Game Design Overview

## Elevator Pitch
A dieselpunk RTS with Bleach-inflected hero aesthetics, a procedural campaign with authored emotional beats, and a three-act story about robots earning their humanity while an ancient evil wakes beneath the world.

---

## Genre & Inspirations
- **Genre:** Real-time strategy, single-player / cooperative
- **Inspirations:** Dawn of War (strategic points, army identity), Age of Empires (gathering economy), Supreme Commander (salvage loop), Bleach (hero visual language, spiritual pressure)

---

## Setting
A dieselpunk world — 1940s-adjacent industrial aesthetics, proto-mechs, tesla weapons, oil and steel and smoke. One faction discovers how to create autonomous robot soldiers. The robots awaken. Meanwhile, something ancient stirs beneath the ruins of a civilization older than memory.

---

## Aesthetic
- Base palette: desaturated — rust, soot, olive, concrete
- Heroes: oversized silhouettes, signature colors that bleed into the world during abilities
- Abilities use ink-like motion blur and speed lines, not particle effects
- Ancient faction units: wrong colors — bioluminescent, too vivid, geometry that doesn't sit right
- Tone: serious world, absurd spectacle — battles feel costly, hero finishers are cinematic

---

## Core Pillars
1. **Familiar RTS loop** — base building, resource gathering, tech trees, unit production
2. **Hero identity** — persistent, visually dominant heroes with aura effects and signature abilities
3. **Procedural campaign** — authored emotional arc, procedurally determined specifics
4. **Living world** — factions shift between missions based on player actions
5. **Co-op friendly** — shared or split economy, heroes give each player a personal identity

---

## Resources
- **Fuel** — primary resource, gathered from refineries and contested depots
- **Scrap** — salvaged from environment and destroyed units (yours and theirs)
- **Manpower** — trickles from held territory, limits replacement rate

---

## Heroes
- Recruited like units, persist across campaign missions
- Visual dominance — identifiable instantly in a crowd, Bleach-loud signature look
- Passive aura modifies nearby units (e.g. brawler hero suppresses morale penalties)
- One signature ability — builds toward, not spammed; visually enormous
- Heroes go down, not die — incapacitated for mission or campaign stretch
- Accumulate visual scars and minor permanent traits from survival
- In co-op: each player owns a hero as their personal identity in the army

---

## Campaign Structure
Two factions are playable at the start (Combine and Ironborn), each with 5 linear missions that tell the same 5 world events from opposite sides. Completing either campaign unlocks The Architect — a third faction whose 5-mission campaign reveals the war was engineered as a blood ritual to awaken ancient horrors. Mission outcomes are recorded; finale text adapts based on which faction was beaten first and the player's win/loss history.

See: [campaign.md](campaign.md)

---

## Factions
See: [factions.md](factions.md)

---

## 3D Model Assets

The client runs in **placeholder mode** by default — all units and buildings appear as colored cuboids. This requires no setup and is fully playable.

### Enabling GLB Models

Set the `CINDERTIDE_MODEL_PATH` environment variable to a directory containing `.glb` model files before launching the client:

```sh
CINDERTIDE_MODEL_PATH=/path/to/models cargo run --bin cindertide
```

The game logs either `3D models loaded from: <path>` or `CINDERTIDE_MODEL_PATH not set — using placeholder geometry` on startup.

### File Naming Convention

Each unit and building type maps to a specific filename. Place the corresponding `.glb` files in the model directory:

| Unit type | File |
|-----------|------|
| Riflemen | `unit_riflemen.glb` |
| HeavyWeapons | `unit_heavy_weapons.glb` |
| LightVehicle | `unit_light_vehicle.glb` |
| HeavyArmor | `unit_heavy_armor.glb` |

| Building type | File |
|---------------|------|
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

Missing files fall back to the placeholder cuboid silently — you can supply models incrementally without breaking anything.

The `assets/models/` directory is gitignored so large binary asset packs are never committed.
