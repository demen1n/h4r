//! "Pick two armies and fight": a battle sandbox on the combat rules of `h4-rules`
//! (`BattleState`), both sides played by hand. Not the game's battle screen.
//!
//! usage: h4-sandbox [data dir]
//!
//! - `H4_ARMIES="orc:30,troll:4 vs skeleton:40,ghost:8"` — the attacker's and the
//!   defender's stacks (creature keyword : count, up to 7 each);
//! - `H4_SEED` — the battle's random seed (default 1);
//! - `H4_ORDERS="50,40;w;d;52,61"` — orders for the active unit, one after another: a
//!   cell (x,y) is a click, `w` waits, `d` defends; `H4_SCREENSHOT=out.png` saves a frame
//!   after the last order and quits (for checks without a person).
//!
//! Keys: click — move to the cell, or attack the enemy stack in it (shooters shoot);
//! W wait, D defend, G grid, Esc quit.
//!
//! What is the game's: deployment, initiative and turn order, movement points, the path
//! search and the stop, zones of control, the attack position and approach, damage,
//! retaliation, shots and shooting back, facing. What is not: no backdrop, terrain or obstacles (a plain
//! field), no creature abilities (the per-creature ability lists aren't read yet: every
//! stack walks, shooters are the stacks with shots), no shot penalties (range and
//! obstacles are ported but not fed here), morale 0, no heroes, no spells, no AI.

use std::collections::HashMap;

use h4_formats::adv_actor::DIRECTIONS;
use h4_formats::archive::{data_dir, Resources};
use h4_formats::combat_actor::CombatActorModel;
use h4_formats::sprite::{ActorSequence, Rect};
use h4_formats::table::Table;
use h4_rules::battle::deployment::Formation;
use h4_rules::battle::initiative::InitiativeStats;
use h4_rules::battle::melee_command::{ranged_attack, MeleeContext, RangedContext, StackRules};
use h4_rules::battle::retaliation::abilities::RANGED;
use h4_rules::battle::move_command::{approach_and_attack, attack_positions_against, move_unit, AttackOrder, MoveRules};
use h4_rules::battle::move_points::MoveModifiers;
use h4_rules::battle::state::{BattleState, CombatEvent, UnitId, UnitKind};
use h4_rules::battle::zone_of_control::{zones_for, ZoneTraits};
use h4_rules::battle::Side;
use h4_rules::battlefield::{BattlefieldGrid, CellSurface, CombatFootprint, FLAG_10};
use h4_rules::creatures::{self, CreatureStats};
use h4_state::army::{Army, ArmyId, MapArmy, Stack, SLOTS};
use h4_state::Pos;
use macroquad::prelude::*;

const DEFAULT_ARMIES: &str = "orc:30,troll:4 vs skeleton:40,ghost:8";
/// Room for the text above the field.
const TOP: f32 = 150.;

/// A point of the field in cells (x = row, y = column, continuous) on the 1180×1024
/// battlefield picture: the cell center formula of `164b40`, `decomp/battlefield.md`.
fn to_screen(x: f32, y: f32) -> Vec2 {
    vec2(590. + 16. * (y - x), 512. + 8. * (x + y - 102.))
}

/// The cell under a battlefield pixel (the inverse of [`to_screen`]).
fn to_cell(p: Vec2) -> (i32, i32) {
    let u = (p.x - 590.) / 16.; // y - x
    let v = (p.y - 512.) / 8. + 102.; // x + y
    (((v - u) / 2.).floor() as i32, ((v + u) / 2.).floor() as i32)
}

/// One loaded animation: frames with their placement relative to the feet.
struct Animation {
    frames: Vec<Texture2D>,
    area: Rect,
    offset: Vec2,
    fps: f32,
}

/// Combat models and animations, loaded on first use.
struct Sprites<'a> {
    res: &'a Resources,
    models: HashMap<String, Option<CombatActorModel>>,
    animations: HashMap<(String, String, usize), Option<Animation>>,
}

impl<'a> Sprites<'a> {
    fn model(&mut self, keyword: &str) -> Option<&CombatActorModel> {
        let res = self.res;
        self.models
            .entry(keyword.to_lowercase())
            .or_insert_with(|| {
                let data = res.read(&format!("combat_actor.{keyword}.h4d")).ok()?;
                CombatActorModel::read(&data).ok()
            })
            .as_ref()
    }

    /// The first of `actions` the model has, looking toward `facing`.
    fn animation(&mut self, keyword: &str, actions: &[&str], facing: usize) -> Option<&Animation> {
        let key = (keyword.to_lowercase(), actions.join("/"), facing);
        if !self.animations.contains_key(&key) {
            let loaded = self.load(keyword, actions, facing);
            self.animations.insert(key.clone(), loaded);
        }
        self.animations.get(&key).and_then(Option::as_ref)
    }

    fn load(&mut self, keyword: &str, actions: &[&str], facing: usize) -> Option<Animation> {
        let res = self.res;
        let model = self.model(keyword)?;
        let (name, fps) = actions.iter().find_map(|action| {
            let name = model.sequence(action, DIRECTIONS[facing % 8])?;
            let fps = model.action(action).map(|a| f32::from(a.frames_per_second.max(1)))?;
            Some((name, fps))
        })?;
        let sequence = res.read(&name).and_then(|d| ActorSequence::read(&d)).ok()?;
        let area = sequence.bounds()?;
        let (w, h) = (u16::try_from(area.width()).ok()?, u16::try_from(area.height()).ok()?);
        let frames = (0..sequence.frames.len().max(1))
            .map(|i| {
                let t = Texture2D::from_rgba8(w, h, &sequence.compose(i, area, true));
                t.set_filter(FilterMode::Nearest);
                t
            })
            .collect();
        Some(Animation { frames, area, offset: vec2(sequence.offset.0 as f32, sequence.offset.1 as f32), fps })
    }
}

/// `"orc:30,troll:4"` → stacks, by creature keyword.
fn parse_army(text: &str, creatures: &[CreatureStats]) -> Result<Vec<Stack>, String> {
    let mut stacks = Vec::new();
    for item in text.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, count) = item.rsplit_once(':').ok_or(format!("{item:?}: expected keyword:count"))?;
        let creature = creatures
            .iter()
            .position(|c| c.keyword.eq_ignore_ascii_case(name.trim()))
            .ok_or(format!("unknown creature {name:?}"))?;
        let count: i32 = count.trim().parse().map_err(|_| format!("{item:?}: bad count"))?;
        if count <= 0 {
            return Err(format!("{item:?}: the count must be positive"));
        }
        stacks.push(Stack { creature, count });
    }
    if stacks.is_empty() || stacks.len() > SLOTS {
        return Err(format!("an army has 1..={SLOTS} stacks"));
    }
    Ok(stacks)
}

fn map_army(id: u32, owner: u8, stacks: &[Stack]) -> MapArmy {
    MapArmy {
        id: ArmyId(id),
        owner: Some(owner),
        at: Pos { level: 0, row: 0, col: 0 },
        army: Army::of(stacks),
        movement_left: [0.; SLOTS],
        growth_remainder: [0; SLOTS],
    }
}

/// The battle with its stacks deployed as in a normal battle, first round started.
fn new_battle(res: &Resources, sprites: &mut Sprites, seed: u32) -> Result<(BattleState, Vec<CreatureStats>), String> {
    let data = res.read("table.creatures.h4d").map_err(|e| e.to_string())?;
    let table = Table::read(&data, "creatures").map_err(|e| e.to_string())?;
    let creatures = creatures::in_enum_order(&creatures::from_table(&table)?)?;
    let spec = std::env::var("H4_ARMIES").unwrap_or_else(|_| DEFAULT_ARMIES.into());
    let (a, d) = spec.split_once(" vs ").ok_or("H4_ARMIES: expected \"<army> vs <army>\"")?;
    let (a, d) = (parse_army(a, &creatures)?, parse_army(d, &creatures)?);
    let mut field = BattlefieldGrid::standard(CellSurface { terrain: 1, flags: FLAG_10 });
    field.block_edges();
    let mut battle = BattleState::from_armies(&map_army(1, 0, &a), &map_army(2, 1, &d), &creatures, field, seed)?;
    // Each stack's footprint is its combat model's diameter.
    let mut sizes = Vec::new();
    for unit in battle.units() {
        let UnitKind::Creature { stats, .. } = &unit.kind else {
            return Err("the sandbox has no heroes".into());
        };
        let model = sprites.model(&stats.keyword).ok_or(format!("no combat model for {}", stats.keyword))?;
        sizes.push((unit.id, model.footprint_size()));
    }
    battle.deploy_initial([Formation::Loose, Formation::Loose], &sizes)?;
    Ok((battle, creatures))
}

/// Start the next turn, beginning a new round when the last one is over.
fn next_turn(battle: &mut BattleState) -> Result<Option<UnitId>, String> {
    if let Some(unit) = battle.begin_next_turn()? {
        return Ok(Some(unit));
    }
    // Speeds from the table; morale 0 (no heroes, no morale rules here).
    let stats: Vec<_> = battle
        .units()
        .iter()
        .map(|u| {
            let speed = match &u.kind {
                UnitKind::Creature { stats, .. } => stats.speed,
                UnitKind::Hero(_) => 0,
            };
            (u.id, InitiativeStats { speed, morale: 0 })
        })
        .collect();
    battle.begin_initiative_round(&stats)?;
    battle.begin_next_turn()
}

/// The side that has lost every stack, if any.
fn loser(battle: &BattleState) -> Option<Side> {
    [Side::Attacker, Side::Defender]
        .into_iter()
        .find(|&side| !battle.units().iter().any(|u| u.side == side && u.alive()))
}

/// The living unit whose footprint covers a cell.
fn unit_at(battle: &BattleState, cell: (i32, i32)) -> Option<UnitId> {
    battle.units().iter().find_map(|u| {
        let p = u.position?;
        (u.alive() && CombatFootprint { size: p.size }.contains(cell.0 - p.at.0, cell.1 - p.at.1)).then_some(u.id)
    })
}

fn shots(battle: &BattleState, id: UnitId) -> i32 {
    match battle.unit(id).map(|u| &u.kind) {
        Some(UnitKind::Creature { shots, .. }) => *shots,
        _ => 0,
    }
}

/// Carry out a click of the active unit on `cell`: shoot or attack an enemy there,
/// otherwise walk. The turn ends when something happened.
fn click(battle: &mut BattleState, active: UnitId, cell: (i32, i32)) -> Result<String, String> {
    let movement = battle.movement_points(active, &MoveModifiers::default())?;
    let rules = MoveRules::plain(movement);
    let side = battle.unit(active).ok_or("no active unit")?.side;
    let zones = || zones_for(battle, active, |_| ZoneTraits::default());
    let under = unit_at(battle, cell);
    if under == Some(active) {
        return Err("this is where the unit stands (W waits, D defends)".into());
    }
    let target = under.filter(|&t| battle.unit(t).is_some_and(|u| u.side != side));
    let text = if let Some(target) = target {
        if shots(battle, active) > 0 {
            // Abilities aren't read yet: a stack with shots is a shooter, so it can
            // shoot back. No range penalties and no threat map here.
            let rules = |id| StackRules {
                abilities: if shots(battle, id) > 0 { vec![RANGED] } else { Vec::new() },
                ..Default::default()
            };
            let ctx = RangedContext {
                attacker: rules(active),
                target: rules(target),
                penalty: 1,
                return_penalty: 1,
                ..Default::default()
            };
            ranged_attack(battle, active, target, &ctx)?
                .shots
                .iter()
                .map(|s| {
                    let s = &s.strike;
                    format!("{:?} shot{} {} dmg, {} killed", s.by, if s.retaliation { " (answer)" } else { "" }, s.damage, s.losses.killed)
                })
                .collect::<Vec<_>>()
                .join("; ")
        } else {
            let zone = zones()?;
            let positions = attack_positions_against(battle, active, target, &rules, &zone)?;
            // The cheapest place to strike from; the interface would follow the mouse.
            let stand_at = positions.iter().min_by_key(|p| p.cost).ok_or("no place to strike from")?.at;
            let zone = zones()?;
            match approach_and_attack(battle, active, target, stand_at, &rules, zone, &MeleeContext::default())? {
                AttackOrder::Attacked { melee, .. } => melee
                    .strikes
                    .iter()
                    .map(|s| format!("{:?}{} {} dmg, {} killed", s.by, if s.retaliation { " (answer)" } else { "" }, s.damage, s.losses.killed))
                    .collect::<Vec<_>>()
                    .join("; "),
                AttackOrder::Approached(report) => format!("approached to {:?}", report.to),
            }
        }
    } else {
        let size = battle.unit(active).and_then(|u| u.position).ok_or("not on the field")?.size;
        // The click is the center the unit should stand on: its corner is half a size back.
        let half = i32::from(size) / 2;
        let zone = zones()?;
        let report = move_unit(battle, active, (cell.0 - half, cell.1 - half), &rules, zone)?;
        format!("moved to {:?}, cost {}", report.to, report.cost)
    };
    battle.finish_turn()?;
    Ok(text)
}

fn name(battle: &BattleState, id: UnitId) -> String {
    match battle.unit(id).map(|u| &u.kind) {
        Some(UnitKind::Creature { stats, count, .. }) => format!("{} ×{count}", stats.keyword),
        _ => format!("unit {}", id.0),
    }
}

fn config() -> Conf {
    Conf { window_title: "Heroes IV — battle sandbox".into(), window_width: 1180, window_height: 900, ..Default::default() }
}

#[macroquad::main(config)]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let res = match Resources::open(data_dir(std::env::args().nth(1))) {
        Ok(res) => res,
        Err(e) => return log::error!("{e}"),
    };
    let mut sprites = Sprites { res: &res, models: HashMap::new(), animations: HashMap::new() };
    let seed = std::env::var("H4_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(1);
    let (mut battle, _creatures) = match new_battle(&res, &mut sprites, seed) {
        Ok(b) => b,
        Err(e) => return log::error!("{e}"),
    };
    // Where everybody stands, for writing H4_ORDERS.
    for unit in battle.units() {
        if let Some(p) = unit.position {
            println!("{:?} {}: corner {:?}, size {}", unit.side, name(&battle, unit.id), p.at, p.size);
        }
    }
    let mut orders: Vec<String> = std::env::var("H4_ORDERS")
        .map(|s| s.split(';').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect())
        .unwrap_or_default();
    orders.reverse(); // popped from the end
    let screenshot = std::env::var("H4_SCREENSHOT").ok();
    let mut grid = false;
    let mut log_lines: Vec<String> = Vec::new();
    let mut seen_events = 0;
    let mut clock = 0.;
    let mut frames_after_orders = 0;
    loop {
        if is_key_pressed(KeyCode::Escape) {
            break;
        }
        if is_key_pressed(KeyCode::G) {
            grid = !grid;
        }
        clock += get_frame_time();
        let scale = (screen_width() / 1180.).min((screen_height() - TOP) / 1024.).max(0.01);
        let origin = vec2((screen_width() - 1180. * scale) / 2., TOP);
        let over = loser(&battle);
        // Make sure someone is acting.
        if over.is_none() && battle.turns().active.is_none() {
            if let Err(e) = next_turn(&mut battle) {
                log_lines.push(format!("error: {e}"));
            }
        }
        let active = battle.turns().active;
        // One order per frame: from H4_ORDERS, the keyboard or the mouse.
        let order = if let Some(order) = orders.pop() {
            Some(order)
        } else if is_key_pressed(KeyCode::W) {
            Some("w".into())
        } else if is_key_pressed(KeyCode::D) {
            Some("d".into())
        } else if is_mouse_button_pressed(MouseButton::Left) && mouse_position().1 >= TOP {
            let (mx, my) = mouse_position();
            let (x, y) = to_cell((vec2(mx, my) - origin) / scale);
            Some(format!("{x},{y}"))
        } else {
            None
        };
        if let (Some(order), Some(active), None) = (order, active, over) {
            let who = name(&battle, active);
            let result = match order.as_str() {
                "w" => battle.wait().map(|_| "waits".to_string()),
                "d" => battle.defend().map(|_| "defends".to_string()),
                cell => match cell.split_once(',').map(|(x, y)| (x.parse::<i32>(), y.parse::<i32>())) {
                    Some((Ok(x), Ok(y))) => click(&mut battle, active, (x, y)),
                    _ => Err(format!("unknown order {cell:?}")),
                },
            };
            let line = match result {
                Ok(text) => format!("{who}: {text}"),
                Err(e) => format!("{who}: {e}"),
            };
            println!("{line}");
            log_lines.push(line);
        }
        for event in &battle.events()[seen_events..] {
            if !matches!(event, CombatEvent::Rolled { .. }) {
                log::debug!("{event:?}");
            }
        }
        seen_events = battle.events().len();

        clear_background(Color::from_rgba(52, 70, 44, 255));
        // The field: the diamond of cells, lighter where the active unit could walk.
        let field = battle.field();
        for x in 0..field.row_count() as i32 {
            let Some((start, end)) = field.row_bounds(x) else { continue };
            for y in start..end {
                let blocked = field.is_blocked(x, y, None);
                if !grid && !blocked {
                    continue;
                }
                let c = origin + to_screen(x as f32 + 0.5, y as f32 + 0.5) * scale;
                let color = if blocked { Color::new(0., 0., 0., 0.35) } else { Color::new(1., 1., 1., 0.12) };
                let corners = [vec2(-16., 0.), vec2(0., -8.), vec2(16., 0.), vec2(0., 8.)].map(|v| c + v * scale);
                if blocked {
                    draw_triangle(corners[0], corners[1], corners[2], color);
                    draw_triangle(corners[0], corners[2], corners[3], color);
                } else {
                    for i in 0..4 {
                        let (a, b) = (corners[i], corners[(i + 1) % 4]);
                        draw_line(a.x, a.y, b.x, b.y, 1., color);
                    }
                }
            }
        }
        // Units, far to near (by the screen y of their centers).
        let mut units: Vec<_> = battle.units().iter().filter(|u| u.alive() && u.position.is_some()).collect();
        units.sort_by(|a, b| {
            let depth = |u: &&h4_rules::battle::state::BattleUnit| {
                let p = u.position.expect("filtered above");
                p.at.0 + p.at.1 + i32::from(p.size)
            };
            depth(a).cmp(&depth(b))
        });
        for unit in units {
            let p = unit.position.expect("filtered above");
            let half = f32::from(p.size) / 2.;
            let feet = to_screen(p.at.0 as f32 + half, p.at.1 as f32 + half);
            let center = origin + feet * scale;
            let is_active = Some(unit.id) == active;
            // The footprint, colored by side.
            let tint = match unit.side {
                Side::Attacker => Color::new(0.9, 0.3, 0.2, if is_active { 0.55 } else { 0.25 }),
                Side::Defender => Color::new(0.2, 0.4, 0.9, if is_active { 0.55 } else { 0.25 }),
            };
            for (dx, dy) in (CombatFootprint { size: p.size }).cells() {
                let c = origin + to_screen((p.at.0 + dx) as f32 + 0.5, (p.at.1 + dy) as f32 + 0.5) * scale;
                let corners = [vec2(-16., 0.), vec2(0., -8.), vec2(16., 0.), vec2(0., 8.)].map(|v| c + v * scale);
                draw_triangle(corners[0], corners[1], corners[2], tint);
                draw_triangle(corners[0], corners[2], corners[3], tint);
            }
            if let UnitKind::Creature { stats, count, .. } = &unit.kind {
                if let Some(anim) = sprites.animation(&stats.keyword, &["wait", "fidget", "melee"], usize::from(unit.facing)) {
                    let frame = (clock * anim.fps) as usize % anim.frames.len();
                    let top_left = feet + anim.offset + vec2(anim.area.left as f32, anim.area.top as f32);
                    let at = origin + top_left * scale;
                    draw_texture_ex(&anim.frames[frame], at.x, at.y, WHITE, DrawTextureParams {
                        dest_size: Some(vec2(anim.area.width() as f32, anim.area.height() as f32) * scale),
                        ..Default::default()
                    });
                }
                let label = format!("{count}");
                draw_rectangle(center.x - 14., center.y + 4., 28., 16., Color::new(0., 0., 0., 0.7));
                draw_text(&label, center.x - 11., center.y + 17., 18., WHITE);
            }
        }
        // Text.
        let headline = match (over, active) {
            (Some(side), _) => format!("Battle over: the {side:?} side lost every stack."),
            (None, Some(a)) => format!(
                "Round {} - {} ({:?}): click a cell to move, an enemy to attack{}; W wait, D defend, G grid",
                battle.turns().round,
                name(&battle, a),
                battle.unit(a).map(|u| u.side).unwrap_or(Side::Attacker),
                if shots(&battle, a) > 0 { " (shoots)" } else { "" }
            ),
            (None, None) => "No one can act.".into(),
        };
        draw_text(&headline, 10., 22., 20., WHITE);
        for (i, line) in log_lines.iter().rev().take(5).enumerate() {
            draw_text(line, 10., 48. + 20. * i as f32, 18., LIGHTGRAY);
        }
        if orders.is_empty() {
            frames_after_orders += 1;
            if frames_after_orders == 5 {
                if let Some(path) = &screenshot {
                    get_screen_data().export_png(path);
                    break;
                }
            }
        }
        next_frame().await;
    }
}
