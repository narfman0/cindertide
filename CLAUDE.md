# Cindertide — Developer Notes

## Narrative Structure

Faction voice is critical to the game's tone. Every faction has a distinct voice that must be consistent across all dialogue, descriptions, and script text.

### Faction Voices

- **The Combine** (`assets/factions/combine.toml`, `assets/scripts/combine_m*.toml`): Corporate, cold, procedural. Impersonal third person. No exclamation marks. Speaker tag: "Field Comms".
- **The Ironborn** (`assets/factions/ironborn.toml`, `assets/scripts/ironborn_m*.toml`): First-person plural "we." Physical, ownership-based. Direct. Speaker tag: "Foreman".
- **The Architect / Covenant** (`assets/factions/covenant.toml`, `assets/scripts/architect_m*.toml`): Private log entries, academic precision, nineteen years of work, self-correction mid-sentence. Speaker tag: "Architect".
- **The Hollow** (`assets/factions/hollow.toml`): Described from outside. Observer/field-report tone. Things are wrong in specific ways.

### Campaign Divergence

The three campaigns (Combine, Ironborn, Architect) all play the same five world
events but their experiences of each event are allowed to diverge increasingly
through m4, including outright contradiction. The divergence IS the story — do
not flatten it. See the "Campaign divergence principle" table in
`docs/campaign.md` for the per-mission divergence pattern.

### Cinematic Framing

Each faction has a signature `cinematic_framing` preset (in `assets/factions/*.toml`)
used by default whenever a script focuses on one of that faction's units or
buildings. Use the matching preset when authoring cinematics:
- Combine → `over_shoulder` (cold, corporate surveillance POV)
- Ironborn → `low_angle_hero` (subject looms, ownership)
- Covenant → `close_up` (private log, academic precision)
- Hollow → `off_kilter` (asymmetric, wrong in specific ways)

Plus shared presets `isometric` (default gameplay) and `wide_establishing`.

### Authoring Cutscenes

Two paths:
- **TOML directly** in `assets/scripts/<faction>_m<idx>.toml` — see existing
  scripts for shape; `camera_focus { target = { type = ... }, framing = "..." }`,
  `camera_release`, `camera_shake { intensity, duration }`.
- **In-mission editor** (F9 while in a mission) — egui side panel with per-action
  inline editors, replay controls, Save / Save-as-edited buttons. `Save as edited`
  writes `<name>.edited.toml` so hand-authored scripts with comments aren't
  clobbered.

### Key Files

- Faction definitions (units, buildings, descriptions, cinematic_framing): `assets/factions/`
- Mission scripts (dialogue, spawns, objectives, camera): `assets/scripts/`
- Mission maps (briefings, win/loss, pre-placed units): `assets/maps/`
- World/faction lore: `docs/factions.md`, `docs/world.md`
- Campaign structure + canon map (ritual sites, divergence): `docs/campaign.md`
- Script system implementation: `src/mission_script/mod.rs`
- Camera system: `src/camera/mod.rs`
- Cutscene editor: `src/cutscene_editor/mod.rs`
