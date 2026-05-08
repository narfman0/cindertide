use bevy::prelude::*;
use super::{MoveProgress, MoveTarget, MovementSpeed, UnitPos};

/// Advances unit movement each frame. When elapsed >= 1.0 a step along the
/// pre-computed path is consumed and elapsed resets. MoveTarget is removed
/// once the unit reaches its destination.
pub fn move_units_system(
    mut commands: Commands,
    mut query: Query<(Entity, &mut UnitPos, &MoveTarget, &MovementSpeed, &mut MoveProgress)>,
    time: Res<Time>,
) {
    for (entity, mut unit_pos, _move_target, speed, mut progress) in query.iter_mut() {
        progress.elapsed += time.delta_secs() * speed.tiles_per_second;

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
