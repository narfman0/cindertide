# Cindertide — AI Agents

All agent behavior is headless-testable via BRP. No rendering required for any system described here.

---

## Unit AI

Units are not individually intelligent — they execute orders issued by their controlling agent (player or AI opponent). Unit-level decision making is limited to:

- **Threat response:** If attacked and no order is active, unit returns fire on attacker
- **Cover seeking:** When taking fire without an explicit move order, unit moves to nearest available cover within short range
- **Morale breaking:** When morale reaches broken state, unit routes toward friendly base automatically — see [mechanics.md](mechanics.md)
- **Attack priority:** When given an attack-move order, unit targets nearest enemy by default; hero presence overrides to hero's current target

Units do not pathfind around enemies independently — that is the player/AI opponent's responsibility.

---

## AI Opponent

The AI opponent plays by the same rules as the player — same resources, same build costs, same unit caps. No cheating. Difficulty is expressed through decision quality and reaction speed, not stat inflation.

### Economic AI
- Prioritizes Scrap and Fuel nodes near starting base in early game
- Expands toward control points in priority order: fuel depots → strategic points → high ground
- Builds Supply Depots to raise pop cap in step with production — see [mechanics.md](mechanics.md)
- Salvages scrap from battlefield dead when safe to do so

### Production AI
- Maintains a ratio of unit types based on doctrine — see [mechanics.md](mechanics.md) doctrine branches
- Responds to player unit composition: heavy player armor → prioritizes anti-tank; heavy player infantry → prioritizes suppression units
- Queues production continuously — never idles a production building if resources allow

### Tech AI
- Advances tiers at fixed resource thresholds
- Chooses doctrine branch at campaign start and stays consistent — does not mix doctrines
- Researches upgrades in fixed priority order within chosen doctrine

### Tactical AI
- Divides forces into 3 groups: **attack**, **defend**, **harass**
- Attack group pushes toward player's weakest front (fewest units detected)
- Defend group holds base and key control points
- Harass group targets player resource nodes and isolated units
- Groups rebalance based on game state every 60 seconds
- Uses suppression before advancing — mortar team fires, infantry follows
- Attempts flanking on heavy armor: sends light vehicles around while infantry engages frontally

### Hero AI
- AI heroes stay attached to attack group
- Hero ability fires when charge meter is full and 3+ enemy units are in range — no hoarding
- Hero retreats toward Repair Bay when below 25% HP

### Hollow AI
- Does not build or gather resources — spawns from corruption zones on a timer
- Spawn rate increases as Hollow corruption spreads on the map — see [world.md](world.md)
- Targets player units over buildings; targets heroes preferentially when in range
- Behavioral mode matches zone type: consuming mode in open ground, subsuming mode near ruins, indifferent mode unpredictable
- United Voice (Act 3): all modes active simultaneously, spawn rate doubled, heroes targeted with priority

---

## Campaign AI (Living World)

Between missions the campaign map updates based on outcomes — see [campaign.md](campaign.md).

### Territory Logic
- Factions that won their last engagement expand into adjacent unclaimed zones
- Factions that lost contract — cede one zone toward their base
- Hollow corruption spreads one zone per mission regardless of player action in Act 2+; accelerates in Act 3
- Player-held zones do not change without a mission being fought over them

### Mission Generation
- Available missions are generated from current territory state
- 2–3 options presented each turn; each has a mission type, terrain type, and reward profile — see [world.md](world.md)
- If the player ignores a zone for 2+ missions, an event fires (enemy entrenches, corruption spreads, ally is besieged)

### Authored Beat Triggers
Authored beats monitor game state and fire when conditions are met — see [campaign.md](campaign.md):

| Beat | Trigger Condition |
|---|---|
| Robot save | Riflemen squad at 1 HP, Prototype Automaton in range with charge available |
| Robot betrayal | Act 2 begins, Ironborn faction flag set, fired on first Combine/Covenant mission |
| Robot understanding | Player completes capture-robot mission objective |
| Ancient unification | Act 3 begins, all three Hollow modes have appeared at least once |
| Hero goes down | Hero HP reaches 0 for first time |
| Last stand | Player base structure below 30% HP, reinforcements scheduled within 90 seconds |

Beats adapt to game state — see [campaign.md](campaign.md) for how outcomes flex based on win/loss.

---

## Pathfinding

- Grid-based A* on the map tile graph
- Terrain cost per tile type: road < grass < forest < rubble < mud
- Vehicles cannot enter forest or rubble tiles
- Destroyed bridges remove tile connections — pathfinding reroutes
- Hollow void patches add high cost (units avoid unless ordered directly)
- Chokepoints flagged in map data — AI prioritizes controlling them

---

## Testing Expectations

Every agent system is testable headless via BRP:
- Spawn units, issue orders, assert positions and states after N ticks
- Spawn AI opponent, assert it builds and expands within expected turn counts
- Trigger authored beats manually, assert correct state changes fire
- Run full mission to win/lose condition, assert campaign state updates correctly

See build order in [agents.md](agents.md) step 15 (Basic AI) through step 22 (Full AI doctrine) for incremental implementation order.
