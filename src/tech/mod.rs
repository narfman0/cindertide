// Tech tree — tiers 1–3 and doctrine branches per `mechanics.md`.

use bevy::prelude::*;
use crate::resources::{ResourcePool, ResourceCost, can_afford, spend, refund};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    One,
    Two,
    Three,
}

impl Tier {
    pub fn next(&self) -> Option<Tier> {
        match self {
            Tier::One => Some(Tier::Two),
            Tier::Two => Some(Tier::Three),
            Tier::Three => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doctrine {
    Assault,
    Fortification,
    Salvage,
}

#[derive(Component, Debug, Clone)]
pub struct Tech {
    pub tier: Tier,
    pub doctrine: Option<Doctrine>,
}

impl Default for Tech {
    fn default() -> Self {
        Self { tier: Tier::One, doctrine: None }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResearchTarget {
    Tier(Tier),
    Doctrine(Doctrine),
}

#[derive(Component, Debug, Clone)]
pub struct ResearchInProgress {
    pub target: ResearchTarget,
    pub elapsed: f32,
    pub total: f32,
}

// --- Pure tables ---

pub fn tier_cost(t: Tier) -> ResourceCost {
    match t {
        Tier::Two => ResourceCost { fuel: 200.0, scrap: 200.0, manpower: 0.0 },
        Tier::Three => ResourceCost { fuel: 500.0, scrap: 400.0, manpower: 0.0 },
        Tier::One => ResourceCost { fuel: 0.0, scrap: 0.0, manpower: 0.0 },
    }
}

pub fn tier_seconds(t: Tier) -> f32 {
    match t {
        Tier::Two => 60.0,
        Tier::Three => 120.0,
        Tier::One => 0.0,
    }
}

pub fn doctrine_cost(_d: Doctrine) -> ResourceCost {
    ResourceCost { fuel: 100.0, scrap: 150.0, manpower: 0.0 }
}

pub fn doctrine_seconds(_d: Doctrine) -> f32 {
    45.0
}

#[derive(Debug, PartialEq)]
pub enum ResearchError {
    AlreadyAtTier,
    SkipsTier,
    DoctrineAlreadyChosen,
    AlreadyResearching,
    Unaffordable,
}

/// Validate-and-charge for starting research. Mutates pool and returns the
/// ResearchInProgress to insert on the faction. Pure-ish: no ECS access.
pub fn start_research(
    pool: &mut ResourcePool,
    tech: &Tech,
    in_progress: bool,
    target: ResearchTarget,
) -> Result<ResearchInProgress, ResearchError> {
    if in_progress {
        return Err(ResearchError::AlreadyResearching);
    }

    let (cost, total) = match &target {
        ResearchTarget::Tier(t) => {
            // Must advance one tier at a time, in order.
            let next = tech.tier.next().ok_or(ResearchError::AlreadyAtTier)?;
            if next != *t {
                return Err(ResearchError::SkipsTier);
            }
            (tier_cost(*t), tier_seconds(*t))
        }
        ResearchTarget::Doctrine(d) => {
            if tech.doctrine.is_some() {
                return Err(ResearchError::DoctrineAlreadyChosen);
            }
            (doctrine_cost(*d), doctrine_seconds(*d))
        }
    };

    if !can_afford(pool, &cost) {
        return Err(ResearchError::Unaffordable);
    }
    spend(pool, &cost);
    Ok(ResearchInProgress { target, elapsed: 0.0, total })
}

/// Advance an in-progress research by dt. Returns Some(target) if it just
/// completed (the caller should apply it and remove the ResearchInProgress).
pub fn step_research(rp: &mut ResearchInProgress, dt: f32) -> Option<ResearchTarget> {
    rp.elapsed += dt;
    if rp.elapsed >= rp.total {
        Some(rp.target.clone())
    } else {
        None
    }
}

pub fn apply_completed(tech: &mut Tech, target: &ResearchTarget) {
    match target {
        ResearchTarget::Tier(t) => tech.tier = *t,
        ResearchTarget::Doctrine(d) => tech.doctrine = Some(*d),
    }
}

/// Computed stat multipliers from the faction's current tech tier + doctrine.
/// Stored as a component on the faction entity, updated when research completes.
#[derive(Component, Debug, Clone)]
pub struct TechBuff {
    /// Multiplier on unit attack damage (1.0 = base).
    pub damage_mult: f32,
    /// Multiplier on unit max HP at spawn (1.0 = base).
    pub hp_mult: f32,
    /// Multiplier on all resource trickle rates (1.0 = base).
    pub trickle_mult: f32,
    /// Multiplier on unit production costs (1.0 = base, 0.9 = 10% cheaper).
    pub cost_mult: f32,
    /// Multiplier on building construction time (1.0 = base, 0.8 = 20% faster).
    pub build_time_mult: f32,
    /// Multiplier on building max HP (1.0 = base).
    pub building_hp_mult: f32,
}

impl Default for TechBuff {
    fn default() -> Self {
        Self {
            damage_mult: 1.0,
            hp_mult: 1.0,
            trickle_mult: 1.0,
            cost_mult: 1.0,
            build_time_mult: 1.0,
            building_hp_mult: 1.0,
        }
    }
}

/// Recomputes TechBuff from the current Tech state. Pure function.
pub fn compute_tech_buff(tech: &Tech) -> TechBuff {
    let mut b = TechBuff::default();
    match tech.tier {
        Tier::Two   => { b.damage_mult += 0.10; b.hp_mult += 0.10; b.trickle_mult += 0.15; }
        Tier::Three => { b.damage_mult += 0.25; b.hp_mult += 0.25; b.trickle_mult += 0.30; b.cost_mult -= 0.10; }
        Tier::One   => {}
    }
    match tech.doctrine {
        Some(Doctrine::Assault)       => { b.damage_mult += 0.20; }
        Some(Doctrine::Fortification) => { b.building_hp_mult += 0.30; b.build_time_mult -= 0.20; }
        Some(Doctrine::Salvage)       => { b.trickle_mult += 0.30; b.cost_mult -= 0.15; }
        None => {}
    }
    b
}

// --- System ---

pub fn research_system(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Tech, &mut ResearchInProgress)>,
) {
    let dt = time.delta_secs();
    for (entity, mut tech, mut rp) in &mut q {
        if let Some(target) = step_research(&mut rp, dt) {
            apply_completed(&mut tech, &target);
            let buff = compute_tech_buff(&tech);
            commands.entity(entity).remove::<ResearchInProgress>().insert(buff);
        }
    }
}

pub struct TechPlugin;

impl Plugin for TechPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, research_system);
    }
}

// suppress unused warning (refund is for callers wanting to undo)
const _: fn(&mut ResourcePool, &ResourceCost) = refund;

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(f: f32, s: f32) -> ResourcePool {
        ResourcePool { fuel: f, scrap: s, manpower: 0.0 }
    }

    #[test]
    fn tier_advances_correctly() {
        assert_eq!(Tier::One.next(), Some(Tier::Two));
        assert_eq!(Tier::Two.next(), Some(Tier::Three));
        assert_eq!(Tier::Three.next(), None);
    }

    #[test]
    fn start_tier_two_charges_correctly() {
        let mut p = pool(500.0, 500.0);
        let tech = Tech::default();
        let rp = start_research(&mut p, &tech, false, ResearchTarget::Tier(Tier::Two)).unwrap();
        assert_eq!(p.fuel, 300.0);
        assert_eq!(p.scrap, 300.0);
        assert_eq!(rp.total, tier_seconds(Tier::Two));
    }

    #[test]
    fn start_research_rejects_skipping_tier() {
        let mut p = pool(1000.0, 1000.0);
        let tech = Tech::default();
        let r = start_research(&mut p, &tech, false, ResearchTarget::Tier(Tier::Three));
        assert_eq!(r.unwrap_err(), ResearchError::SkipsTier);
        // Pool untouched.
        assert_eq!(p.fuel, 1000.0);
    }

    #[test]
    fn start_research_rejects_already_at_tier3() {
        let mut p = pool(1000.0, 1000.0);
        let tech = Tech { tier: Tier::Three, doctrine: None };
        let r = start_research(&mut p, &tech, false, ResearchTarget::Tier(Tier::Three));
        assert!(r.is_err());
    }

    #[test]
    fn start_research_rejects_concurrent() {
        let mut p = pool(1000.0, 1000.0);
        let tech = Tech::default();
        let r = start_research(&mut p, &tech, true, ResearchTarget::Tier(Tier::Two));
        assert_eq!(r.unwrap_err(), ResearchError::AlreadyResearching);
    }

    #[test]
    fn start_research_rejects_second_doctrine() {
        let mut p = pool(1000.0, 1000.0);
        let tech = Tech { tier: Tier::Two, doctrine: Some(Doctrine::Assault) };
        let r = start_research(&mut p, &tech, false, ResearchTarget::Doctrine(Doctrine::Salvage));
        assert_eq!(r.unwrap_err(), ResearchError::DoctrineAlreadyChosen);
    }

    #[test]
    fn step_research_completes_at_total() {
        let mut rp = ResearchInProgress { target: ResearchTarget::Tier(Tier::Two), elapsed: 0.0, total: 10.0 };
        assert!(step_research(&mut rp, 5.0).is_none());
        assert_eq!(step_research(&mut rp, 5.5), Some(ResearchTarget::Tier(Tier::Two)));
    }

    #[test]
    fn apply_completed_sets_tier() {
        let mut t = Tech::default();
        apply_completed(&mut t, &ResearchTarget::Tier(Tier::Two));
        assert_eq!(t.tier, Tier::Two);
    }

    #[test]
    fn apply_completed_sets_doctrine() {
        let mut t = Tech::default();
        apply_completed(&mut t, &ResearchTarget::Doctrine(Doctrine::Salvage));
        assert_eq!(t.doctrine, Some(Doctrine::Salvage));
    }

    #[test]
    fn unaffordable_research_fails_without_charge() {
        let mut p = pool(0.0, 0.0);
        let tech = Tech::default();
        let r = start_research(&mut p, &tech, false, ResearchTarget::Tier(Tier::Two));
        assert_eq!(r.unwrap_err(), ResearchError::Unaffordable);
    }

    #[test]
    fn tech_buff_default_is_identity() {
        let b = TechBuff::default();
        assert_eq!(b.damage_mult, 1.0);
        assert_eq!(b.hp_mult, 1.0);
        assert_eq!(b.trickle_mult, 1.0);
        assert_eq!(b.cost_mult, 1.0);
        assert_eq!(b.build_time_mult, 1.0);
        assert_eq!(b.building_hp_mult, 1.0);
    }

    #[test]
    fn tech_buff_tier_two_increases_damage_and_hp() {
        let tech = Tech { tier: Tier::Two, doctrine: None };
        let b = compute_tech_buff(&tech);
        assert!((b.damage_mult - 1.10).abs() < 0.001);
        assert!((b.hp_mult - 1.10).abs() < 0.001);
        assert!((b.trickle_mult - 1.15).abs() < 0.001);
    }

    #[test]
    fn tech_buff_tier_three_more_than_tier_two() {
        let t2 = compute_tech_buff(&Tech { tier: Tier::Two, doctrine: None });
        let t3 = compute_tech_buff(&Tech { tier: Tier::Three, doctrine: None });
        assert!(t3.damage_mult > t2.damage_mult);
        assert!(t3.hp_mult > t2.hp_mult);
    }

    #[test]
    fn tech_buff_assault_doctrine_boosts_damage() {
        let tech = Tech { tier: Tier::One, doctrine: Some(Doctrine::Assault) };
        let b = compute_tech_buff(&tech);
        assert!((b.damage_mult - 1.20).abs() < 0.001);
        assert_eq!(b.building_hp_mult, 1.0); // no building bonus
    }

    #[test]
    fn tech_buff_salvage_doctrine_boosts_trickle() {
        let tech = Tech { tier: Tier::One, doctrine: Some(Doctrine::Salvage) };
        let b = compute_tech_buff(&tech);
        assert!((b.trickle_mult - 1.30).abs() < 0.001);
        assert!((b.cost_mult - 0.85).abs() < 0.001);
    }

    #[test]
    fn tech_buff_fortification_doctrine_reduces_build_time() {
        let tech = Tech { tier: Tier::One, doctrine: Some(Doctrine::Fortification) };
        let b = compute_tech_buff(&tech);
        assert!((b.build_time_mult - 0.80).abs() < 0.001);
        assert!((b.building_hp_mult - 1.30).abs() < 0.001);
    }
}
