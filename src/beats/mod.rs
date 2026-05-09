use bevy::prelude::*;
use crate::combat::{Health, Dead};
use crate::heroes::{Hero, HeroDowned};
use crate::buildings::{BuildingTypeId, BuildingPos};
use crate::map::Faction;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BeatId {
    HeroGoesDown,
    LastStand,
    AncientUnification,
}

#[derive(Resource, Debug, Default)]
pub struct FiredBeats(pub HashSet<BeatId>);

pub fn should_fire_hero_goes_down(any_downed: bool, already_fired: bool) -> bool {
    any_downed && !already_fired
}

pub fn should_fire_last_stand(
    min_command_hp_fraction: f32,
    already_fired: bool,
) -> bool {
    min_command_hp_fraction < 0.30 && !already_fired
}

pub fn beat_check_system(
    mut fired: ResMut<FiredBeats>,
    heroes: Query<&Hero, With<HeroDowned>>,
    bunkers: Query<(&BuildingTypeId, &Health, &BuildingPos), Without<Dead>>,
) {
    if should_fire_hero_goes_down(!heroes.is_empty(), fired.0.contains(&BeatId::HeroGoesDown)) {
        fired.0.insert(BeatId::HeroGoesDown);
    }

    let mut min_frac: f32 = 1.0;
    for (bt, h, _) in &bunkers {
        if bt.id() == "command_bunker" && h.max > 0.0 {
            let frac = h.current / h.max;
            if frac < min_frac {
                min_frac = frac;
            }
        }
    }
    if should_fire_last_stand(min_frac, fired.0.contains(&BeatId::LastStand)) {
        fired.0.insert(BeatId::LastStand);
    }

    let _: Option<Faction> = None;
}

pub struct BeatsPlugin;

impl Plugin for BeatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FiredBeats>();
        app.add_systems(Update, beat_check_system);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hero_goes_down_fires_first_time() {
        assert!(should_fire_hero_goes_down(true, false));
    }

    #[test]
    fn hero_goes_down_does_not_refire() {
        assert!(!should_fire_hero_goes_down(true, true));
    }

    #[test]
    fn hero_goes_down_no_downed_no_fire() {
        assert!(!should_fire_hero_goes_down(false, false));
    }

    #[test]
    fn last_stand_fires_under_30pct() {
        assert!(should_fire_last_stand(0.29, false));
    }

    #[test]
    fn last_stand_no_fire_at_30pct() {
        assert!(!should_fire_last_stand(0.30, false));
    }

    #[test]
    fn last_stand_no_refire() {
        assert!(!should_fire_last_stand(0.10, true));
    }
}
