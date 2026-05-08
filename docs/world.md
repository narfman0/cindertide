# Cindertide — World & Map Design

---

## Map Scale

Medium maps — large enough for multiple fronts and flanking routes, small enough that every area feels purposeful. Target: 3–5 minutes to cross on foot, 2–4 distinct zones per map.

**Synty asset packs** are the visual foundation. Maps are assembled from modular Synty pieces — buildings, roads, terrain tiles, props. This keeps art consistent and makes the editor practical.

---

## Terrain Types

### Built Environment (primary)
- Bombed city blocks — rubble providing cover, garrisonable ruins, choke-point streets
- Industrial zones — refineries, factories, rail yards, fuel depots
- Fortified lines — trenches, bunkers, wire, pillboxes
- Occupied towns — civilian structures, narrow lanes, elevated positions

### Natural Terrain (secondary)
- Rivers and bridges — natural choke points, bridges destroyable
- Forest edges — light cover, concealment, limits vehicle movement
- Hills and ridgelines — high ground sight bonuses, elevation-based cover
- Mud flats — slows vehicles, infantry unaffected

### Ancient/Hollow Zones (Act 2–3)
- Corrupted terrain — wrong geometry, desaturation bleeds into surroundings
- Hollow ruins — ancient structures that predate everything else on the map
- Void patches — areas of absolute darkness, units entering lose sight radius temporarily
- Subsumed growth — organic overgrowth that functions as heavy cover but damages non-Hollow units over time

---

## Map Structure

### Zones
Each map is divided into 3–5 named zones. Each zone has:
- A visual identity (industrial, urban, natural, corrupted)
- 1–3 control points
- Defined cover density (sparse / moderate / dense)
- Vehicle accessibility (some zones are infantry-only due to terrain)

### Control Points
Typed points distributed across zones — see mechanics.md. Point type fits zone identity:
- Industrial zones: fuel depots
- Strategic positions: high ground, crossroads
- Ancient ruins: residue points (appear Act 2+)

### Chokepoints
Every map has 1–2 defined chokepoints — bridges, mountain passes, blown-out underpasses — where battles concentrate naturally. These are authored even on procedural maps.

---

## Procedural Map Generation

Maps are procedurally generated per campaign run with authored constraints:

### What is procedural:
- Zone layout and arrangement
- Control point placement within zones
- Resource node locations
- Decorative prop placement
- Hollow corruption spread (which zones show ancient influence)

### What is authored (always present):
- Number of zones (3–5 based on mission type)
- Chokepoint count and type (always 1–2)
- Starting base locations (always separated, always defensible)
- Mission-specific objective placement (the thing you're attacking/defending)
- Terrain type distribution (mission brief tells you what to expect)

### Mission Types that Shape Generation:
| Mission Type | Map Shape |
|---|---|
| Assault | Linear — attacker pushes toward defender's base |
| Control | Open — multiple contested points across center |
| Defense | Radial — attacker approaches from multiple directions |
| Extraction | Asymmetric — small team navigates toward objective |
| Survival | Compact — hold a small area against escalating waves |

---

## Level Editor

Shipped with the game. Players and modders can create, edit, and share custom maps.

### Capabilities:
- Place and paint Synty modular terrain tiles
- Place buildings, props, cover objects, destructibles
- Define zones with names and visual identity tags
- Place and type control points
- Set starting base locations and mission type
- Define chokepoints (flagged for AI pathfinding priority)
- Place authored objectives (capture point, destroy target, escort path)
- Paint Hollow corruption zones
- Set ambient audio zones
- Playtest from editor without full build

### Distribution:
- Editor ships as part of the base game install — no separate download
- Maps saved as data files (not code) — shareable as single files
- Steam Workshop support planned for post-launch
- Campaign slot: players can substitute custom maps into a procedural campaign run at mission select

### Editor Constraints (keep it simple):
- No scripting in V1 — objectives and win conditions set via dropdown, not code
- Terrain height is fixed per tile (no freeform sculpting) — keeps Synty assets aligned
- Map size bounded (min 3 zones, max 6 zones) — prevents untestable massive maps

---

## World Geography (Campaign Layer)

The campaign map is a stylized top-down view of the conflict region — not a realistic globe, more like a war room map with pins and overlays.

### Regions:
- **The Ironfields** — Combine heartland, heavy industry, Act 1 primary theater
- **The Ashline** — contested border zone, mixed terrain, Act 1–2 primary theater
- **The Pale Ground** — ancient ruins region, Hollow corruption spreading from here, Act 2–3 primary theater
- **The Last Works** — Ironborn sanctuary, industrial ruins they've claimed, Act 3 staging ground

### Living World:
Between missions, faction territories shift on the campaign map based on mission outcomes and time passage. Players see the map change — regions darken with Hollow corruption, faction borders redraw, the Pale Ground expands in Act 3 regardless of player action.
