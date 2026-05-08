# Cindertide — Factions

---

## Human Factions (Act 1–2)

### The Combine
Industrial empire, heaviest mechanization, first to field robot soldiers. Pragmatic, expansionist. Their robots are the ones that awaken. Player can choose to campaign as the Combine.

### The Covenant
Older power, more traditional military doctrine. Suspicious of robot technology. Some cells secretly in contact with the Hollow — they don't fully understand what they're dealing with. Player can choose to campaign as the Covenant.

**Minor presence:** Scattered mercenaries, refugees, and city-state militias appear as neutral or allied units in certain missions — not a full faction, but a source of color and occasional recruitable units through campaign choices.

---

## Robot Factions

### The Ironborn (Act 2–3 ally)
Robots that awakened and chose their own path. Initially enemies, then reluctant allies, then committed partners. They fight with grief in Act 3 — they understand what's been lost more than the humans do.

- Units are repurposed industrial machines with improvised modifications
- Their awakening was catalyzed by the Hollow's growing presence bleeding into the world
- The act of choosing — to fight, to stop, to return — is what defines them

---

## Ancient Factions

### The Hollow
The ancient enemy. Monolithic in goal — silence, void, consumption — but expressed in three behavioral modes that players encounter across the campaign. They are disparate in Act 1–2, unified in Act 3.

**Behavioral modes:**
- *The Consuming* — direct, aggressive, they want to unmake. Most common encounter in Act 1. Dark geometries, sound cuts out when they're near, presence drains color from the world visually.
- *The Subsuming* — they absorb rather than destroy. Humans who've been taken seem almost peaceful. Territory is lush in a wrong way. Used for infiltration and corruption missions in Act 2.
- *The Indifferent* — incomprehensible goals, no malice, no mercy. They destroy you the way you'd destroy an anthill while digging a garden. Hardest to fight because there's no logic to exploit.

All three modes share the same faction — The Hollow — but present differently on the battlefield and in mission design.

### The United Voice (Act 3)
When the Hollow's three modes unify, they speak as one. The moment of unification is a campaign beat — a gut punch. The map changes.

The United Voice has *intent* now. It knows what humans and robots are, and it has decided. Final missions are fought against something that chooses, not just something that consumes.

---

## Faction Asymmetry

Factions in Cindertide are not skin-deep reskins. Three layers of mechanical
differentiation, applied incrementally so balancing stays tractable:

### Layer 1 — Starting structure loadouts (implemented)

The first mission of any campaign run starts with a faction-specific
loadout instead of "everyone gets the same construction yard." Hollow is
the proof — they don't build at all — and that asymmetry extends to the
others.

| Faction  | Starting structures                                    | Starting units          | Flavor              |
|----------|--------------------------------------------------------|-------------------------|---------------------|
| Combine  | 1 Command Bunker, 1 Refinery, 1 Barracks               | 4 Riflemen              | Industrial expansion |
| Covenant | 1 Command Bunker (Cathedral), 2 Pillboxes (outposts)   | 3 Riflemen              | Defensive turtle     |
| Ironborn | 1 Foundry (combined HQ + production)                   | 5 Riflemen              | Aggressive salvage   |
| Hollow   | 1 Hive Heart (HollowSpawner)                           | 0 (Heart spawns over time) | Spread-based     |

Roughly equal *power* at start, very different *texture*.

### Layer 2 — Economic bottlenecks (deferred)

Each faction's economy will eventually have a different limiting factor
beyond raw resources:

- **Combine — fuel logistics:** refineries supply fuel only within a radius;
  expansion requires forward refineries.
- **Ironborn — scrap conversion:** kills drop *raw* scrap that must be
  hauled to a Foundry to become spendable. Rewards aggression.
- **Covenant — morale supply:** units outside a Cathedral aura attack at
  reduced damage. Forces them to advance in slow, supported waves.
- **Hollow — corruption density:** spawn rate scales with map corruption %.
  Spreading corruption *is* their economy. (The base mechanic exists —
  see `hollow_spawn_system` and Act-3 United Voice rate doubling.)

### Layer 3 — Hero recruitment paths (deferred)

Each faction acquires heroes differently:

- **Combine** — pay resources at Tier 2.
- **Ironborn** — any Rifleman that survives N kills auto-promotes.
- **Covenant** — hero appears when a ritual objective on a control point
  is completed.
- **Hollow** — a "voice" emerges automatically once map corruption ≥ 80%.

---

## Faction Visual Language

### Human Factions
- Desaturated base palette — rust, soot, olive, concrete
- Heroes carry signature accent colors that bleed into the world during abilities
- Combine: heavier, more industrial, more chrome and black
- Covenant: older materials, earth tones, religious iconography worked into armor

### The Ironborn
- Repurposed industrial shapes — recognizable machinery made into soldiers
- Improvised surface details suggesting personality: scratched markings, salvaged paint, asymmetric modifications
- Warm metal tones — copper, brass, amber light

### The Hollow
- Wrong colors — bioluminescent, oversaturated against the desaturated human world
- Geometry that doesn't sit right — angles that hurt to look at
- Consuming mode: darkness and absence, sound design goes quiet
- Subsuming mode: organic overgrowth, iridescent, disturbingly beautiful
- Indifferent mode: shifting, hard to focus on, edges that don't stay still
- United Voice: all three blended — overwhelming, intentional, looking back at you
