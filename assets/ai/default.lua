-- Default Cindertide AI — plays fair, plays smart

function ai_tick(state)
    local cmds = {}

    -- Find cover positions near each unit
    local function nearest_cover(unit)
        local best, best_dist = nil, 999
        for _, c in ipairs(state.cover_positions) do
            local d = math.abs(c.x - unit.x) + math.abs(c.y - unit.y)
            if d < best_dist then best, best_dist = c, d end
        end
        return best
    end

    -- Focus fire: find the lowest-health visible enemy
    local function priority_target()
        local best, best_hp = nil, 999999
        for _, e in ipairs(state.enemy_visible) do
            if e.health < best_hp then best, best_hp = e, e.health end
        end
        return best
    end

    local target = priority_target()

    for _, unit in ipairs(state.my_units) do
        local hp_frac = unit.health / unit.max_health

        -- Retreat to cover if health low and not already in cover
        if hp_frac < 0.3 and not unit.in_cover then
            local cover = nearest_cover(unit)
            if cover then
                table.insert(cmds, { cmd = "move", unit_id = unit.id, x = cover.x, y = cover.y })
            end
        -- Attack priority target if visible and focus_fire is on
        elseif target and state.params.focus_fire then
            table.insert(cmds, { cmd = "attack", unit_id = unit.id, target_id = target.id })
        -- Move to cover first if not already there and enemies are near
        elseif #state.enemy_visible > 0 and not unit.in_cover then
            local cover = nearest_cover(unit)
            if cover then
                table.insert(cmds, { cmd = "move", unit_id = unit.id, x = cover.x, y = cover.y })
            end
        elseif target then
            table.insert(cmds, { cmd = "attack_move", unit_id = unit.id, x = target.x, y = target.y })
        end
    end

    return cmds
end

function on_wave(state)
    -- Called when attack wave timer fires
    -- Returns list of units to include in wave and target position
    local cmds = {}
    -- Find enemy buildings, target nearest
    local target_x, target_y = 64, 40  -- default center
    if #state.enemy_visible > 0 then
        target_x = state.enemy_visible[1].x
        target_y = state.enemy_visible[1].y
    end

    -- Split force: 70% direct assault, 30% flank
    local units = state.my_units
    local flank_count = math.floor(#units * 0.3)
    for i, unit in ipairs(units) do
        if i <= flank_count then
            -- Flank: approach from 90 degrees offset
            table.insert(cmds, { cmd = "attack_move", unit_id = unit.id,
                x = target_x + 15, y = target_y })
        else
            table.insert(cmds, { cmd = "attack_move", unit_id = unit.id,
                x = target_x, y = target_y })
        end
    end
    return cmds
end
