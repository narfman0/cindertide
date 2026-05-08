//! TUI rendering — pure layout + style; reads from a TuiApp snapshot.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::state::{Screen, TuiApp, UnitSummary, BuildingSummary, TileSummary};

pub fn draw(f: &mut Frame, app: &TuiApp) {
    match &app.screen {
        Screen::Title { selected } => draw_title(f, *selected),
        Screen::FactionPicker { selected } => draw_faction_picker(f, *selected),
        Screen::LoadPicker { selected, slots } => draw_load_picker(f, *selected, slots),
        Screen::Campaign { selected_option } => draw_campaign(f, app, *selected_option),
        Screen::InMission => draw_in_mission(f, app),
        Screen::GameOver { won } => draw_game_over(f, *won),
    }
}

fn draw_title(f: &mut Frame, selected: usize) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Cindertide ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let menu_lines = ["Single Player", "Load Game", "Exit"];
    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  CINDERTIDE",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "  diesel + bone + a voice in the dark",
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )),
        Line::from(""),
        Line::from(""),
    ];
    for (i, item) in menu_lines.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let style = if i == selected {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        lines.push(Line::from(Span::styled(format!("{prefix}{item}"), style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↑/↓ select   Enter confirm   q quit",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_faction_picker(f: &mut Frame, selected: usize) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Choose Faction ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let factions = [
        ("Combine",  "Industrial expansion: 1 Bunker, 1 Refinery, 1 Barracks, 4 Riflemen", Color::Yellow),
        ("Covenant", "Defensive turtle: 1 Cathedral (Bunker), 2 outposts, 3 Riflemen",     Color::Blue),
        ("Ironborn", "Aggressive salvage: 1 Foundry, 5 Riflemen",                          Color::Gray),
    ];
    let mut lines: Vec<Line> = vec![Line::from("")];
    for (i, (name, blurb, color)) in factions.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let head_style = if i == selected {
            Style::default().fg(*color).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(*color)
        };
        lines.push(Line::from(Span::styled(format!("{prefix}{name}"), head_style)));
        lines.push(Line::from(Span::styled(
            format!("    {blurb}"),
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "  ↑/↓ select   Enter confirm   Esc back",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_load_picker(f: &mut Frame, selected: usize, slots: &[String]) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Load Game ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = vec![Line::from("")];
    if slots.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no saves)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, slot) in slots.iter().enumerate() {
            let prefix = if i == selected { "> " } else { "  " };
            let style = if i == selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            lines.push(Line::from(Span::styled(format!("{prefix}{slot}"), style)));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  ↑/↓ select   Enter load   Esc back",
        Style::default().fg(Color::DarkGray),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_campaign(f: &mut Frame, app: &TuiApp, selected: usize) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Campaign — {}   Won: {}   Lost: {} ",
            app.snapshot.player.as_deref().unwrap_or("?"),
            app.snapshot.missions_won,
            app.snapshot.missions_lost,
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Min(8), Constraint::Length(2)])
        .split(inner);

    // Zones
    let mut zone_lines: Vec<Line> = vec![Line::from(Span::styled(
        " Territory:",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    for z in &app.snapshot.zones {
        let owner_str = z.owner.clone().unwrap_or_else(|| "—".to_string());
        let owner_color = faction_color(&owner_str);
        let line = Line::from(vec![
            Span::raw(format!("  [{}] {:<18} ", z.id, z.name)),
            Span::styled(format!("{:<10}", owner_str), Style::default().fg(owner_color)),
            Span::styled(
                format!(" corruption {}%", (z.corruption * 100.0) as u32),
                Style::default().fg(Color::Magenta),
            ),
        ]);
        zone_lines.push(line);
    }
    f.render_widget(Paragraph::new(zone_lines), chunks[0]);

    // Options
    let mut opt_lines: Vec<Line> = vec![Line::from(Span::styled(
        " Mission options:",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if app.snapshot.options.is_empty() {
        opt_lines.push(Line::from(Span::styled(
            "  (none — you control every zone)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, o) in app.snapshot.options.iter().enumerate() {
            let prefix = if i == selected { "> " } else { "  " };
            let style = if i == selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let zone_name = app
                .snapshot
                .zones
                .iter()
                .find(|z| z.id == o.zone_id)
                .map(|z| z.name.clone())
                .unwrap_or_default();
            opt_lines.push(Line::from(Span::styled(
                format!(
                    "{}{} on {:<16} {:<10} vs {}",
                    prefix, " ", zone_name, o.mission_type, o.opponent
                ),
                style,
            )));
        }
    }
    f.render_widget(Paragraph::new(opt_lines), chunks[1]);

    // Controls
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " ↑/↓ select   Enter start mission   s save   Esc title   q quit",
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[2],
    );
}

fn draw_in_mission(f: &mut Frame, app: &TuiApp) {
    let area = f.area();
    let title = if let Some(m) = &app.snapshot.mission {
        let pause = if app.snapshot.paused { " [PAUSED] " } else { "" };
        format!(
            " Mission: {} ({} vs {})   {:.1}s / {:.0}s   {}{}",
            m.mission_type,
            m.player,
            m.opponent,
            m.elapsed,
            m.deadline,
            m.status,
            pause
        )
    } else {
        " Mission ".to_string()
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(20), Constraint::Length(2)])
        .split(inner);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(36), Constraint::Min(40)])
        .split(main_chunks[0]);

    draw_mission_sidebar(f, app, body[0]);
    draw_mission_map(f, app, body[1]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " Space pause   Esc abandon   q quit",
            Style::default().fg(Color::DarkGray),
        ))),
        main_chunks[1],
    );
}

fn draw_mission_sidebar(f: &mut Frame, app: &TuiApp, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(r) = &app.snapshot.resources {
        lines.push(Line::from(Span::styled(
            "Resources",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(format!(
            "  Fuel    {:>5.0}  {:+.1}/s",
            r.fuel, r.fuel_trickle
        )));
        lines.push(Line::from(format!(
            "  Scrap   {:>5.0}  {:+.1}/s",
            r.scrap, r.scrap_trickle
        )));
        lines.push(Line::from(format!(
            "  Manpwr  {:>5.0}  {:+.1}/s",
            r.manpower, r.manpower_trickle
        )));
        lines.push(Line::from(format!("  Pop     {} / {}", r.pop_current, r.pop_max)));
        lines.push(Line::from(""));
    }

    let player = app.snapshot.player.as_deref().unwrap_or("");
    let mut own_units: Vec<&UnitSummary> = app
        .snapshot
        .units
        .iter()
        .filter(|u| u.faction == player)
        .collect();
    own_units.sort_by_key(|u| u.entity_id);
    lines.push(Line::from(Span::styled(
        format!("Units ({})", own_units.len()),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for u in own_units.iter().take(8) {
        let glyph = unit_glyph(&u.unit_type);
        lines.push(Line::from(format!(
            "  [{}] {:<8} {:>3.0}/{:.0}",
            glyph, u.unit_type, u.health, u.health_max,
        )));
    }
    if own_units.len() > 8 {
        lines.push(Line::from(Span::styled(
            format!("  ... +{} more", own_units.len() - 8),
            Style::default().fg(Color::DarkGray),
        )));
    }
    lines.push(Line::from(""));

    let mut own_buildings: Vec<&BuildingSummary> = app
        .snapshot
        .buildings
        .iter()
        .filter(|b| b.faction == player)
        .collect();
    own_buildings.sort_by_key(|b| b.entity_id);
    lines.push(Line::from(Span::styled(
        format!("Buildings ({})", own_buildings.len()),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for b in own_buildings.iter().take(6) {
        let status = if b.built { "built" } else { "constr." };
        lines.push(Line::from(format!(
            "  [B] {:<14} {}",
            b.building_type, status
        )));
    }
    if own_buildings.len() > 6 {
        lines.push(Line::from(Span::styled(
            format!("  ... +{} more", own_buildings.len() - 6),
            Style::default().fg(Color::DarkGray),
        )));
    }

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

fn draw_mission_map(f: &mut Frame, app: &TuiApp, area: Rect) {
    // Determine bounds from tiles.
    if app.snapshot.tiles.is_empty() {
        let p = Paragraph::new(Line::from(Span::styled(
            "(no tiles loaded)",
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(p, area);
        return;
    }
    let max_x = app.snapshot.tiles.iter().map(|t| t.x).max().unwrap_or(0);
    let max_y = app.snapshot.tiles.iter().map(|t| t.y).max().unwrap_or(0);
    let w = (max_x + 1) as usize;
    let h = (max_y + 1) as usize;

    // Build a grid of (char, color).
    let mut grid: Vec<Vec<(char, Color)>> = vec![vec![(' ', Color::Reset); w]; h];
    for t in &app.snapshot.tiles {
        if let (Some(row), x) = (grid.get_mut(t.y as usize), t.x as usize) {
            if let Some(cell) = row.get_mut(x) {
                *cell = terrain_glyph(&t.terrain);
            }
        }
    }
    for b in &app.snapshot.buildings {
        if let (Some(row), x) = (grid.get_mut(b.y as usize), b.x as usize) {
            if let Some(cell) = row.get_mut(x) {
                let g = building_glyph(&b.building_type);
                *cell = (g, faction_color(&b.faction));
            }
        }
    }
    for u in &app.snapshot.units {
        if let (Some(row), x) = (grid.get_mut(u.y as usize), u.x as usize) {
            if let Some(cell) = row.get_mut(x) {
                let g = unit_glyph(&u.unit_type);
                *cell = (g, faction_color(&u.faction));
            }
        }
    }

    let lines: Vec<Line> = grid
        .iter()
        .take(area.height as usize)
        .map(|row| {
            let spans: Vec<Span> = row
                .iter()
                .take(area.width as usize)
                .map(|(c, color)| Span::styled(c.to_string(), Style::default().fg(*color)))
                .collect();
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);
}

fn draw_game_over(f: &mut Frame, won: bool) {
    let area = f.area();
    let title = if won { " Victory " } else { " Defeat " };
    let color = if won { Color::Green } else { Color::Red };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = if won { "VICTORY" } else { "DEFEAT" };
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            format!("    {}", text),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "    Enter / Esc — title screen",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

fn faction_color(name: &str) -> Color {
    match name {
        "Combine" => Color::Yellow,
        "Covenant" => Color::Blue,
        "Ironborn" => Color::Gray,
        "Hollow" => Color::Magenta,
        _ => Color::White,
    }
}

fn terrain_glyph(name: &str) -> (char, Color) {
    match name {
        "Road" => ('=', Color::DarkGray),
        "Grass" => ('.', Color::Green),
        "Forest" => ('^', Color::Green),
        "Rubble" => ('#', Color::DarkGray),
        "Mud" => ('~', Color::Rgb(120, 80, 40)),
        "Corrupted" => ('*', Color::Magenta),
        "Void" => ('X', Color::DarkGray),
        _ => ('.', Color::Reset),
    }
}

fn unit_glyph(unit_type: &str) -> char {
    match unit_type {
        "Riflemen" => 'r',
        "HeavyWeapons" => 'h',
        "LightVehicle" => 'v',
        "HeavyArmor" => 'V',
        _ => 'u',
    }
}

fn building_glyph(bt: &str) -> char {
    match bt {
        "CommandBunker" => 'C',
        "Refinery" => 'R',
        "Scrapyard" => 'S',
        "Barracks" => 'B',
        "Foundry" => 'F',
        "MotorPool" => 'M',
        "SupplyDepot" => 'D',
        "Pillbox" => 'P',
        "RepairBay" => 'H',
        "Watchtower" => 'T',
        "Workshop" => 'W',
        "ResearchLab" => 'L',
        _ => 'b',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_glyphs_unique_per_type() {
        let r = unit_glyph("Riflemen");
        let h = unit_glyph("HeavyWeapons");
        let v = unit_glyph("LightVehicle");
        let big_v = unit_glyph("HeavyArmor");
        assert_ne!(r, h);
        assert_ne!(h, v);
        assert_ne!(v, big_v);
    }

    #[test]
    fn faction_colors_distinct() {
        let combine = faction_color("Combine");
        let hollow = faction_color("Hollow");
        let unknown = faction_color("nonsense");
        assert_ne!(combine, hollow);
        assert_eq!(unknown, Color::White);
    }

    #[test]
    fn terrain_glyph_road() {
        let (c, _) = terrain_glyph("Road");
        assert_eq!(c, '=');
    }

    #[test]
    fn terrain_glyph_grass() {
        let (c, _) = terrain_glyph("Grass");
        assert_eq!(c, '.');
    }
}
