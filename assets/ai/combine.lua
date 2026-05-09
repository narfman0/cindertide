-- Combine AI — aggressive focus fire, combined arms coordination.
-- The Combine uses superior firepower and coordinated strikes to overwhelm enemies.

function ai_tick(state)
    local cmds = {}

    local function priority_target()
        -- Combine prioritizes highest-threat enemies (heavy weapons/armor first, then lowest HP)
        local best, best_score = nil, -1
        for _, e in ipairs(state.enemy_visible) do
            -- Score: prefer heavy units (they deal more damage) and damaged units (easier to finish)
            local threat_score = 1000 - e.health
            if e.type == "HeavyArmor" then threat_score = threat_score + 500 end
            if e.type == "HeavyWeapons" then threat_score = threat_score + 300 end
            if threat_score > best_score then best, best_score = e, threat_score end
        end
        return best
    end

    local target = priority_target()

    for _, unit in ipairs(state.my_units) do
        local hp_frac = unit.health / unit.max_health

        -- Combine: only retreat below 20% (more aggressive threshold)
        if hp_frac < 0.2 then
            -- Find any nearby cover
            local best_cover, best_d = nil, 999
            for _, c in ipairs(state.cover_positions) do
                local d = math.abs(c.x - unit.x) + math.abs(c.y - unit.y)
                if d < best_d then best_cover, best_d = c, d end
            end
            if best_cover then
                table.insert(cmds, { cmd = "move", unit_id = unit.id, x = best_cover.x, y = best_cover.y })
            end
        elseif target then
            -- Combine always focus-fires: all units on priority target
            table.insert(cmds, { cmd = "attack", unit_id = unit.id, target_id = target.id })
        end
    end

    return cmds
end

function on_wave(state)
    -- Combine: coordinated frontal assault with heavy support
    local cmds = {}
    local target_x, target_y = 64, 40
    if #state.enemy_visible > 0 then
        target_x = state.enemy_visible[1].x
        target_y = state.enemy_visible[1].y
    end

    -- Heavy units (HeavyArmor, HeavyWeapons) lead; infantry follows
    local heavy_units = {}
    local light_units = {}
    for _, unit in ipairs(state.my_units) do
        if unit.type == "HeavyArmor" or unit.type == "HeavyWeapons" then
            table.insert(heavy_units, unit)
        else
            table.insert(light_units, unit)
        end
    end

    -- Heavy units advance directly
    for _, unit in ipairs(heavy_units) do
        table.insert(cmds, { cmd = "attack_move", unit_id = unit.id,
            x = target_x, y = target_y })
    end

    -- Light infantry flanks behind heavy units
    for i, unit in ipairs(light_units) do
        local offset = (i % 2 == 0) and 5 or -5
        table.insert(cmds, { cmd = "attack_move", unit_id = unit.id,
            x = target_x + offset, y = target_y })
    end

    return cmds
end
