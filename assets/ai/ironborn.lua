-- Ironborn AI — tactical doctrine: always flank, never charge directly.
-- Ironborn are mechanized fighters who use mobility and flanking maneuvers.

function ai_tick(state)
    local cmds = {}

    local function nearest_cover(unit)
        local best, best_dist = nil, 999
        for _, c in ipairs(state.cover_positions) do
            local d = math.abs(c.x - unit.x) + math.abs(c.y - unit.y)
            if d < best_dist then best, best_dist = c, d end
        end
        return best
    end

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

        if hp_frac < 0.25 and not unit.in_cover then
            local cover = nearest_cover(unit)
            if cover then
                table.insert(cmds, { cmd = "move", unit_id = unit.id, x = cover.x, y = cover.y })
            end
        elseif target then
            -- Ironborn doctrine: always flank — offset attack position by 15 tiles on X
            table.insert(cmds, { cmd = "attack_move", unit_id = unit.id,
                x = target.x + 15, y = target.y })
        end
    end

    return cmds
end

function on_wave(state)
    -- Ironborn always flanks — 100% of units take a flanking approach
    local cmds = {}
    local target_x, target_y = 64, 40
    if #state.enemy_visible > 0 then
        target_x = state.enemy_visible[1].x
        target_y = state.enemy_visible[1].y
    end

    for _, unit in ipairs(state.my_units) do
        -- Full flank: all units approach from the side
        local flank_x = target_x + 15
        local flank_y = target_y
        -- Alternate flanks for different units to split defender attention
        if unit.id % 2 == 0 then
            flank_x = target_x - 15
        end
        table.insert(cmds, { cmd = "attack_move", unit_id = unit.id,
            x = flank_x, y = flank_y })
    end
    return cmds
end
