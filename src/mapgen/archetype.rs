// S-expression archetype system for procedural map generation.
// No external crates — parser implemented from scratch.

use std::collections::HashMap;
use crate::map::TerrainType;

// ---------------------------------------------------------------------------
// S-expression data type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Sexpr {
    Atom(String),
    Number(f32),
    List(Vec<Sexpr>),
}

// ---------------------------------------------------------------------------
// S-expression parser
// ---------------------------------------------------------------------------

pub fn parse_sexpr(input: &str) -> Result<Vec<Sexpr>, String> {
    let mut chars = input.chars().peekable();
    let mut results = Vec::new();
    loop {
        skip_whitespace_and_comments(&mut chars);
        if chars.peek().is_none() {
            break;
        }
        let expr = parse_one(&mut chars)?;
        results.push(expr);
    }
    Ok(results)
}

fn skip_whitespace_and_comments(chars: &mut std::iter::Peekable<std::str::Chars>) {
    loop {
        match chars.peek() {
            Some(&c) if c.is_whitespace() => { chars.next(); }
            Some(&';') => {
                // Skip to end of line
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c == '\n' { break; }
                }
            }
            _ => break,
        }
    }
}

fn parse_one(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Sexpr, String> {
    skip_whitespace_and_comments(chars);
    match chars.peek() {
        Some(&'(') => parse_list(chars),
        Some(&'"') => parse_string(chars),
        Some(_) => parse_atom_or_number(chars),
        None => Err("unexpected end of input".to_string()),
    }
}

fn parse_list(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Sexpr, String> {
    // consume '('
    chars.next();
    let mut items = Vec::new();
    loop {
        skip_whitespace_and_comments(chars);
        match chars.peek() {
            Some(&')') => { chars.next(); break; }
            None => return Err("unterminated list".to_string()),
            _ => {
                items.push(parse_one(chars)?);
            }
        }
    }
    Ok(Sexpr::List(items))
}

fn parse_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Sexpr, String> {
    chars.next(); // consume '"'
    let mut s = String::new();
    loop {
        match chars.next() {
            Some('"') => break,
            Some('\\') => {
                match chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some(c) => s.push(c),
                    None => return Err("unterminated string escape".to_string()),
                }
            }
            Some(c) => s.push(c),
            None => return Err("unterminated string".to_string()),
        }
    }
    Ok(Sexpr::Atom(s))
}

fn parse_atom_or_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Sexpr, String> {
    let mut s = String::new();
    loop {
        match chars.peek() {
            Some(&c) if !c.is_whitespace() && c != '(' && c != ')' && c != '"' && c != ';' => {
                s.push(c);
                chars.next();
            }
            _ => break,
        }
    }
    if s.is_empty() {
        return Err("empty atom".to_string());
    }
    // Try parsing as number
    if let Ok(f) = s.parse::<f32>() {
        Ok(Sexpr::Number(f))
    } else {
        Ok(Sexpr::Atom(s))
    }
}

// ---------------------------------------------------------------------------
// Archetype data model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ArchetypeDef {
    pub id: String,
    pub name: String,
    pub description: String,
    pub params: Vec<ParamDef>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct ParamDef {
    pub name: String,
    pub default: f32,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone)]
pub enum Step {
    Fill(TerrainType),
    River { at: RiverAt, meander: CountParam, width: CountParam },
    Bridges { count: CountParam, terrain: TerrainType },
    Forests { count_param: CountParam, placement: Placement },
    Rubble { count_param: CountParam, placement: Placement },
    UrbanRuins { density_param: CountParam },
    Ridgelines { count_param: CountParam },
    Islands { count_param: CountParam },
    Bases { player: Corner, enemy: Corner, clear_radius: i32 },
    Resources { count_param: CountParam, placement: Placement },
}

/// A count that is either a literal or a param name reference.
#[derive(Debug, Clone)]
pub enum CountParam {
    Literal(f32),
    ParamRef(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RiverAt { CenterY, CenterX }

#[derive(Debug, Clone, PartialEq)]
pub enum Placement { Flanks, Center, Random, Scattered }

#[derive(Debug, Clone, PartialEq)]
pub enum Corner { BottomLeft, TopRight, BottomRight, TopLeft }

// ---------------------------------------------------------------------------
// Archetype def parser
// ---------------------------------------------------------------------------

fn atom_str(s: &Sexpr) -> Option<&str> {
    if let Sexpr::Atom(a) = s { Some(a.as_str()) } else { None }
}

fn num_val(s: &Sexpr) -> Option<f32> {
    match s {
        Sexpr::Number(f) => Some(*f),
        Sexpr::Atom(a) => a.parse::<f32>().ok(),
        _ => None,
    }
}

fn parse_terrain(s: &str) -> Option<TerrainType> {
    match s {
        "Grass" | "grass" => Some(TerrainType::Grass),
        "Road" | "road" => Some(TerrainType::Road),
        "Forest" | "forest" => Some(TerrainType::Forest),
        "Rubble" | "rubble" => Some(TerrainType::Rubble),
        "Mud" | "mud" => Some(TerrainType::Mud),
        "Corrupted" | "corrupted" => Some(TerrainType::Corrupted),
        _ => None,
    }
}

fn parse_placement(s: &str) -> Placement {
    match s {
        "flanks" => Placement::Flanks,
        "center" => Placement::Center,
        "random" => Placement::Random,
        _ => Placement::Scattered,
    }
}

fn parse_corner(s: &str) -> Corner {
    match s {
        "bottom-left" => Corner::BottomLeft,
        "top-right" => Corner::TopRight,
        "bottom-right" => Corner::BottomRight,
        "top-left" => Corner::TopLeft,
        _ => Corner::BottomLeft,
    }
}

/// Parse a count-or-param-name from a Sexpr.
fn parse_count_param(s: &Sexpr) -> CountParam {
    match s {
        Sexpr::Number(f) => CountParam::Literal(*f),
        Sexpr::Atom(a) => {
            if let Ok(f) = a.parse::<f32>() {
                CountParam::Literal(f)
            } else {
                CountParam::ParamRef(a.clone())
            }
        }
        _ => CountParam::Literal(1.0),
    }
}

fn resolve_count(cp: &CountParam, params: &HashMap<String, f32>) -> usize {
    let v = match cp {
        CountParam::Literal(f) => *f,
        CountParam::ParamRef(name) => *params.get(name).unwrap_or(&1.0),
    };
    v.round().max(0.0) as usize
}

fn resolve_f32(cp: &CountParam, params: &HashMap<String, f32>) -> f32 {
    match cp {
        CountParam::Literal(f) => *f,
        CountParam::ParamRef(name) => *params.get(name).unwrap_or(&0.0),
    }
}

pub fn parse_archetype_def(sexprs: &[Sexpr]) -> Result<ArchetypeDef, String> {
    // Expect (archetype id ...)
    let top = sexprs.first().ok_or("empty input")?;
    let items = if let Sexpr::List(items) = top { items } else {
        return Err("expected top-level list".to_string());
    };

    if items.is_empty() {
        return Err("empty archetype list".to_string());
    }
    let head = atom_str(&items[0]).ok_or("expected 'archetype' symbol")?;
    if head != "archetype" {
        return Err(format!("expected 'archetype', got '{head}'"));
    }

    let id = atom_str(items.get(1).ok_or("missing id")?)
        .ok_or("id must be atom")?
        .to_string();

    let mut name = String::new();
    let mut description = String::new();
    let mut params = Vec::new();
    let mut steps = Vec::new();

    for item in &items[2..] {
        let sub = if let Sexpr::List(sub) = item { sub } else { continue };
        if sub.is_empty() { continue; }
        match atom_str(&sub[0]).unwrap_or("") {
            "name" => {
                if let Some(s) = sub.get(1).and_then(|x| atom_str(x)) {
                    name = s.to_string();
                }
            }
            "description" => {
                if let Some(s) = sub.get(1).and_then(|x| atom_str(x)) {
                    description = s.to_string();
                }
            }
            "params" => {
                for param_item in &sub[1..] {
                    if let Sexpr::List(ps) = param_item {
                        if ps.len() >= 2 {
                            let pname = atom_str(&ps[0]).unwrap_or("").to_string();
                            let default = num_val(&ps[1]).unwrap_or(0.0);
                            let mut min = 0.0f32;
                            let mut max = 100.0f32;
                            // Parse :min and :max keywords
                            let mut i = 2;
                            while i + 1 < ps.len() {
                                match atom_str(&ps[i]).unwrap_or("") {
                                    ":min" => { min = num_val(&ps[i+1]).unwrap_or(0.0); }
                                    ":max" => { max = num_val(&ps[i+1]).unwrap_or(100.0); }
                                    _ => {}
                                }
                                i += 2;
                            }
                            params.push(ParamDef { name: pname, default, min, max });
                        }
                    }
                }
            }
            "steps" => {
                for step_item in &sub[1..] {
                    if let Some(step) = parse_step(step_item) {
                        steps.push(step);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(ArchetypeDef { id, name, description, params, steps })
}

fn parse_step(item: &Sexpr) -> Option<Step> {
    let sub = if let Sexpr::List(sub) = item { sub } else { return None };
    if sub.is_empty() { return None; }
    match atom_str(&sub[0]).unwrap_or("") {
        "fill" => {
            let terrain_name = atom_str(sub.get(1)?)?;
            let terrain = parse_terrain(terrain_name)?;
            Some(Step::Fill(terrain))
        }
        "river" => {
            let mut at = RiverAt::CenterY;
            let mut meander = CountParam::Literal(0.4);
            let mut width = CountParam::Literal(2.0);
            let mut i = 1;
            while i < sub.len() {
                match atom_str(&sub[i]).unwrap_or("") {
                    ":at" => {
                        if let Some(s) = sub.get(i+1).and_then(|x| atom_str(x)) {
                            at = if s == "center-x" { RiverAt::CenterX } else { RiverAt::CenterY };
                        }
                        i += 2;
                    }
                    ":meander" => {
                        if let Some(next) = sub.get(i+1) {
                            meander = parse_count_param(next);
                        }
                        i += 2;
                    }
                    ":width" => {
                        if let Some(next) = sub.get(i+1) {
                            width = parse_count_param(next);
                        }
                        i += 2;
                    }
                    _ => { i += 1; }
                }
            }
            Some(Step::River { at, meander, width })
        }
        "bridges" => {
            let count = sub.get(1).map(parse_count_param).unwrap_or(CountParam::Literal(2.0));
            let mut terrain = TerrainType::Road;
            let mut i = 2;
            while i < sub.len() {
                if atom_str(&sub[i]).unwrap_or("") == ":terrain" {
                    if let Some(s) = sub.get(i+1).and_then(|x| atom_str(x)) {
                        terrain = parse_terrain(s).unwrap_or(TerrainType::Road);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Some(Step::Bridges { count, terrain })
        }
        "forests" => {
            let count_param = sub.get(1).map(parse_count_param).unwrap_or(CountParam::Literal(3.0));
            let mut placement = Placement::Scattered;
            let mut i = 2;
            while i < sub.len() {
                if atom_str(&sub[i]).unwrap_or("") == ":placement" {
                    if let Some(s) = sub.get(i+1).and_then(|x| atom_str(x)) {
                        placement = parse_placement(s);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Some(Step::Forests { count_param, placement })
        }
        "rubble" => {
            let count_param = sub.get(1).map(parse_count_param).unwrap_or(CountParam::Literal(2.0));
            let mut placement = Placement::Scattered;
            let mut i = 2;
            while i < sub.len() {
                if atom_str(&sub[i]).unwrap_or("") == ":placement" {
                    if let Some(s) = sub.get(i+1).and_then(|x| atom_str(x)) {
                        placement = parse_placement(s);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Some(Step::Rubble { count_param, placement })
        }
        "urban-ruins" => {
            let density_param = sub.get(1).map(parse_count_param).unwrap_or(CountParam::Literal(0.35));
            Some(Step::UrbanRuins { density_param })
        }
        "ridgelines" => {
            let count_param = sub.get(1).map(parse_count_param).unwrap_or(CountParam::Literal(2.0));
            Some(Step::Ridgelines { count_param })
        }
        "islands" => {
            let count_param = sub.get(1).map(parse_count_param).unwrap_or(CountParam::Literal(4.0));
            Some(Step::Islands { count_param })
        }
        "bases" => {
            let mut player = Corner::BottomLeft;
            let mut enemy = Corner::TopRight;
            let mut clear_radius = 6i32;
            let mut i = 1;
            while i < sub.len() {
                match atom_str(&sub[i]).unwrap_or("") {
                    ":player" => {
                        if let Some(s) = sub.get(i+1).and_then(|x| atom_str(x)) {
                            player = parse_corner(s);
                        }
                        i += 2;
                    }
                    ":enemy" => {
                        if let Some(s) = sub.get(i+1).and_then(|x| atom_str(x)) {
                            enemy = parse_corner(s);
                        }
                        i += 2;
                    }
                    ":clear-radius" => {
                        if let Some(next) = sub.get(i+1) {
                            clear_radius = num_val(next).unwrap_or(6.0) as i32;
                        }
                        i += 2;
                    }
                    _ => { i += 1; }
                }
            }
            Some(Step::Bases { player, enemy, clear_radius })
        }
        "resources" => {
            let count_param = sub.get(1).map(parse_count_param).unwrap_or(CountParam::Literal(6.0));
            let mut placement = Placement::Scattered;
            let mut i = 2;
            while i < sub.len() {
                if atom_str(&sub[i]).unwrap_or("") == ":placement" {
                    if let Some(s) = sub.get(i+1).and_then(|x| atom_str(x)) {
                        placement = parse_placement(s);
                    }
                    i += 2;
                } else {
                    i += 1;
                }
            }
            Some(Step::Resources { count_param, placement })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Deterministic RNG — same LCG as mapgen/mod.rs
// ---------------------------------------------------------------------------

fn lcg_next(s: &mut u64) -> u64 {
    *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *s
}

fn hash_noise(x: i32, y: i32, seed: u64) -> u8 {
    let mut h = seed
        .wrapping_add(x as u64 * 2654435761)
        .wrapping_add(y as u64 * 2246822519);
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51afd7ed558ccd);
    h ^= h >> 33;
    h = h.wrapping_mul(0xc4ceb9fe1a85ec53);
    h ^= h >> 33;
    (h & 0xff) as u8
}

// ---------------------------------------------------------------------------
// Corner helpers
// ---------------------------------------------------------------------------

fn corner_pos(c: &Corner, width: i32, height: i32) -> (i32, i32) {
    match c {
        Corner::BottomLeft  => (3, height - 4),
        Corner::TopRight    => (width - 4, 3),
        Corner::BottomRight => (width - 4, height - 4),
        Corner::TopLeft     => (3, 3),
    }
}

// ---------------------------------------------------------------------------
// Interpreter: generate_from_archetype
// ---------------------------------------------------------------------------

pub fn generate_from_archetype(
    def: &ArchetypeDef,
    params: &HashMap<String, f32>,
    width: i32,
    height: i32,
    seed: u64,
) -> HashMap<(i32, i32), TerrainType> {
    // Merge param defaults with overrides
    let mut resolved: HashMap<String, f32> = def.params.iter()
        .map(|p| (p.name.clone(), p.default))
        .collect();
    for (k, v) in params {
        if let Some(val) = resolved.get_mut(k) {
            *val = *v;
        }
    }

    let mut map: HashMap<(i32, i32), TerrainType> = HashMap::new();

    // Track river positions for bridge placement
    let mut river_tiles: Vec<(i32, i32)> = Vec::new();

    for step in &def.steps {
        match step {
            Step::Fill(terrain) => {
                for y in 0..height {
                    for x in 0..width {
                        map.insert((x, y), terrain.clone());
                    }
                }
            }

            Step::River { at, meander, width: river_width } => {
                river_tiles.clear();
                let meander_val = resolve_f32(meander, &resolved);
                let rw = resolve_count(river_width, &resolved).max(1);
                let half_rw = (rw as i32) / 2;
                match at {
                    RiverAt::CenterY => {
                        let cy = height / 2;
                        for x in 0..width {
                            let y_offset = (meander_val * (height as f32 / 4.0)
                                * ((x as f32) * 0.08).sin()).round() as i32;
                            let center_y = cy + y_offset;
                            for dy in -half_rw..=half_rw {
                                let ty = center_y + dy;
                                if ty >= 0 && ty < height {
                                    map.insert((x, ty), TerrainType::Mud);
                                    river_tiles.push((x, ty));
                                }
                            }
                        }
                    }
                    RiverAt::CenterX => {
                        let cx = width / 2;
                        for y in 0..height {
                            let x_offset = (meander_val * (width as f32 / 4.0)
                                * ((y as f32) * 0.08).sin()).round() as i32;
                            let center_x = cx + x_offset;
                            for dx in -half_rw..=half_rw {
                                let tx = center_x + dx;
                                if tx >= 0 && tx < width {
                                    map.insert((tx, y), TerrainType::Mud);
                                    river_tiles.push((tx, y));
                                }
                            }
                        }
                    }
                }
            }

            Step::Bridges { count, terrain } => {
                let bridge_count = resolve_count(count, &resolved);
                if bridge_count == 0 { continue; }
                let interval = width / (bridge_count as i32).max(1);
                for i in 0..bridge_count {
                    let bridge_x = interval / 2 + i as i32 * interval;
                    // Clear a 1-wide vertical crossing at bridge_x
                    for y in 0..height {
                        if river_tiles.contains(&(bridge_x, y)) {
                            map.insert((bridge_x, y), terrain.clone());
                        }
                    }
                }
            }

            Step::Forests { count_param, placement } => {
                let count = resolve_count(count_param, &resolved);
                place_clusters(&mut map, count, placement, width, height, seed ^ 0xF0_0000_0000, 8, 15, TerrainType::Forest);
            }

            Step::Rubble { count_param, placement } => {
                let count = resolve_count(count_param, &resolved);
                place_clusters(&mut map, count, placement, width, height, seed ^ 0xB0_0000_0000, 4, 8, TerrainType::Rubble);
            }

            Step::UrbanRuins { density_param } => {
                let density = resolve_f32(density_param, &resolved).clamp(0.0, 1.0);
                let threshold = ((1.0 - density) * 255.0) as u8;
                for y in 0..height {
                    for x in 0..width {
                        if hash_noise(x, y, seed ^ 0xAB_0000_0000) >= threshold {
                            map.insert((x, y), TerrainType::Rubble);
                        }
                    }
                }
            }

            Step::Ridgelines { count_param } => {
                let count = resolve_count(count_param, &resolved);
                for i in 0..count {
                    let offset_y = height / (count as i32 + 1) * (i as i32 + 1);
                    for x in 0..width {
                        let tilted_y = offset_y + (x as f32 * 0.5) as i32;
                        for dy in -1i32..=1 {
                            let ty = tilted_y + dy;
                            if ty >= 0 && ty < height {
                                let n = hash_noise(x, ty, seed ^ 0xD1_0000_0000);
                                if n < 200 {
                                    map.insert((x, ty), TerrainType::Forest);
                                }
                            }
                        }
                    }
                }
            }

            Step::Islands { count_param } => {
                let count = resolve_count(count_param, &resolved);
                // Divide map into count vertical segments with Road bridges
                let seg_w = width / (count as i32).max(1);
                for seg in 0..count {
                    let start_x = seg as i32 * seg_w;
                    let end_x = (start_x + seg_w).min(width);
                    // Fill segment with Grass (clear from mud fill)
                    for y in 2..height-2 {
                        for x in (start_x+2)..end_x-2 {
                            map.insert((x, y), TerrainType::Grass);
                        }
                    }
                    // Road bridge connecting adjacent segments
                    if seg + 1 < count {
                        let bridge_x = end_x - 1;
                        let mid_y = height / 2;
                        for dy in -1i32..=1 {
                            let ty = mid_y + dy;
                            if ty >= 0 && ty < height {
                                map.insert((bridge_x, ty), TerrainType::Road);
                                map.insert((bridge_x + 1, ty), TerrainType::Road);
                            }
                        }
                    }
                }
            }

            Step::Bases { player, enemy, clear_radius } => {
                let (px, py) = corner_pos(player, width, height);
                let (ex, ey) = corner_pos(enemy, width, height);
                for (bx, by) in [(px, py), (ex, ey)] {
                    for dy in -clear_radius..=*clear_radius {
                        for dx in -clear_radius..=*clear_radius {
                            let tx = bx + dx;
                            let ty = by + dy;
                            if tx >= 0 && ty >= 0 && tx < width && ty < height {
                                let dist = ((dx*dx + dy*dy) as f32).sqrt();
                                if dist <= *clear_radius as f32 {
                                    map.insert((tx, ty), TerrainType::Grass);
                                }
                            }
                        }
                    }
                }
            }

            Step::Resources { count_param, placement } => {
                let count = resolve_count(count_param, &resolved);
                place_resources(&mut map, count, placement, width, height, seed ^ 0xE5_0000_0000);
            }
        }
    }

    map
}

fn place_clusters(
    map: &mut HashMap<(i32, i32), TerrainType>,
    count: usize,
    placement: &Placement,
    width: i32,
    height: i32,
    seed: u64,
    min_size: usize,
    max_size: usize,
    terrain: TerrainType,
) {
    let mut rng = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);

    for _ in 0..count {
        let cx = match placement {
            Placement::Flanks => {
                let v = lcg_next(&mut rng) % 2;
                if v == 0 {
                    (lcg_next(&mut rng) % (width / 4).max(1) as u64) as i32
                } else {
                    (3 * width / 4) + (lcg_next(&mut rng) % (width / 4).max(1) as u64) as i32
                }
            }
            Placement::Center => {
                width / 4 + (lcg_next(&mut rng) % (width / 2).max(1) as u64) as i32
            }
            _ => (lcg_next(&mut rng) % width.max(1) as u64) as i32,
        };
        let cy = (lcg_next(&mut rng) % height.max(1) as u64) as i32;
        let cluster_size = min_size + (lcg_next(&mut rng) % (max_size - min_size + 1).max(1) as u64) as usize;

        for _ in 0..cluster_size {
            let dx = (lcg_next(&mut rng) % 5) as i32 - 2;
            let dy = (lcg_next(&mut rng) % 5) as i32 - 2;
            let tx = cx + dx;
            let ty = cy + dy;
            if tx >= 0 && ty >= 0 && tx < width && ty < height {
                map.insert((tx, ty), terrain.clone());
            }
        }
    }
}

fn place_resources(
    map: &mut HashMap<(i32, i32), TerrainType>,
    count: usize,
    placement: &Placement,
    width: i32,
    height: i32,
    seed: u64,
) {
    let mut rng = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let mut placed: Vec<(i32, i32)> = Vec::new();

    let mut attempts = 0;
    while placed.len() < count && attempts < 500 {
        attempts += 1;
        let x = match placement {
            Placement::Center => width / 4 + (lcg_next(&mut rng) % (width / 2).max(1) as u64) as i32,
            Placement::Flanks => {
                let v = lcg_next(&mut rng) % 2;
                if v == 0 {
                    (lcg_next(&mut rng) % (width / 4).max(1) as u64) as i32
                } else {
                    (3 * width / 4) + (lcg_next(&mut rng) % (width / 4).max(1) as u64) as i32
                }
            }
            _ => 4 + (lcg_next(&mut rng) % (width - 8).max(1) as u64) as i32,
        };
        let y = 4 + (lcg_next(&mut rng) % (height - 8).max(1) as u64) as i32;
        // Keep spread
        let too_close = placed.iter().any(|&(ox, oy)| (x-ox).abs().max((y-oy).abs()) < 6);
        if !too_close {
            placed.push((x, y));
            map.insert((x, y), TerrainType::Corrupted);
        }
    }
}

// ---------------------------------------------------------------------------
// File loading
// ---------------------------------------------------------------------------

pub fn load_archetype(path: &str) -> Result<ArchetypeDef, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let sexprs = parse_sexpr(&text)?;
    parse_archetype_def(&sexprs)
}

pub fn scan_archetypes() -> Vec<ArchetypeDef> {
    let dir = "assets/archetypes";
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut result = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("lisp") {
            if let Some(p) = path.to_str() {
                match load_archetype(p) {
                    Ok(def) => result.push(def),
                    Err(e) => eprintln!("archetype parse error {p}: {e}"),
                }
            }
        }
    }
    // Sort by name for stable ordering
    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_list() {
        let sexprs = parse_sexpr("(fill Grass)").unwrap();
        assert_eq!(sexprs.len(), 1);
    }

    #[test]
    fn parse_with_comment() {
        let sexprs = parse_sexpr("; a comment\n(fill Grass)").unwrap();
        assert_eq!(sexprs.len(), 1);
    }

    #[test]
    fn parse_number() {
        let sexprs = parse_sexpr("0.4").unwrap();
        if let Sexpr::Number(f) = &sexprs[0] {
            assert!((f - 0.4).abs() < 0.001);
        } else {
            panic!("expected Number");
        }
    }

    #[test]
    fn generate_fills_all_tiles() {
        let def = ArchetypeDef {
            id: "test".to_string(),
            name: "Test".to_string(),
            description: String::new(),
            params: Vec::new(),
            steps: vec![Step::Fill(TerrainType::Grass)],
        };
        let map = generate_from_archetype(&def, &HashMap::new(), 20, 10, 42);
        assert_eq!(map.len(), 200);
        assert!(map.values().all(|t| *t == TerrainType::Grass));
    }
}
