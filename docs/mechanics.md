# Cindertide — Core Mechanics

---

## Base Building

### Grid System
- Small grid snapping — tight and readable, not sprawling
- Base footprint is constrained by terrain and buildable zones on each map
- Buildings have adjacency bonuses (motor pool next to barracks speeds vehicle/infantry coordination — minor but rewarding)
- Walls and fortifications are placeable on grid edges

### Building Categories
| Category | Examples |
|---|---|
| Economy | Refinery, Scrapyard, Recruitment Office |
| Production | Barracks, Motor Pool, Foundry (late), Airfield (late) |
| Tech | Workshop, Command Bunker, Research Lab |
| Support | Supply Depot (pop cap), Watchtower, Repair Bay |
| Defense | Pillbox, AA Gun, Tank Trap |

### Population Cap
- Hard cap raised by building **Supply Depots**
- Each depot raises cap by a fixed amount
- Cap applies to all unit types — infantry, vehicles, robots share the same pool
- Heroes do not count toward population cap

---

## Resources

### Fuel
- Primary resource
- Gathered from **Refineries** built on fuel nodes (fixed map locations)
- Also found in **contested depots** — capturable points that generate fuel without building
- Powers vehicle production and some tech research

### Scrap
- Secondary resource
- Gathered by **Scrapyard** workers from environmental debris and battlefield salvage
- Destroyed units leave scrap on the field — both yours and enemies
- Used for buildings, infantry, and upgrades
- Rewards aggressive map presence and post-battle cleanup

### Manpower
- Tertiary resource
- Trickles passively from territory held and population structures
- Caps production rate — you can queue units but manpower limits how fast they emerge
- Represents conscription, logistics, crew

---

## Tech Tree

### Tier Structure
Three tiers unlocked by researching a **Command Upgrade** at the Command Bunker. Each tier costs fuel + scrap and takes time.

- **Tier 1** — Basic units, basic structures, early upgrades
- **Tier 2** — Specialized units, vehicle access, mid upgrades
- **Tier 3** — Late units (robots, heavy armor, air), signature upgrades, hero abilities unlocked

### Research Branches
Each tier has 2–3 branching research tracks at the Workshop/Research Lab. Branches are faction-flavored and mutually exclusive within a tier — you commit to a doctrine:

- **Assault Doctrine** — offensive unit buffs, faster production, aggressive hero abilities
- **Fortification Doctrine** — defensive structures, suppression resistance, support hero abilities
- **Salvage Doctrine** — enhanced scrap generation, robot affinity (unlocks earlier in Act 2+), hybrid abilities

Clarity rule: tier advancement is always visible and linear. Branch choices are the depth layer for players who want it.

---

## Combat

### Core Feel
Tanky and positional. Units hold ground. Fights are decided by positioning and timing, not just numbers.

### Cover System
- **Light cover** — sandbags, low walls, vehicles: reduces incoming damage
- **Heavy cover** — buildings, fortified positions: significant damage reduction, garrisonable
- **No cover** — open ground: full damage, suppression builds faster
- Cover state is visible on unit — they crouch, animate differently

### Suppression
- Units under sustained fire accumulate suppression
- Suppressed units: reduced movement, reduced accuracy, cannot charge
- Full suppression: unit pins in place until fire stops or a hero/support unit breaks it
- Heavy weapons (machine guns, mortars) are primary suppressors
- Hero aura can reduce or resist suppression for nearby units

### Flanking
- Units attacked from the side or rear take bonus damage (armor facing)
- Flanking also bypasses some cover bonuses
- AI and players both exploit this — positioning is rewarded, static lines get punished
- Fast units (motorcycles, light vehicles) are primary flankers

### Morale
- Squads have a morale state: **steady / shaken / broken**
- Sustained casualties, suppression, and hero deaths reduce morale
- Broken units rout toward base — they don't die, they retreat
- Hero presence in proximity raises morale passively
- Broken enemies can be chased down or let go — chasing costs time and exposure

---

## Map Control Points

### Point Types
Points are typed — their benefit varies, and some interact differently per faction:

| Point Type | Base Benefit | Notes |
|---|---|---|
| **Strategic** | +Manpower trickle | Universal — all factions benefit equally |
| **Fuel Depot** | +Fuel trickle | Combine gets bonus rate (industrial efficiency) |
| **Scrap Field** | +Scrap trickle | Ironborn (robots) get bonus rate (affinity for salvage) |
| **High Ground** | Unit sight radius bonus | Garrison to activate; sniper/recon units get extended bonus |
| **Ancient Ruins** | Variable — see below | Dangerous, rewarding |

### Ancient Ruin Points
- Uncapturable in the traditional sense — must be *secured* by holding the area
- Generate a slow trickle of a rare fourth resource: **Residue**
- Residue unlocks late-game research into ancient countermeasures (Act 3 relevant)
- Holding ruins attracts ancient faction attention — increased ambient threat near that point
- The Weave faction sometimes *grows* toward ruins — if left uncontested too long the point becomes hostile

### Capturing Points
- Send any unit to a point to begin capture — takes time under fire
- Contested if both sides have units present — neither side captures
- Fortifying a point (build a small garrison structure) speeds future recapture and adds defense
- Points can be destroyed by some abilities — removed from the map temporarily

---

## Production

### Per-Building Queues
- Each production building has its own queue — no global queue
- Multiple buildings of the same type run parallel queues (reward economic investment)
- Queue cap per building: 5 units

### Unit Categories
| Category | Produced At | Role |
|---|---|---|
| Infantry | Barracks | Core fighting, garrisoning, capping points |
| Heavy Weapons | Barracks (T2) | Suppression, anti-armor support |
| Light Vehicles | Motor Pool | Flanking, scouting, fast response |
| Heavy Armor | Motor Pool (T3) | Frontline pushing, absorbing fire |
| Robots | Foundry (T2, Act 2+) | Versatile, expensive, high morale |
| Air Support | Airfield (T3) | Strikes, recon, transport |

### Repair
- Vehicles and robots can be repaired at a **Repair Bay** or by engineer units in field
- Infantry squads reinforce at Barracks or from a hero with logistics ability
- Repair costs scrap

---

## Heroes

See: [heroes.md](heroes.md)
