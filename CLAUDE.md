# Cindertide — Developer Notes

## Narrative Structure

Faction voice is critical to the game's tone. Every faction has a distinct voice that must be consistent across all dialogue, descriptions, and script text.

### Faction Voices

- **The Combine** (`assets/factions/combine.toml`, `assets/scripts/combine_m*.toml`): Corporate, cold, procedural. Impersonal third person. No exclamation marks. Speaker tag: "Field Comms".
- **The Ironborn** (`assets/factions/ironborn.toml`, `assets/scripts/ironborn_m*.toml`): First-person plural "we." Physical, ownership-based. Direct. Speaker tag: "Foreman".
- **The Architect / Covenant** (`assets/factions/covenant.toml`, `assets/scripts/architect_m*.toml`): Private log entries, academic precision, nineteen years of work, self-correction mid-sentence. Speaker tag: "Architect".
- **The Hollow** (`assets/factions/hollow.toml`): Described from outside. Observer/field-report tone. Things are wrong in specific ways.

### Key Files

- Faction definitions (units, buildings, descriptions): `assets/factions/`
- Mission scripts (dialogue, spawns, objectives): `assets/scripts/`
- World/faction lore: `docs/factions.md`, `docs/world.md`
- Campaign structure: `docs/campaign.md`
- Script system implementation: `src/mission_script/mod.rs`
