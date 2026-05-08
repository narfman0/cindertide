// Combat module — damage, targeting, and resolution systems.

use bevy::prelude::*;
use crate::map::{CoverDensity, GridPos, Tile, damage_reduction};
use crate::units::{UnitPos, MoveTarget};

#[derive(Component, Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackRange {
    pub tiles: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackDamage {
    pub base: f32,
    pub suppression_value: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackSpeed {
    pub attacks_per_second: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackCooldown {
    pub remaining: f32,
}

#[derive(Component, Debug, Clone)]
pub struct AttackTarget {
    pub entity: Entity,
}

#[derive(Component, Debug)]
pub struct Dead;

#[derive(Component, Debug, Clone)]
pub struct InCover {
    pub density: CoverDensity,
}

#[derive(Component, Debug, Clone)]
pub struct Suppression {
    pub current: f32,
    pub max: f32,
}

#[derive(Component, Debug, Clone)]
pub struct SuppressionDecay {
    pub per_second: f32,
}

#[derive(Component, Debug, Clone)]
pub struct SuppressionContribution {
    pub per_attack: f32,
}

#[derive(Component, Debug)] pub struct Pinned;
#[derive(Component, Debug)] pub struct Routing;
#[derive(Component)] pub struct TookDamageThisFrame;

#[derive(Component, Debug, Clone, PartialEq)]
pub enum MoraleState { Steady, Shaken, Broken }

#[derive(Component, Debug, Clone)]
pub struct Morale {
    pub current: f32,
    pub max: f32,
}

/// Facing direction of a unit on the grid.
#[derive(Component, Debug, Clone, PartialEq)]
pub enum Facing {
    North,
    South,
    East,
    West,
}

/// The angle of an incoming attack relative to target's facing.
#[derive(Debug, Clone, PartialEq)]
pub enum AttackAngle {
    Front,
    Flank,
    Rear,
}

// --- Pure functions ---

pub fn chebyshev_distance(a: &GridPos, b: &GridPos) -> f32 {
    let dx = (a.x - b.x).abs();
    let dy = (a.y - b.y).abs();
    dx.max(dy) as f32
}

pub fn is_in_range(attacker_pos: &GridPos, target_pos: &GridPos, range: f32) -> bool {
    chebyshev_distance(attacker_pos, target_pos) <= range
}

/// Apply damage to health. Returns true if the unit died (current <= 0).
pub fn apply_damage(health: &mut Health, damage: f32) -> bool {
    health.current -= damage;
    if health.current < 0.0 {
        health.current = 0.0;
    }
    health.current <= 0.0
}

/// Compute final damage after cover reduction. mechanics.md: light=25%, heavy=50%.
pub fn damage_after_cover(base_damage: f32, cover: &CoverDensity) -> f32 {
    base_damage * (1.0 - damage_reduction(cover))
}

/// Determine attack angle based on attacker position, target position, and target facing.
/// dx/dy is direction from target to attacker.
pub fn attack_angle(attacker_pos: &GridPos, target_pos: &GridPos, target_facing: &Facing) -> AttackAngle {
    let dx = attacker_pos.x - target_pos.x;
    let dy = attacker_pos.y - target_pos.y;
    let adx = dx.abs();
    let ady = dy.abs();

    match target_facing {
        Facing::North => {
            if adx > ady { AttackAngle::Flank }
            else if dy < 0 { AttackAngle::Front }
            else { AttackAngle::Rear }
        }
        Facing::South => {
            if adx > ady { AttackAngle::Flank }
            else if dy > 0 { AttackAngle::Front }
            else { AttackAngle::Rear }
        }
        Facing::East => {
            if ady > adx { AttackAngle::Flank }
            else if dx > 0 { AttackAngle::Front }
            else { AttackAngle::Rear }
        }
        Facing::West => {
            if ady > adx { AttackAngle::Flank }
            else if dx < 0 { AttackAngle::Front }
            else { AttackAngle::Rear }
        }
    }
}

/// Damage multiplier for attack angle: Front=1.0, Flank=1.35, Rear=1.75
pub fn angle_damage_multiplier(angle: &AttackAngle) -> f32 {
    match angle {
        AttackAngle::Front => 1.0,
        AttackAngle::Flank => 1.35,
        AttackAngle::Rear => 1.75,
    }
}

/// Cover effectiveness is reduced when flanked/rear:
/// Flank gives 50% of normal cover reduction, Rear gives 0%.
pub fn effective_cover_reduction(cover: &CoverDensity, angle: &AttackAngle) -> f32 {
    let base = damage_reduction(cover);
    match angle {
        AttackAngle::Front => base,
        AttackAngle::Flank => base * 0.5,
        AttackAngle::Rear => 0.0,
    }
}

/// Compute effective damage taking InCover component and attack angle into account.
/// Apply angle multiplier first, then cover reduction (cover less effective when flanked/rear).
pub fn effective_damage(base: f32, cover: Option<&InCover>, angle: Option<AttackAngle>) -> f32 {
    let multiplier = angle.as_ref().map(angle_damage_multiplier).unwrap_or(1.0);
    let after_angle = base * multiplier;
    match cover {
        Some(c) => {
            let reduction = match &angle {
                Some(a) => effective_cover_reduction(&c.density, a),
                None => damage_reduction(&c.density),
            };
            after_angle * (1.0 - reduction)
        }
        None => after_angle,
    }
}

pub fn add_suppression(s: &mut Suppression, amount: f32) {
    s.current = (s.current + amount).clamp(0.0, s.max);
}

pub fn decay_suppression(s: &mut Suppression, dt: f32, per_second: f32) {
    s.current = (s.current - dt * per_second).max(0.0);
}

pub fn is_fully_suppressed(s: &Suppression) -> bool {
    s.current >= s.max
}

/// Output multiplier for a suppressed attacker (mechanics.md: reduced
/// accuracy → reduced expected damage). Full suppression halves output.
pub fn suppression_output_multiplier(s: &Suppression) -> f32 {
    let frac = (s.current / s.max).clamp(0.0, 1.0);
    1.0 - frac * 0.5
}

/// Movement multiplier for a suppressed unit. Full suppression halves
/// movement; the Pinned marker (added separately) blocks movement entirely.
pub fn suppression_movement_multiplier(s: &Suppression) -> f32 {
    let frac = (s.current / s.max).clamp(0.0, 1.0);
    1.0 - frac * 0.5
}

/// Cover reduces incoming suppression by the same fraction it reduces damage.
pub fn suppression_after_cover(amount: f32, cover: Option<&InCover>) -> f32 {
    match cover {
        Some(c) => amount * (1.0 - damage_reduction(&c.density)),
        None => amount,
    }
}

/// Movement penalty from suppression: 0.0 (none) to 0.8 (80% slower).
pub fn suppression_movement_penalty(suppression: f32, max: f32) -> f32 {
    let frac = (suppression / max).clamp(0.0, 1.0);
    frac * 0.8
}

/// Accuracy penalty from suppression: 0.0 to 0.5 (50% reduction).
pub fn suppression_accuracy_penalty(suppression: f32, max: f32) -> f32 {
    let frac = (suppression / max).clamp(0.0, 1.0);
    frac * 0.5
}

/// Derive morale state from Morale component.
/// > 66% → Steady, 33–66% → Shaken, < 33% → Broken.
pub fn morale_state(morale: &Morale) -> MoraleState {
    let frac = morale.current / morale.max;
    if frac > 0.66 {
        MoraleState::Steady
    } else if frac > 0.33 {
        MoraleState::Shaken
    } else {
        MoraleState::Broken
    }
}

/// Suppression gained per hit, reduced by cover.
/// None=full, Light=75%, Heavy=50%.
pub fn suppression_gain_per_hit(weapon_suppression: f32, cover: Option<&InCover>) -> f32 {
    match cover {
        None => weapon_suppression,
        Some(c) => match c.density {
            CoverDensity::None => weapon_suppression,
            CoverDensity::Light => weapon_suppression * 0.75,
            CoverDensity::Heavy => weapon_suppression * 0.50,
        },
    }
}

// --- Systems ---

pub fn cooldown_system(time: Res<Time>, mut query: Query<&mut AttackCooldown>) {
    for mut cooldown in &mut query {
        cooldown.remaining -= time.delta_secs();
        if cooldown.remaining < 0.0 {
            cooldown.remaining = 0.0;
        }
    }
}

struct PendingAttack {
    attacker: Entity,
    target: Entity,
    base_damage: f32,
    suppression_value: f32,
    attacker_pos: GridPos,
    range: f32,
    attacker_mult: f32,
    attacks_per_second: f32,
}

pub fn attack_system(
    mut commands: Commands,
    mut queries: ParamSet<(
        Query<(
            Entity,
            &AttackTarget,
            &AttackDamage,
            &AttackCooldown,
            &UnitPos,
            &AttackSpeed,
            &AttackRange,
            Option<&Suppression>,
        )>,
        Query<&mut AttackCooldown>,
        Query<(&mut Health, &UnitPos, Option<&InCover>, Option<&mut Suppression>, Option<&Facing>), Without<Dead>>,
    )>,
) {
    // Phase 1: read attackers, collect actions; no mutation yet so the
    // attacker-side Suppression read does not conflict with the target-side
    // mut borrow in p2.
    let mut pending: Vec<PendingAttack> = Vec::new();
    for (entity, at, dmg, cd, pos, spd, range, supp) in queries.p0().iter() {
        if cd.remaining > 0.0 {
            continue;
        }
        let mult = supp.map(suppression_output_multiplier).unwrap_or(1.0);
        pending.push(PendingAttack {
            attacker: entity,
            target: at.entity,
            base_damage: dmg.base,
            suppression_value: dmg.suppression_value,
            attacker_pos: pos.pos.clone(),
            range: range.tiles,
            attacker_mult: mult,
            attacks_per_second: spd.attacks_per_second,
        });
    }

    // Phase 2: apply effects to targets; record successful hits and stale targets.
    let mut hits: Vec<(Entity, f32)> = Vec::new();
    let mut to_remove_target: Vec<Entity> = Vec::new();
    let mut took_damage: Vec<Entity> = Vec::new();
    {
        let mut targets = queries.p2();
        for p in &pending {
            match targets.get_mut(p.target) {
                Ok((mut health, tpos, cover, tsupp, tfacing)) => {
                    if !is_in_range(&p.attacker_pos, &tpos.pos, p.range) {
                        continue;
                    }
                    let angle = tfacing.map(|f| attack_angle(&p.attacker_pos, &tpos.pos, f));
                    let dmg = effective_damage(p.base_damage, cover, angle) * p.attacker_mult;
                    let died = apply_damage(&mut health, dmg);
                    if dmg > 0.0 {
                        took_damage.push(p.target);
                    }
                    let supp_gain = suppression_gain_per_hit(p.suppression_value, cover) * p.attacker_mult;
                    if let Some(mut ts) = tsupp {
                        add_suppression(&mut ts, supp_gain);
                    }
                    hits.push((p.attacker, p.attacks_per_second));
                    if died {
                        to_remove_target.push(p.attacker);
                    }
                }
                Err(_) => {
                    to_remove_target.push(p.attacker);
                }
            }
        }
    }

    // Phase 3: reset cooldowns on attackers whose attack landed.
    {
        let mut cooldowns = queries.p1();
        for (entity, aps) in hits {
            if let Ok(mut cd) = cooldowns.get_mut(entity) {
                cd.remaining = 1.0 / aps;
            }
        }
    }

    for entity in took_damage {
        if let Ok(mut e) = commands.get_entity(entity) {
            e.insert(TookDamageThisFrame);
        }
    }
    for entity in to_remove_target {
        if let Ok(mut e) = commands.get_entity(entity) {
            e.remove::<AttackTarget>();
        }
    }
}

pub fn suppression_decay_system(
    time: Res<Time>,
    mut query: Query<(&mut Suppression, &SuppressionDecay)>,
) {
    let dt = time.delta_secs();
    for (mut supp, decay) in &mut query {
        decay_suppression(&mut supp, dt, decay.per_second);
    }
}

pub fn suppression_pin_system(
    mut commands: Commands,
    query: Query<(Entity, &Suppression, Option<&Pinned>)>,
) {
    for (entity, supp, pinned) in &query {
        let should_pin = is_fully_suppressed(supp);
        match (should_pin, pinned) {
            (true, None) => {
                commands.entity(entity).insert(Pinned);
            }
            (false, Some(_)) => {
                commands.entity(entity).remove::<Pinned>();
            }
            _ => {}
        }
    }
}

pub fn death_system(
    mut commands: Commands,
    query: Query<(Entity, &Health), Without<Dead>>,
) {
    for (entity, health) in &query {
        if health.current <= 0.0 {
            commands.entity(entity).insert(Dead).remove::<AttackTarget>();
        }
    }
}

/// Update the InCover component on each unit based on the tile they occupy.
pub fn update_cover_system(
    mut commands: Commands,
    units: Query<(Entity, &UnitPos)>,
    tiles: Query<&Tile>,
) {
    // Build a lookup map from GridPos -> CoverDensity
    let cover_map: std::collections::HashMap<&GridPos, &CoverDensity> = tiles
        .iter()
        .map(|t| (&t.pos, &t.cover))
        .collect();

    for (entity, unit_pos) in &units {
        match cover_map.get(&unit_pos.pos) {
            Some(CoverDensity::Light) => {
                commands.entity(entity).insert(InCover { density: CoverDensity::Light });
            }
            Some(CoverDensity::Heavy) => {
                commands.entity(entity).insert(InCover { density: CoverDensity::Heavy });
            }
            _ => {
                commands.entity(entity).remove::<InCover>();
            }
        }
    }
}

/// Update facing direction based on MoveTarget. Units with a MoveTarget face toward
/// their destination. Units without a MoveTarget keep their current facing (defaulting to North).
pub fn update_facing_system(
    mut commands: Commands,
    mut units: Query<(Entity, &UnitPos, Option<&MoveTarget>, Option<&mut Facing>)>,
) {
    for (entity, unit_pos, move_target, facing) in &mut units {
        if let Some(target) = move_target {
            let dx = target.target.x - unit_pos.pos.x;
            let dy = target.target.y - unit_pos.pos.y;
            let new_facing = if dx.abs() >= dy.abs() {
                if dx >= 0 { Facing::East } else { Facing::West }
            } else {
                if dy < 0 { Facing::North } else { Facing::South }
            };
            if let Some(mut f) = facing {
                *f = new_facing;
            } else {
                commands.entity(entity).insert(new_facing);
            }
        } else if facing.is_none() {
            commands.entity(entity).insert(Facing::North);
        }
    }
}

/// Each frame: for units that took damage (TookDamageThisFrame), add suppression.
/// For units not under fire, decay suppression. Pin/unpin based on full suppression.
pub fn suppression_system(
    mut commands: Commands,
    time: Res<Time>,
    mut query: Query<(Entity, &mut Suppression, Option<&TookDamageThisFrame>, Option<&Pinned>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut supp, took_damage, pinned) in &mut query {
        if took_damage.is_some() {
            // add_suppression already called at attack time; just ensure pin check happens
        } else {
            // No hit this frame: decay
            let rate = supp.max * 0.1; // 10% of max per second default decay
            supp.current = (supp.current - rate * dt).max(0.0);
        }
        let should_pin = supp.current >= supp.max;
        match (should_pin, pinned.is_some()) {
            (true, false) => { commands.entity(entity).insert(Pinned); }
            (false, true) => { commands.entity(entity).remove::<Pinned>(); }
            _ => {}
        }
    }
}

/// Update MoraleState; insert/remove Routing when Broken/recovered.
pub fn morale_system(
    mut commands: Commands,
    query: Query<(Entity, &Morale, Option<&Routing>)>,
) {
    for (entity, morale, routing) in &query {
        let state = morale_state(morale);
        match (state, routing.is_some()) {
            (MoraleState::Broken, false) => { commands.entity(entity).insert(Routing); }
            (MoraleState::Steady, true) | (MoraleState::Shaken, true) => {
                commands.entity(entity).remove::<Routing>();
            }
            _ => {}
        }
    }
}

/// Morale slowly recovers (0.5/sec) when not Routing; decreases when Pinned (-2/sec).
pub fn morale_decay_system(
    time: Res<Time>,
    mut query: Query<(&mut Morale, Option<&Routing>, Option<&Pinned>)>,
) {
    let dt = time.delta_secs();
    for (mut morale, routing, pinned) in &mut query {
        if routing.is_none() {
            morale.current = (morale.current + 0.5 * dt).min(morale.max);
        }
        if pinned.is_some() {
            morale.current = (morale.current - 2.0 * dt).max(0.0);
        }
    }
}

/// Clear TookDamageThisFrame marker each frame (cleanup system).
pub fn clear_took_damage_system(
    mut commands: Commands,
    query: Query<Entity, With<TookDamageThisFrame>>,
) {
    for entity in &query {
        commands.entity(entity).remove::<TookDamageThisFrame>();
    }
}

// --- Plugin ---

pub struct CombatPlugin;

impl Plugin for CombatPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                clear_took_damage_system,
                update_cover_system,
                update_facing_system,
                suppression_decay_system,
                suppression_system,
                cooldown_system,
                attack_system,
                suppression_pin_system,
                morale_system,
                morale_decay_system,
                death_system,
            )
                .chain(),
        );
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    fn make_health(current: f32, max: f32) -> Health {
        Health { current, max }
    }

    fn make_pos(x: i32, y: i32) -> GridPos {
        GridPos { x, y }
    }

    #[test]
    fn test_apply_damage_reduces_health() {
        let mut h = make_health(100.0, 100.0);
        apply_damage(&mut h, 30.0);
        assert_eq!(h.current, 70.0);
    }

    #[test]
    fn test_apply_damage_returns_true_on_death() {
        let mut h = make_health(10.0, 100.0);
        let died = apply_damage(&mut h, 10.0);
        assert!(died);
    }

    #[test]
    fn test_apply_damage_does_not_go_below_zero() {
        let mut h = make_health(5.0, 100.0);
        apply_damage(&mut h, 50.0);
        assert_eq!(h.current, 0.0);
    }

    #[test]
    fn test_chebyshev_distance_adjacent() {
        let a = make_pos(0, 0);
        let b = make_pos(1, 0);
        assert_eq!(chebyshev_distance(&a, &b), 1.0);
    }

    #[test]
    fn test_chebyshev_distance_diagonal() {
        let a = make_pos(0, 0);
        let b = make_pos(1, 1);
        assert_eq!(chebyshev_distance(&a, &b), 1.0);
    }

    #[test]
    fn test_chebyshev_distance_two_away() {
        let a = make_pos(0, 0);
        let b = make_pos(2, 1);
        assert_eq!(chebyshev_distance(&a, &b), 2.0);
    }

    #[test]
    fn test_is_in_range_true() {
        let a = make_pos(0, 0);
        let b = make_pos(2, 2);
        assert!(is_in_range(&a, &b, 2.0));
    }

    #[test]
    fn test_is_in_range_false() {
        let a = make_pos(0, 0);
        let b = make_pos(3, 0);
        assert!(!is_in_range(&a, &b, 2.0));
    }

    #[test]
    fn test_health_not_dead_at_positive() {
        let mut h = make_health(1.0, 100.0);
        let died = apply_damage(&mut h, 0.5);
        assert!(!died);
        assert!(h.current > 0.0);
    }

    #[test]
    fn test_damage_after_no_cover_is_unchanged() {
        assert_eq!(damage_after_cover(40.0, &CoverDensity::None), 40.0);
    }

    #[test]
    fn test_damage_after_light_cover_reduced_25pct() {
        assert_eq!(damage_after_cover(40.0, &CoverDensity::Light), 30.0);
    }

    #[test]
    fn test_damage_after_heavy_cover_reduced_50pct() {
        assert_eq!(damage_after_cover(40.0, &CoverDensity::Heavy), 20.0);
    }

    // --- InCover / effective_damage tests ---

    #[test]
    fn test_cover_reduces_damage_light() {
        let cover = InCover { density: CoverDensity::Light };
        let result = effective_damage(100.0, Some(&cover), None);
        assert_eq!(result, 75.0);
    }

    #[test]
    fn test_cover_reduces_damage_heavy() {
        let cover = InCover { density: CoverDensity::Heavy };
        let result = effective_damage(100.0, Some(&cover), None);
        assert_eq!(result, 50.0);
    }

    #[test]
    fn test_no_cover_full_damage() {
        let result = effective_damage(100.0, None, None);
        assert_eq!(result, 100.0);
    }

    #[test]
    fn test_cover_cannot_reduce_below_zero() {
        let cover = InCover { density: CoverDensity::Heavy };
        let result = effective_damage(0.0, Some(&cover), None);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_effective_damage_calculation() {
        let cover = InCover { density: CoverDensity::Heavy };
        let result = effective_damage(100.0, Some(&cover), None);
        assert_eq!(result, 50.0);
    }

    // --- Suppression tests ---

    fn make_supp(current: f32, max: f32) -> Suppression {
        Suppression { current, max }
    }

    #[test]
    fn test_add_suppression_accumulates() {
        let mut s = make_supp(0.0, 100.0);
        add_suppression(&mut s, 30.0);
        assert_eq!(s.current, 30.0);
        add_suppression(&mut s, 20.0);
        assert_eq!(s.current, 50.0);
    }

    #[test]
    fn test_add_suppression_clamps_to_max() {
        let mut s = make_supp(80.0, 100.0);
        add_suppression(&mut s, 50.0);
        assert_eq!(s.current, 100.0);
    }

    #[test]
    fn test_decay_reduces_suppression() {
        let mut s = make_supp(50.0, 100.0);
        decay_suppression(&mut s, 1.0, 10.0);
        assert_eq!(s.current, 40.0);
    }

    #[test]
    fn test_decay_does_not_go_below_zero() {
        let mut s = make_supp(5.0, 100.0);
        decay_suppression(&mut s, 1.0, 100.0);
        assert_eq!(s.current, 0.0);
    }

    #[test]
    fn test_is_fully_suppressed_true_at_max() {
        let s = make_supp(100.0, 100.0);
        assert!(is_fully_suppressed(&s));
    }

    #[test]
    fn test_is_fully_suppressed_false_below_max() {
        let s = make_supp(99.0, 100.0);
        assert!(!is_fully_suppressed(&s));
    }

    #[test]
    fn test_suppression_output_multiplier_zero_supp_full_output() {
        let s = make_supp(0.0, 100.0);
        assert_eq!(suppression_output_multiplier(&s), 1.0);
    }

    #[test]
    fn test_suppression_output_multiplier_full_supp_half_output() {
        let s = make_supp(100.0, 100.0);
        assert_eq!(suppression_output_multiplier(&s), 0.5);
    }

    #[test]
    fn test_suppression_movement_multiplier_full_supp_half_speed() {
        let s = make_supp(100.0, 100.0);
        assert_eq!(suppression_movement_multiplier(&s), 0.5);
    }

    #[test]
    fn test_suppression_after_cover_no_cover_unchanged() {
        assert_eq!(suppression_after_cover(20.0, None), 20.0);
    }

    #[test]
    fn test_suppression_after_heavy_cover_halved() {
        let c = InCover { density: CoverDensity::Heavy };
        assert_eq!(suppression_after_cover(20.0, Some(&c)), 10.0);
    }

    #[test]
    fn test_suppression_after_light_cover_reduced_25pct() {
        let c = InCover { density: CoverDensity::Light };
        assert_eq!(suppression_after_cover(20.0, Some(&c)), 15.0);
    }

    // --- Facing / AttackAngle tests ---

    #[test]
    fn test_attack_angle_front_north() {
        // Attacker north of target (dy < 0 from target to attacker), target facing North → Front
        let attacker = make_pos(0, -1);
        let target = make_pos(0, 0);
        assert_eq!(attack_angle(&attacker, &target, &Facing::North), AttackAngle::Front);
    }

    #[test]
    fn test_attack_angle_rear_north() {
        // Attacker south of target (dy > 0), target facing North → Rear
        let attacker = make_pos(0, 1);
        let target = make_pos(0, 0);
        assert_eq!(attack_angle(&attacker, &target, &Facing::North), AttackAngle::Rear);
    }

    #[test]
    fn test_attack_angle_flank_east() {
        // Attacker east of target facing North → Flank
        let attacker = make_pos(1, 0);
        let target = make_pos(0, 0);
        assert_eq!(attack_angle(&attacker, &target, &Facing::North), AttackAngle::Flank);
    }

    #[test]
    fn test_attack_angle_front_east() {
        // Attacker east of target, target facing East → Front
        let attacker = make_pos(1, 0);
        let target = make_pos(0, 0);
        assert_eq!(attack_angle(&attacker, &target, &Facing::East), AttackAngle::Front);
    }

    #[test]
    fn test_attack_angle_rear_west() {
        // Attacker east of target, target facing West → Rear
        let attacker = make_pos(1, 0);
        let target = make_pos(0, 0);
        assert_eq!(attack_angle(&attacker, &target, &Facing::West), AttackAngle::Rear);
    }

    #[test]
    fn test_angle_multiplier_front() {
        assert_eq!(angle_damage_multiplier(&AttackAngle::Front), 1.0);
    }

    #[test]
    fn test_angle_multiplier_flank() {
        assert_eq!(angle_damage_multiplier(&AttackAngle::Flank), 1.35);
    }

    #[test]
    fn test_angle_multiplier_rear() {
        assert_eq!(angle_damage_multiplier(&AttackAngle::Rear), 1.75);
    }

    #[test]
    fn test_effective_cover_front() {
        // Heavy cover front → full 0.5 reduction
        let reduction = effective_cover_reduction(&CoverDensity::Heavy, &AttackAngle::Front);
        assert_eq!(reduction, 0.5);
    }

    #[test]
    fn test_effective_cover_flank() {
        // Heavy cover flank → 0.25 reduction (half)
        let reduction = effective_cover_reduction(&CoverDensity::Heavy, &AttackAngle::Flank);
        assert_eq!(reduction, 0.25);
    }

    #[test]
    fn test_effective_cover_rear() {
        // Heavy cover rear → 0.0 reduction
        let reduction = effective_cover_reduction(&CoverDensity::Heavy, &AttackAngle::Rear);
        assert_eq!(reduction, 0.0);
    }

    #[test]
    fn test_effective_damage_flank_no_cover() {
        // base=100, flank, no cover → 135.0
        let result = effective_damage(100.0, None, Some(AttackAngle::Flank));
        assert_eq!(result, 135.0);
    }

    #[test]
    fn test_effective_damage_rear_heavy_cover() {
        // base=100, rear, heavy cover → 175.0 (cover bypassed)
        let cover = InCover { density: CoverDensity::Heavy };
        let result = effective_damage(100.0, Some(&cover), Some(AttackAngle::Rear));
        assert_eq!(result, 175.0);
    }

    #[test]
    fn test_effective_damage_front_light_cover() {
        // base=100, front, light cover → 75.0
        let cover = InCover { density: CoverDensity::Light };
        let result = effective_damage(100.0, Some(&cover), Some(AttackAngle::Front));
        assert_eq!(result, 75.0);
    }

    // --- Suppression penalty tests (step 7) ---

    #[test]
    fn test_suppression_movement_penalty_none() {
        assert_eq!(suppression_movement_penalty(0.0, 100.0), 0.0);
    }

    #[test]
    fn test_suppression_movement_penalty_full() {
        assert_eq!(suppression_movement_penalty(100.0, 100.0), 0.8);
    }

    #[test]
    fn test_suppression_movement_penalty_half() {
        let penalty = suppression_movement_penalty(50.0, 100.0);
        assert!((penalty - 0.4).abs() < 1e-6, "expected ~0.4, got {penalty}");
    }

    #[test]
    fn test_suppression_accuracy_penalty_none() {
        assert_eq!(suppression_accuracy_penalty(0.0, 100.0), 0.0);
    }

    #[test]
    fn test_suppression_accuracy_penalty_full() {
        assert_eq!(suppression_accuracy_penalty(100.0, 100.0), 0.5);
    }

    // --- Morale state tests ---

    fn make_morale(current: f32, max: f32) -> Morale {
        Morale { current, max }
    }

    #[test]
    fn test_morale_state_steady() {
        let m = make_morale(80.0, 100.0);
        assert_eq!(morale_state(&m), MoraleState::Steady);
    }

    #[test]
    fn test_morale_state_shaken() {
        let m = make_morale(50.0, 100.0);
        assert_eq!(morale_state(&m), MoraleState::Shaken);
    }

    #[test]
    fn test_morale_state_broken() {
        let m = make_morale(20.0, 100.0);
        assert_eq!(morale_state(&m), MoraleState::Broken);
    }

    #[test]
    fn test_morale_state_boundary_high() {
        // 66/100 = 0.66, not > 0.66, so Shaken
        let m = make_morale(66.0, 100.0);
        assert_eq!(morale_state(&m), MoraleState::Shaken);
    }

    #[test]
    fn test_morale_state_boundary_low() {
        // 33/100 = 0.33, not > 0.33, so Broken
        let m = make_morale(33.0, 100.0);
        assert_eq!(morale_state(&m), MoraleState::Broken);
    }

    // --- suppression_gain_per_hit tests ---

    #[test]
    fn test_suppression_gain_no_cover() {
        assert_eq!(suppression_gain_per_hit(20.0, None), 20.0);
    }

    #[test]
    fn test_suppression_gain_light_cover() {
        let c = InCover { density: CoverDensity::Light };
        assert_eq!(suppression_gain_per_hit(20.0, Some(&c)), 15.0);
    }

    #[test]
    fn test_suppression_gain_heavy_cover() {
        let c = InCover { density: CoverDensity::Heavy };
        assert_eq!(suppression_gain_per_hit(20.0, Some(&c)), 10.0);
    }
}
