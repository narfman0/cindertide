use bevy::prelude::*;
use super::{MoveProgress, MoveTarget, MovementSpeed, UnitPos};
use crate::combat::{Pinned, Suppression, suppression_movement_multiplier};

/// Advances unit movement each frame. When elapsed >= 1.0 a step along the
/// pre-computed path is consumed and elapsed resets. MoveTarget is removed
/// once the unit reaches its destination. Pinned units skip movement entirely;
/// merely suppressed units move at reduced speed.
pub fn move_units_system(
    mut commands: Commands,
    mut query: Query<
        (Entity, &mut UnitPos, &MoveTarget, &MovementSpeed, &mut MoveProgress, Option<&Suppression>),
        Without<Pinned>,
    >,
    time: Res<Time>,
) {
    for (entity, mut unit_pos, _move_target, speed, mut progress, supp) in query.iter_mut() {
        let mult = supp.map(suppression_movement_multiplier).unwrap_or(1.0);
        progress.elapsed += time.delta_secs() * speed.tiles_per_second * mult;

        while progress.elapsed >= 1.0 {
            progress.elapsed -= 1.0;
            let next_step = progress.current_step + 1;
            if next_step < progress.path.len() {
                unit_pos.pos = progress.path[next_step].clone();
                progress.current_step = next_step;
            }
            // Reached final waypoint — remove movement components
            if progress.current_step + 1 >= progress.path.len() {
                commands.entity(entity).remove::<MoveTarget>();
                commands.entity(entity).remove::<MoveProgress>();
                break;
            }
        }
    }
}
