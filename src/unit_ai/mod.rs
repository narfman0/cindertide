// Unit AI — autonomous behaviors per `agents.md`.
// - Threat response: return fire when attacked and no active order
// - Routing retreat: broken units move toward HomeBase

use bevy::prelude::*;
use crate::combat::{AttackTarget, LastAttackedBy, Routing, Dead, Pinned};
use crate::units::{UnitPos, HomeBase, MovementSpeed, MoveProgress};

/// Time after which a LastAttackedBy memory expires.
pub const THREAT_MEMORY_SECONDS: f32 = 6.0;

// --- Pure helpers ---

pub fn step_toward(from: &crate::map::GridPos, to: &crate::map::GridPos) -> crate::map::GridPos {
    let dx = (to.x - from.x).signum();
    let dy = (to.y - from.y).signum();
    crate::map::GridPos { x: from.x + dx, y: from.y + dy }
}

// --- Systems ---

/// Tick LastAttackedBy memory; remove when stale.
pub fn threat_memory_decay_system(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut LastAttackedBy)>,
) {
    let dt = time.delta_secs();
    for (entity, mut last) in &mut q {
        last.age_secs += dt;
        if last.age_secs > THREAT_MEMORY_SECONDS {
            commands.entity(entity).remove::<LastAttackedBy>();
        }
    }
}

/// Unit returned fire: if attacked and no current order, target the attacker.
pub fn threat_response_system(
    mut commands: Commands,
    q: Query<(Entity, &LastAttackedBy), (Without<AttackTarget>, Without<Dead>, Without<Routing>)>,
) {
    for (entity, last) in &q {
        commands.entity(entity).insert(AttackTarget { entity: last.attacker });
    }
}

/// Broken (Routing) units move one tile toward HomeBase per (1/speed) seconds.
/// Bypasses MoveTarget/MoveProgress.path entirely.
pub fn routing_retreat_system(
    time: Res<Time>,
    mut q: Query<
        (&HomeBase, &mut UnitPos, &MovementSpeed, &mut MoveProgress),
        (With<Routing>, Without<Pinned>, Without<Dead>),
    >,
) {
    let dt = time.delta_secs();
    for (home, mut pos, speed, mut progress) in &mut q {
        progress.elapsed += dt * speed.tiles_per_second;
        while progress.elapsed >= 1.0 {
            progress.elapsed -= 1.0;
            if pos.pos == home.pos {
                break;
            }
            let next = step_toward(&pos.pos, &home.pos);
            pos.pos = next;
        }
    }
}

pub struct UnitAiPlugin;

impl Plugin for UnitAiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (threat_memory_decay_system, threat_response_system, routing_retreat_system).chain(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::GridPos;

    #[test]
    fn step_toward_diagonal_moves_one_tile_each_axis() {
        let from = GridPos { x: 0, y: 0 };
        let to = GridPos { x: 5, y: 5 };
        assert_eq!(step_toward(&from, &to), GridPos { x: 1, y: 1 });
    }

    #[test]
    fn step_toward_horizontal_only() {
        let from = GridPos { x: 0, y: 0 };
        let to = GridPos { x: 5, y: 0 };
        assert_eq!(step_toward(&from, &to), GridPos { x: 1, y: 0 });
    }

    #[test]
    fn step_toward_negative_direction() {
        let from = GridPos { x: 5, y: 5 };
        let to = GridPos { x: 0, y: 0 };
        assert_eq!(step_toward(&from, &to), GridPos { x: 4, y: 4 });
    }

    #[test]
    fn step_toward_already_at_destination_is_idempotent() {
        let from = GridPos { x: 3, y: 3 };
        let to = GridPos { x: 3, y: 3 };
        assert_eq!(step_toward(&from, &to), GridPos { x: 3, y: 3 });
    }
}
