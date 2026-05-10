//! Headless campaign AI validation: each campaign map must resolve within its deadline.
//!
//!   cargo test --test campaign_ai -- --nocapture

use std::time::Duration;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use cindertide::{
    map::{MapPlugin, ControlPoint, Faction},
    units::{UnitPlugin, UnitPos},
    combat::CombatPlugin,
    resources::ResourcesPlugin,
    control::ControlPlugin,
    buildings::BuildingsPlugin,
    production::ProductionPlugin,
    heroes::HeroPlugin,
    tech::TechPlugin,
    unit_ai::UnitAiPlugin,
    repair::RepairPlugin,
    ai::AiPlugin,
    mission::{Mission, MissionPlugin, MissionStatus},
    campaign::CampaignPlugin,
    beats::BeatsPlugin,
    hollow::HollowPlugin,
    save::SavePlugin,
    game::GamePlugin,
    mission_script::MissionScriptPlugin,
    factions::LoadedFactions,
    narrative::NarrativeData,
    load_campaign_map, bake_navmesh,
};

fn build_headless_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(500)));
    app.add_plugins(MapPlugin)
        .add_plugins(UnitPlugin)
        .add_plugins(CombatPlugin)
        .add_plugins(ResourcesPlugin)
        .add_plugins(ControlPlugin)
        .add_plugins(BuildingsPlugin)
        .add_plugins(ProductionPlugin)
        .add_plugins(HeroPlugin)
        .add_plugins(TechPlugin)
        .add_plugins(UnitAiPlugin)
        .add_plugins(RepairPlugin)
        .add_plugins(AiPlugin)
        .add_plugins(MissionPlugin)
        .add_plugins(CampaignPlugin)
        .add_plugins(BeatsPlugin)
        .add_plugins(HollowPlugin)
        .add_plugins(SavePlugin)
        .add_plugins(GamePlugin)
        .add_plugins(MissionScriptPlugin);

    let loaded_factions = LoadedFactions::load_from_dir("assets/factions");
    app.insert_resource(loaded_factions);

    let narrative = NarrativeData::load("assets/narrative.toml")
        .unwrap_or_else(|_| NarrativeData {
            factions: Default::default(),
            finales: cindertide::narrative::Finales {
                combine_first: String::new(),
                ironborn_first: String::new(),
            },
        });
    app.insert_resource(narrative);
    app
}

// Safety ceiling: missions that cannot resolve via objectives within this time
// are considered deadlocked. Set well above the longest real deadline (3600s)
// so script events fire and the AI has time to respond.
const GRACE: f32 = 4200.0;

fn run_map_to_completion(map_path: &str) {
    let mut app = build_headless_app();
    app.update(); // startup systems

    load_campaign_map(app.world_mut(), map_path);
    bake_navmesh(app.world_mut());
    // No deadline override — mission uses its own deadline and scripts fire
    // at their scripted times. AI is injected for the player faction by
    // load_campaign_map so both sides play out.

    let mut ticks = 0u32;

    loop {
        app.update();
        ticks += 1;

        let (status, elapsed) = app
            .world_mut()
            .query::<&Mission>()
            .iter(app.world())
            .next()
            .map(|m| (m.status.clone(), m.elapsed))
            .unwrap_or((MissionStatus::Active, 0.0));

        if ticks % 100 == 0 {
            // Count live units per faction and track average positions.
            let mut combine_units = 0u32;
            let mut ironborn_units = 0u32;
            let mut other_units = 0u32;
            let mut combine_sum_x = 0i32;
            let mut ironborn_sum_x = 0i32;
            {
                let mut q = app.world_mut().query::<(&UnitPos, &Faction)>();
                for (pos, f) in q.iter(app.world()) {
                    match f.id() {
                        "combine"  => { combine_units += 1; combine_sum_x += pos.pos.x; }
                        "ironborn" => { ironborn_units += 1; ironborn_sum_x += pos.pos.x; }
                        _          => other_units += 1,
                    }
                }
            }
            let combine_avg_x = if combine_units > 0 { combine_sum_x / combine_units as i32 } else { 0 };
            let ironborn_avg_x = if ironborn_units > 0 { ironborn_sum_x / ironborn_units as i32 } else { 0 };
            // Count control point ownership.
            let mut cp_combine = 0u32;
            let mut cp_ironborn = 0u32;
            let mut cp_neutral = 0u32;
            {
                let mut q = app.world_mut().query::<&ControlPoint>();
                for cp in q.iter(app.world()) {
                    match cp.owner.as_ref().map(|f| f.id()) {
                        Some("combine")  => cp_combine += 1,
                        Some("ironborn") => cp_ironborn += 1,
                        _                => cp_neutral += 1,
                    }
                }
            }
            println!(
                "[{}] t={:.0}s  units combine={}(avgX={}) ironborn={}(avgX={}) other={}  cps combine={} ironborn={} neutral={}",
                map_path, elapsed, combine_units, combine_avg_x, ironborn_units, ironborn_avg_x, other_units,
                cp_combine, cp_ironborn, cp_neutral,
            );
        }

        match status {
            MissionStatus::Won | MissionStatus::Lost => {
                println!("[{}] resolved {:?} after {} ticks ({:.1}s)", map_path, status, ticks, elapsed);
                return;
            }
            _ => {}
        }

        assert!(elapsed < GRACE,
            "{map_path}: did not resolve within {GRACE}s (ticks={ticks}) — possible deadlock"
        );
    }
}

macro_rules! map_test {
    ($name:ident, $path:expr) => {
        #[test]
        fn $name() { run_map_to_completion($path); }
    };
}

map_test!(combine_m0, "assets/maps/combine_m0.toml");
map_test!(combine_m1, "assets/maps/combine_m1.toml");
map_test!(combine_m2, "assets/maps/combine_m2.toml");
map_test!(combine_m3, "assets/maps/combine_m3.toml");
map_test!(combine_m4, "assets/maps/combine_m4.toml");
map_test!(ironborn_m0, "assets/maps/ironborn_m0.toml");
map_test!(ironborn_m1, "assets/maps/ironborn_m1.toml");
map_test!(ironborn_m2, "assets/maps/ironborn_m2.toml");
map_test!(ironborn_m3, "assets/maps/ironborn_m3.toml");
map_test!(ironborn_m4, "assets/maps/ironborn_m4.toml");
map_test!(architect_m0, "assets/maps/architect_m0.toml");
map_test!(architect_m1, "assets/maps/architect_m1.toml");
map_test!(architect_m2, "assets/maps/architect_m2.toml");
map_test!(architect_m3, "assets/maps/architect_m3.toml");
map_test!(architect_m4, "assets/maps/architect_m4.toml");
