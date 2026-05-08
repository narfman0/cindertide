// Control points — capture progression and trickle bonuses to factions.

use bevy::prelude::*;
use crate::map::{ControlPoint, ControlPointType, Faction, GridPos};
use crate::resources::{ResourceTrickle, FactionEntity};
use crate::units::UnitPos;
use std::collections::HashSet;

pub const DEFAULT_CAPTURE_RATE: f32 = 0.2; // progress per second

// --- Pure functions ---

/// Cell-distance test (Chebyshev) — capture radius works on a square footprint.
pub fn within_radius(a: &GridPos, b: &GridPos, radius: f32) -> bool {
    let dx = (a.x - b.x).abs() as f32;
    let dy = (a.y - b.y).abs() as f32;
    dx.max(dy) <= radius
}

/// Apply one step of capture progression to a control point.
///
/// Rules per `mechanics.md`:
/// - 2+ factions present in radius: contested; no progress change
/// - 0 factions: capture decays; clear contesting once back to 0
/// - 1 faction == current owner: capture decays
/// - 1 faction != current owner: that faction gains capture; if it was a
///   different contester before, progress resets first
/// - Reaching progress = 1.0 transfers ownership and resets progress
pub fn step_capture(cp: &mut ControlPoint, factions_present: &[Faction], dt: f32, rate: f32) {
    let unique: HashSet<&Faction> = factions_present.iter().collect();

    if unique.len() > 1 {
        return; // contested
    }

    if unique.is_empty() {
        cp.capture_progress = (cp.capture_progress - rate * dt).max(0.0);
        if cp.capture_progress == 0.0 {
            cp.contesting = None;
        }
        return;
    }

    let f = (*unique.iter().next().unwrap()).clone();

    if cp.owner.as_ref() == Some(&f) {
        cp.capture_progress = (cp.capture_progress - rate * dt).max(0.0);
        if cp.capture_progress == 0.0 {
            cp.contesting = None;
        }
        return;
    }

    if cp.contesting.as_ref() != Some(&f) {
        cp.contesting = Some(f.clone());
        cp.capture_progress = 0.0;
    }

    cp.capture_progress = (cp.capture_progress + rate * dt).min(1.0);
    if cp.capture_progress >= 1.0 {
        cp.owner = Some(f);
        cp.contesting = None;
        cp.capture_progress = 0.0;
    }
}

/// Trickle contribution from holding a single point of the given type, for
/// the given owner. Per-faction bonuses come from `mechanics.md` (Combine
/// boosts fuel, Ironborn boosts scrap).
pub fn point_trickle_bonus(point_type: &ControlPointType, owner: &Faction) -> (f32, f32, f32) {
    // returns (fuel_per_sec, scrap_per_sec, manpower_per_sec)
    match point_type {
        ControlPointType::Strategic => (0.0, 0.0, 0.5),
        ControlPointType::FuelDepot => {
            let f = if matches!(owner, Faction::Combine) { 7.5 } else { 5.0 };
            (f, 0.0, 0.0)
        }
        ControlPointType::ScrapField => {
            let s = if matches!(owner, Faction::Ironborn) { 7.5 } else { 5.0 };
            (0.0, s, 0.0)
        }
        ControlPointType::HighGround => (0.0, 0.0, 0.0),
        ControlPointType::AncientRuins => (0.0, 0.0, 0.0),
    }
}

// --- Systems ---

pub fn capture_system(
    time: Res<Time>,
    mut points: Query<&mut ControlPoint>,
    units: Query<(&UnitPos, &Faction)>,
) {
    let dt = time.delta_secs();
    for mut cp in &mut points {
        let mut factions: Vec<Faction> = Vec::new();
        for (upos, faction) in &units {
            if within_radius(&upos.pos, &cp.pos, cp.capture_radius) {
                factions.push(faction.clone());
            }
        }
        step_capture(&mut cp, &factions, dt, DEFAULT_CAPTURE_RATE);
    }
}

/// Recomputes ResourceTrickle on each Faction entity from baseline plus
/// owned-point bonuses. Baseline manpower 1.0/s matches FactionBundle.
pub fn recompute_trickle_system(
    points: Query<&ControlPoint>,
    mut factions: Query<(&FactionEntity, &mut ResourceTrickle)>,
) {
    for (fe, mut trickle) in &mut factions {
        let (mut f, mut s, mut m) = (0.0_f32, 0.0_f32, 1.0_f32); // baseline manpower 1.0
        for cp in &points {
            if let Some(owner) = &cp.owner {
                if owner == &fe.faction {
                    let (df, ds, dm) = point_trickle_bonus(&cp.point_type, owner);
                    f += df;
                    s += ds;
                    m += dm;
                }
            }
        }
        trickle.fuel_per_second = f;
        trickle.scrap_per_second = s;
        trickle.manpower_per_second = m;
    }
}

pub struct ControlPlugin;

impl Plugin for ControlPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (capture_system, recompute_trickle_system).chain());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_point(owner: Option<Faction>) -> ControlPoint {
        ControlPoint {
            point_type: ControlPointType::Strategic,
            pos: GridPos { x: 0, y: 0 },
            capture_radius: 2.0,
            owner,
            contesting: None,
            capture_progress: 0.0,
        }
    }

    #[test]
    fn within_radius_chebyshev() {
        let a = GridPos { x: 0, y: 0 };
        assert!(within_radius(&a, &GridPos { x: 2, y: 2 }, 2.0));
        assert!(within_radius(&a, &GridPos { x: 2, y: 0 }, 2.0));
        assert!(!within_radius(&a, &GridPos { x: 3, y: 0 }, 2.0));
        assert!(!within_radius(&a, &GridPos { x: 0, y: 3 }, 2.0));
    }

    #[test]
    fn neutral_point_captured_by_lone_faction() {
        let mut p = make_point(None);
        // 0.2/s rate × 5s = 1.0 → captured
        step_capture(&mut p, &[Faction::Combine], 5.0, DEFAULT_CAPTURE_RATE);
        assert_eq!(p.owner, Some(Faction::Combine));
        assert_eq!(p.contesting, None);
        assert_eq!(p.capture_progress, 0.0);
    }

    #[test]
    fn two_factions_contested_no_progress() {
        let mut p = make_point(None);
        step_capture(&mut p, &[Faction::Combine, Faction::Hollow], 10.0, DEFAULT_CAPTURE_RATE);
        assert_eq!(p.owner, None);
        assert_eq!(p.capture_progress, 0.0);
    }

    #[test]
    fn no_units_decays_progress() {
        let mut p = make_point(None);
        p.contesting = Some(Faction::Combine);
        p.capture_progress = 0.5;
        step_capture(&mut p, &[], 1.0, DEFAULT_CAPTURE_RATE);
        assert!((p.capture_progress - 0.3).abs() < 1e-5);
        assert_eq!(p.contesting, Some(Faction::Combine));
    }

    #[test]
    fn no_units_decay_clears_contester_at_zero() {
        let mut p = make_point(None);
        p.contesting = Some(Faction::Combine);
        p.capture_progress = 0.05;
        step_capture(&mut p, &[], 1.0, DEFAULT_CAPTURE_RATE);
        assert_eq!(p.capture_progress, 0.0);
        assert_eq!(p.contesting, None);
    }

    #[test]
    fn owner_lone_decays_opposing_progress() {
        let mut p = make_point(Some(Faction::Combine));
        p.contesting = Some(Faction::Hollow);
        p.capture_progress = 0.6;
        step_capture(&mut p, &[Faction::Combine], 1.0, DEFAULT_CAPTURE_RATE);
        assert!((p.capture_progress - 0.4).abs() < 1e-5);
    }

    #[test]
    fn switch_contester_resets_progress() {
        let mut p = make_point(Some(Faction::Combine));
        p.contesting = Some(Faction::Hollow);
        p.capture_progress = 0.5;
        step_capture(&mut p, &[Faction::Ironborn], 0.0, DEFAULT_CAPTURE_RATE);
        assert_eq!(p.contesting, Some(Faction::Ironborn));
        assert_eq!(p.capture_progress, 0.0);
    }

    #[test]
    fn ownership_transfers_at_full_progress() {
        let mut p = make_point(Some(Faction::Combine));
        p.contesting = Some(Faction::Hollow);
        p.capture_progress = 0.95;
        step_capture(&mut p, &[Faction::Hollow], 1.0, DEFAULT_CAPTURE_RATE);
        assert_eq!(p.owner, Some(Faction::Hollow));
        assert_eq!(p.contesting, None);
        assert_eq!(p.capture_progress, 0.0);
    }

    #[test]
    fn strategic_point_grants_manpower() {
        let (f, s, m) = point_trickle_bonus(&ControlPointType::Strategic, &Faction::Combine);
        assert_eq!(f, 0.0);
        assert_eq!(s, 0.0);
        assert_eq!(m, 0.5);
    }

    #[test]
    fn fuel_depot_combine_bonus() {
        let (f, _, _) = point_trickle_bonus(&ControlPointType::FuelDepot, &Faction::Combine);
        assert_eq!(f, 7.5);
    }

    #[test]
    fn fuel_depot_non_combine_baseline() {
        let (f, _, _) = point_trickle_bonus(&ControlPointType::FuelDepot, &Faction::Ironborn);
        assert_eq!(f, 5.0);
    }

    #[test]
    fn scrap_field_ironborn_bonus() {
        let (_, s, _) = point_trickle_bonus(&ControlPointType::ScrapField, &Faction::Ironborn);
        assert_eq!(s, 7.5);
    }
}
