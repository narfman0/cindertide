// Campaign — territory grid, act progression, mission generation between
// individual missions. Data layer only; no per-frame systems.

use bevy::prelude::*;
use crate::map::Faction;
use crate::mapgen::MissionType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    One,
    Two,
    Three,
}

impl Act {
    pub fn next(&self) -> Option<Act> {
        match self {
            Act::One => Some(Act::Two),
            Act::Two => Some(Act::Three),
            Act::Three => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub id: u32,
    pub name: String,
    pub owner: Option<Faction>,
    pub corruption: f32, // 0.0..1.0
    pub adjacent: Vec<u32>,
}

#[derive(Resource, Debug, Clone)]
pub struct CampaignState {
    pub act: Act,
    pub turn: u32,
    pub zones: Vec<Zone>,
}

#[derive(Debug, Clone)]
pub struct MissionOption {
    pub zone_id: u32,
    pub mission_type: MissionType,
    pub opponent: Faction,
}

// --- Pure functions ---

/// Default 4-zone setup mirroring world.md regions.
pub fn default_state() -> CampaignState {
    CampaignState {
        act: Act::One,
        turn: 0,
        zones: vec![
            Zone { id: 0, name: "The Ironfields".into(), owner: Some(Faction::Combine),  corruption: 0.0, adjacent: vec![1, 2] },
            Zone { id: 1, name: "The Ashline".into(),    owner: None,                    corruption: 0.0, adjacent: vec![0, 2, 3] },
            Zone { id: 2, name: "The Pale Ground".into(),owner: Some(Faction::Hollow),   corruption: 0.5, adjacent: vec![0, 1, 3] },
            Zone { id: 3, name: "The Last Works".into(), owner: Some(Faction::Ironborn), corruption: 0.0, adjacent: vec![1, 2] },
        ],
    }
}

/// After a mission resolves, update territory: winner takes the contested zone
/// (if it wasn't theirs). Loser cedes one adjacent zone if available.
pub fn apply_mission_outcome(
    state: &mut CampaignState,
    zone_id: u32,
    winner: Faction,
    loser: Faction,
) {
    if let Some(z) = state.zones.iter_mut().find(|z| z.id == zone_id) {
        z.owner = Some(winner.clone());
    }
    // Loser cedes one adjacent zone if they own one.
    let cession_target = state
        .zones
        .iter()
        .find(|z| z.owner.as_ref() == Some(&loser))
        .map(|z| z.id);
    if let Some(cid) = cession_target {
        if let Some(z) = state.zones.iter_mut().find(|z| z.id == cid) {
            z.owner = None; // becomes contested
        }
    }
    state.turn += 1;
}

/// Hollow corruption spreads by 0.1 per zone per advancement, capped at 1.0.
/// Act 2+ accelerates: corruption also spreads to adjacent zones at 0.05.
pub fn spread_corruption(state: &mut CampaignState) {
    let act = state.act;
    let snapshot: Vec<(u32, f32, Vec<u32>)> = state
        .zones
        .iter()
        .map(|z| (z.id, z.corruption, z.adjacent.clone()))
        .collect();

    for z in state.zones.iter_mut() {
        if z.corruption > 0.0 {
            z.corruption = (z.corruption + 0.1).min(1.0);
        }
    }

    if !matches!(act, Act::One) {
        for (zid, cur, adj) in &snapshot {
            if *cur >= 0.5 {
                for aid in adj {
                    if let Some(z) = state.zones.iter_mut().find(|z| z.id == *aid) {
                        z.corruption = (z.corruption + 0.05).min(1.0);
                    }
                }
            }
            let _ = zid;
        }
    }
}

/// Generate up to 3 mission options from current campaign state.
/// Picks unowned-or-enemy zones and assigns mission types deterministically.
pub fn generate_mission_options(state: &CampaignState, player: &Faction) -> Vec<MissionOption> {
    let mut opts = Vec::new();
    for z in &state.zones {
        if z.owner.as_ref() != Some(player) {
            // Mission type: pick by zone id parity for deterministic variety.
            let mt = match z.id % 5 {
                0 => MissionType::Assault,
                1 => MissionType::Control,
                2 => MissionType::Defense,
                3 => MissionType::Extraction,
                _ => MissionType::Survival,
            };
            let opponent = z.owner.clone().unwrap_or(Faction::Hollow);
            opts.push(MissionOption {
                zone_id: z.id,
                mission_type: mt,
                opponent,
            });
            if opts.len() >= 3 {
                break;
            }
        }
    }
    opts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_has_four_zones() {
        let s = default_state();
        assert_eq!(s.zones.len(), 4);
        assert_eq!(s.act, Act::One);
        assert_eq!(s.turn, 0);
    }

    #[test]
    fn act_progression() {
        assert_eq!(Act::One.next(), Some(Act::Two));
        assert_eq!(Act::Two.next(), Some(Act::Three));
        assert_eq!(Act::Three.next(), None);
    }

    #[test]
    fn mission_outcome_transfers_zone() {
        let mut s = default_state();
        // Pale Ground (id 2) is Hollow's; player wins it.
        apply_mission_outcome(&mut s, 2, Faction::Combine, Faction::Hollow);
        let z = s.zones.iter().find(|z| z.id == 2).unwrap();
        assert_eq!(z.owner, Some(Faction::Combine));
        assert_eq!(s.turn, 1);
    }

    #[test]
    fn corruption_spreads_in_corrupted_zones() {
        let mut s = default_state();
        let before = s.zones.iter().find(|z| z.id == 2).unwrap().corruption;
        spread_corruption(&mut s);
        let after = s.zones.iter().find(|z| z.id == 2).unwrap().corruption;
        assert!(after > before);
    }

    #[test]
    fn corruption_does_not_spread_to_uncorrupted_in_act_one() {
        let mut s = default_state();
        spread_corruption(&mut s);
        let ironfields = s.zones.iter().find(|z| z.id == 0).unwrap();
        assert_eq!(ironfields.corruption, 0.0);
    }

    #[test]
    fn corruption_jumps_to_adjacent_in_act_two() {
        let mut s = default_state();
        s.act = Act::Two;
        spread_corruption(&mut s);
        // Ashline (1) is adjacent to Pale Ground (2, corrupt 0.5+).
        let ashline = s.zones.iter().find(|z| z.id == 1).unwrap();
        assert!(ashline.corruption > 0.0);
    }

    #[test]
    fn generate_options_skips_player_zones() {
        let s = default_state();
        let opts = generate_mission_options(&s, &Faction::Combine);
        assert!(opts.iter().all(|o| o.zone_id != 0));
    }

    #[test]
    fn generate_options_capped_at_three() {
        let s = default_state();
        let opts = generate_mission_options(&s, &Faction::Combine);
        assert!(opts.len() <= 3);
    }
}
