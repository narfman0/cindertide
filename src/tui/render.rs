//! TUI rendering — pure layout + style; reads from a TuiApp snapshot.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::state::{Screen, TuiApp, UnitSummary, BuildingSummary, TileSummary, InputMode, BUILD_MENU_OPTIONS};

pub fn draw(f: &mut Frame, app: &TuiApp) {
    match &app.screen {
        Screen::Title { selected } => draw_title(f, *selected),
        Screen::FactionPicker { selected } => draw_faction_picker(f, *selected, app.snapshot.handler_unlocked),
        Screen::LoadPicker { selected, slots } => draw_load_picker(f, *selected, slots),
        Screen::Campaign => draw_campaign(f, app),
        Screen::Briefing { title, briefing } => draw_briefing(f, title, briefing),
        Screen::InMission => draw_in_mission(f, app),
        Screen::Debrief { title, text, won } => draw_debrief(f, title, text, *won),
        Screen::GameOver { won, handler_unlocked } => draw_game_over(f, *won, *handler_unlocked, app.snapshot.finale_text.as_deref()),
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

fn draw_faction_picker(f: &mut Frame, selected: usize, handler_unlocked: bool) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Choose Faction ");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut factions: Vec<(&str, &str, Color)> = vec![
        ("Combine",  "Industrial expansion: 1 Bunker, 1 Refinery, 1 Barracks, 4 Riflemen", Color::Yellow),
        ("Ironborn", "Aggressive salvage: 1 Foundry, 5 Riflemen",                          Color::Gray),
    ];
    if handler_unlocked {
        factions.push(("The Architect", "A different kind of war. Five sites. A ritual. You already know how it ends.", Color::Cyan));
    }
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

fn draw_briefing(f: &mut Frame, title: &str, briefing: &str) {
    let area = f.area();
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Briefing: {} ", title));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(2)])
        .split(inner);

    let lines: Vec<Line> = std::iter::once(Line::from(""))
        .chain(briefing.lines().map(|l| Line::from(format!("  {}", l))))
        .collect();

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), chunks[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Enter deploy   Esc back",
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[1],
    );
}

fn draw_debrief(f: &mut Frame, title: &str, text: &str, won: bool) {
    let area = f.area();
    let result_label = if won { "MISSION COMPLETE" } else { "MISSION FAILED" };
    let result_color = if won { Color::Green } else { Color::Red };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} — {} ", title, result_label));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(6), Constraint::Length(2)])
        .split(inner);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("  {}", result_label),
            Style::default().fg(result_color).add_modifier(Modifier::BOLD),
        ))),
        chunks[0],
    );

    let lines: Vec<Line> = std::iter::once(Line::from(""))
        .chain(text.lines().map(|l| Line::from(format!("  {}", l))))
        .collect();
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), chunks[1]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "  Enter continue",
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[2],
    );
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

fn draw_campaign(f: &mut Frame, app: &TuiApp) {
    let area = f.area();
    let current = app.snapshot.current_mission_index.unwrap_or(0);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(
            " Campaign — {}   Mission {} / 5   Won: {}   Lost: {} ",
            app.snapshot.player.as_deref().unwrap_or("?"),
            current,
            app.snapshot.missions_won,
            app.snapshot.missions_lost,
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Min(4), Constraint::Length(2)])
        .split(inner);

    // Past mission outcomes
    let mut history_lines: Vec<Line> = vec![Line::from(Span::styled(
        " Mission History:",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if app.snapshot.past_outcomes.is_empty() {
        history_lines.push(Line::from(Span::styled(
            "  (no missions completed yet)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for o in &app.snapshot.past_outcomes {
            let result_str = if o.won { "WON" } else { "LOST" };
            let result_color = if o.won { Color::Green } else { Color::Red };
            history_lines.push(Line::from(vec![
                Span::raw(format!("  Mission {} — {:<12} ", o.mission_index + 1, o.mission_type)),
                Span::styled(result_str, Style::default().fg(result_color).add_modifier(Modifier::BOLD)),
            ]));
        }
    }
    f.render_widget(Paragraph::new(history_lines), chunks[0]);

    // Next mission
    let mut next_lines: Vec<Line> = vec![Line::from(Span::styled(
        " Next Mission:",
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    match &app.snapshot.current_mission_type {
        Some(mt) => {
            next_lines.push(Line::from(Span::styled(
                format!("  > Mission {} — {}   (Enter to start)", current + 1, mt),
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
            )));
        }
        None => {
            next_lines.push(Line::from(Span::styled(
                "  Campaign complete!",
                Style::default().fg(Color::Green),
            )));
        }
    }
    f.render_widget(Paragraph::new(next_lines), chunks[1]);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            " Enter start mission   s save   Esc title   q quit",
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

    let hint = match (&app.input_mode, app.selected_unit) {
        (InputMode::Normal, None) => " Space pause  hjkl move cursor  Enter select unit  b build  Esc abandon",
        (InputMode::Normal, Some(_)) => " m move  a attack  b build  Enter reselect  Esc deselect",
        (InputMode::Moving, _) => " hjkl move target  Enter confirm move  Esc cancel",
        (InputMode::Attacking, _) => " hjkl move to enemy  Enter confirm attack  Esc cancel",
        (InputMode::Building, _) => " ↑↓ pick type  hjkl place cursor  Enter build  Esc cancel",
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray)))),
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

    let selected_pos: Option<(i32, i32)> = app.selected_unit.and_then(|id| {
        app.snapshot.units.iter().find(|u| u.entity_id == id).map(|u| (u.x, u.y))
    });
    let (cx, cy) = app.cursor;
    let cursor_bg = match app.input_mode {
        InputMode::Moving   => Color::Green,
        InputMode::Attacking => Color::Red,
        _                    => Color::DarkGray,
    };

    let lines: Vec<Line> = grid
        .iter()
        .enumerate()
        .take(area.height as usize)
        .map(|(row_idx, row)| {
            let spans: Vec<Span> = row
                .iter()
                .enumerate()
                .take(area.width as usize)
                .map(|(col_idx, (c, color))| {
                    let rx = col_idx as i32;
                    let ry = row_idx as i32;
                    let style = if Some((rx, ry)) == selected_pos {
                        Style::default().fg(*color).bg(Color::Blue)
                    } else if rx == cx && ry == cy {
                        Style::default().fg(*color).bg(cursor_bg)
                    } else {
                        Style::default().fg(*color)
                    };
                    Span::styled(c.to_string(), style)
                })
                .collect();
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), area);

    if app.build_menu_open {
        draw_build_menu(f, app, area);
    }
}

fn draw_build_menu(f: &mut Frame, app: &TuiApp, area: Rect) {
    let menu_w = 18u16;
    let menu_h = BUILD_MENU_OPTIONS.len() as u16 + 2;
    let x = area.x;
    let y = area.y + area.height.saturating_sub(menu_h + 1);
    let menu_area = Rect::new(x, y, menu_w.min(area.width), menu_h.min(area.height));

    let block = Block::default().borders(Borders::ALL).title(" Build ");
    let inner = block.inner(menu_area);
    f.render_widget(block, menu_area);

    let lines: Vec<Line> = BUILD_MENU_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let style = if i == app.build_menu_selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD).bg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(Span::styled(format!(" {}", name), style))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_game_over(f: &mut Frame, won: bool, handler_unlocked: bool, finale_text: Option<&str>) {
    let area = f.area();
    let title = if won { " Victory " } else { " Defeat " };
    let color = if won { Color::Green } else { Color::Red };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = if won { "CAMPAIGN COMPLETE" } else { "DEFEAT" };
    let mut lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            format!("    {}", text),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    if let Some(ft) = finale_text {
        lines.push(Line::from(""));
        for l in ft.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {}", l),
                Style::default().fg(Color::Gray),
            )));
        }
        lines.push(Line::from(""));
    }
    if handler_unlocked {
        lines.push(Line::from(Span::styled(
            "    *** THE ARCHITECT UNLOCKED — a new perspective awaits ***",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "    Enter / Esc — title screen",
        Style::default().fg(Color::DarkGray),
    )));
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
