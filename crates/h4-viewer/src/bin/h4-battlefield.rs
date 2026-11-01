//! Inspect preset backdrops and combat scenery; this is not a playable battle.
use h4_formats::archive::{data_dir, Resources};
use h4_formats::battlefield::{BattlefieldPreset, BattlefieldRow};
use std::collections::HashMap;

use h4_formats::combat_object::{CombatHeaderEntry, CombatHeaderTable, CombatObject};
use h4_formats::sprite::{ActorSequence, Rect};
use h4_formats::table::Table;
use h4_rules::battlefield::{
    apply_preset_cell, cell_is_blocked, BattlefieldGrid, CellSurface, PlacedObstacle, FLAG_10,
};
use h4_rules::obstacle_placement::{
    place_obstacles, ObstaclePlacement, ObstacleWeights, PlacerContext,
};
use h4_rules::random::CombatRandom;
use macroquad::prelude::*;

/// Ground under the backdrop for automatic obstacles (T cycles): terrain id and name.
/// The game takes it from the adventure map around the battle; not ported yet.
const GROUNDS: [(u8, &str); 9] = [
    (1, "grass"),
    (2, "rough"),
    (3, "swamp"),
    (4, "volcanic"),
    (5, "snow"),
    (6, "sand"),
    (7, "dirt"),
    (8, "subterranean"),
    (0, "water"),
];

/// Cached obstacle headers and the weights table, for the obstacle placer.
fn placer_inputs(res: &Resources) -> Result<(Vec<CombatHeaderEntry>, ObstacleWeights), String> {
    let headers = res
        .read(CombatHeaderTable::RESOURCE)
        .and_then(|b| CombatHeaderTable::read(&b))
        .map_err(|e| e.to_string())?;
    let table = res
        .read("table.combat_obstacles.h4d")
        .and_then(|b| Table::read(&b, "combat_obstacles"))
        .map_err(|e| e.to_string())?;
    Ok((headers.entries, ObstacleWeights::from_table(&table)))
}

/// Draw one frame of a scenery model with its image anchored at `anchor`
/// (battlefield pixels).
fn draw_scenery(item: &SceneryPart, anchor: Vec2, origin: Vec2, scale: f32, elapsed: f32) {
    let index = (elapsed * item.fps) as usize % item.frames.len();
    let pos =
        origin + (anchor + item.offset + vec2(item.area.left as f32, item.area.top as f32)) * scale;
    draw_texture_ex(
        &item.frames[index],
        pos.x,
        pos.y,
        WHITE,
        DrawTextureParams {
            dest_size: Some(vec2(item.area.width() as f32, item.area.height() as f32) * scale),
            ..Default::default()
        },
    );
}

// Every (x, y) the viewer iterates comes from the preset's stored row bounds,
// which lie inside the 102×102 field, so cell lookups cannot fail.
const STORED_CELL: &str = "coordinates come from stored preset rows";

// Zero inherits earlier overlays on land, but blocks outside the bridge deck.
// Scenery shown in this resource viewer is excluded from the cell query.
fn preset_status(value: u8, on_bridge: bool) -> (&'static str, Color) {
    if value == 0 && !on_bridge {
        return ("inherited from underlying field", ORANGE);
    }
    if !matches!(value, 0 | 1 | 2 | 255) {
        return ("unrecognized overlay (unchanged)", GRAY);
    }
    let mut cell = CellSurface::default();
    apply_preset_cell(&mut cell, value, on_bridge);
    if cell_is_blocked(cell.flags, &[], None) {
        ("blocked by preset", RED)
    } else {
        ("cleared by preset (occupants excluded)", GREEN)
    }
}

struct Backdrop {
    texture: Texture2D,
    area: Rect,
}
struct SceneryPart {
    /// Position relative to the model origin, in battlefield pixels.
    anchor: Vec2,
    /// Near corner of this square segment, relative to the model origin.
    depth: f32,
    frames: Vec<Texture2D>,
    area: Rect,
    offset: Vec2,
    fps: f32,
}
struct Scenery {
    parts: Vec<SceneryPart>,
    details: String,
    collision: Vec<PlacedObstacle>,
}
fn texture(width: i32, height: i32, rgba: &[u8]) -> Result<Texture2D, String> {
    let w = u16::try_from(width).map_err(|_| "invalid image width")?;
    let h = u16::try_from(height).map_err(|_| "invalid image height")?;
    if w == 0 || h == 0 {
        return Err("empty image".into());
    }
    let t = Texture2D::from_rgba8(w, h, rgba);
    t.set_filter(FilterMode::Nearest);
    Ok(t)
}
/// Name of the first list entry: an ordinary battle, which has no preset.
const NO_PRESET: &str = "(no preset: ordinary battle)";

/// The preset map of a list entry; for `NO_PRESET` a preset of zeros, which
/// leaves every cell as the underlying ground.
fn preset(res: &Resources, name: &str) -> Option<BattlefieldPreset> {
    if name == NO_PRESET {
        let rows = h4_formats::battlefield::row_bounds()
            .map(|(start, end)| BattlefieldRow {
                start,
                cells: vec![0; end - start],
            })
            .collect();
        return Some(BattlefieldPreset {
            rows,
            layers: Vec::new(),
        });
    }
    res.read(name)
        .and_then(|b| BattlefieldPreset::read(&b))
        .ok()
}

fn backdrop(res: &Resources, name: &str) -> Result<Vec<Backdrop>, String> {
    if name == NO_PRESET {
        return Ok(Vec::new());
    }
    let model = res
        .read(name)
        .and_then(|b| BattlefieldPreset::read(&b))
        .map_err(|e| e.to_string())?;
    model
        .layers
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| {
            Ok(Backdrop {
                texture: texture(l.rect.width(), l.rect.height(), &l.to_rgba())?,
                area: l.rect,
            })
        })
        .collect()
}
fn scenery(res: &Resources, name: &str, shadow: bool) -> Result<Scenery, String> {
    let model = res
        .read(name)
        .and_then(|b| CombatObject::read(&b))
        .map_err(|e| e.to_string())?;
    let segments = model.segments().map_err(|e| e.to_string())?;
    let mut parts = Vec::new();
    for segment in &segments {
        // The native crop can retain empty segments. They still take part in
        // collision, but have no pixels to upload.
        let Some(area) = segment.image.bounds() else {
            continue;
        };
        let image: &ActorSequence = &segment.image;
        let frames = (0..image.frames.len().max(1))
            .map(|i| texture(area.width(), area.height(), &image.compose(i, area, shadow)))
            .collect::<Result<Vec<_>, _>>()?;
        let (x, y) = segment.at;
        parts.push(SceneryPart {
            anchor: vec2(16. * (y - x) as f32, 8. * (x + y) as f32),
            depth: 8. * (x + y + 2 * i32::from(segment.size)) as f32,
            frames,
            area,
            offset: vec2(image.offset.0 as f32, image.offset.1 as f32),
            fps: f32::from(model.header.frames_per_second.max(1)),
        });
    }
    let collision = if !model.blocks() || model.skips_blocking() {
        Vec::new()
    } else {
        segments
            .iter()
            .enumerate()
            .map(|(i, segment)| {
                PlacedObstacle::from_segment(&model, segment, (0, 0), i as u32 + 1)
                    .expect("resource dimensions fit battlefield coordinates")
            })
            .collect()
    };
    Ok(Scenery {
        parts,
        collision,
        details: format!(
            "type {} / {}, properties {:?}",
            model.header.type_id, model.header.type_name, model.header.properties
        ),
    })
}
fn names(res: &Resources, prefix: &str) -> Vec<String> {
    let mut names: Vec<_> = res
        .names()
        .filter(|n| n.to_lowercase().starts_with(prefix))
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}
fn config() -> Conf {
    Conf {
        window_title: "Heroes IV — battlefield resource viewer".into(),
        window_width: 1180,
        window_height: 900,
        ..Default::default()
    }
}
#[macroquad::main(config)]
async fn main() {
    let res = match Resources::open(data_dir(std::env::args().nth(1))) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let mut fields = names(&res, "battlefield_preset_map.");
    fields.insert(0, NO_PRESET.to_string());
    let objects = names(&res, "combat_object.");
    if fields.is_empty() || objects.is_empty() {
        eprintln!("no battlefield or scenery resources");
        return;
    }
    let mut field = 0;
    let mut object = match std::env::var("H4_SCENERY") {
        Ok(name) => match objects
            .iter()
            .position(|item| item.eq_ignore_ascii_case(&name))
        {
            Some(index) => index,
            None => {
                eprintln!("unknown scenery resource: {name}");
                return;
            }
        },
        Err(_) => 0,
    };
    let mut shadow = true;
    let mut background = backdrop(&res, &fields[field]);
    let mut item = scenery(&res, &objects[object], shadow);
    let mut anchor = vec2(590., 512.);
    let mut grid = std::env::var_os("H4_GRID").is_some();
    // This is a user-selected battle context, independent of resource names.
    let mut on_bridge = std::env::var("H4_BRIDGE").is_ok_and(|v| v == "1");
    let mut selected = Some((51, 51));
    let mut map = preset(&res, &fields[field]);
    let mut collision = None;
    let mut collision_dirty = true;
    // Automatic obstacles (O): the original placer on the current backdrop.
    let placer = placer_inputs(&res);
    let mut auto = std::env::var("H4_OBSTACLES").is_ok_and(|v| v == "1");
    let mut seed: u32 = std::env::var("H4_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let mut ground = 0;
    let mut placed: Vec<ObstaclePlacement> = Vec::new();
    // Sprites of placed models, by model index; loaded on first use.
    let mut sprites: HashMap<usize, Result<Scenery, String>> = HashMap::new();
    let mut paused = false;
    let mut elapsed = 0.;
    let mut ticks = 0;
    loop {
        if is_key_pressed(KeyCode::Escape) {
            break;
        }
        let mut reload_field = false;
        let mut reload_item = false;
        for (key, delta) in [(KeyCode::Right, 1), (KeyCode::Left, -1)] {
            if is_key_pressed(key) {
                field = (field as isize + delta).rem_euclid(fields.len() as isize) as usize;
                reload_field = true;
            }
        }
        for (key, delta) in [(KeyCode::Down, 1), (KeyCode::Up, -1)] {
            if is_key_pressed(key) {
                object = (object as isize + delta).rem_euclid(objects.len() as isize) as usize;
                reload_item = true;
            }
        }
        if is_key_pressed(KeyCode::S) {
            shadow = !shadow;
            reload_item = true;
            sprites.clear();
        }
        if is_key_pressed(KeyCode::O) {
            auto = !auto;
            collision_dirty = true;
        }
        if is_key_pressed(KeyCode::R) {
            // Another battle seed (the game draws one number 0..32767).
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 32_768;
            collision_dirty = true;
        }
        if is_key_pressed(KeyCode::T) {
            ground = (ground + 1) % GROUNDS.len();
            collision_dirty = true;
        }
        if is_key_pressed(KeyCode::Space) {
            paused = !paused;
        }
        if is_key_pressed(KeyCode::B) {
            on_bridge = !on_bridge;
            collision_dirty = true;
        }
        if is_key_pressed(KeyCode::G) {
            grid = !grid;
        }
        if reload_field {
            map = preset(&res, &fields[field]);
            selected = Some((51, 51));
            anchor = vec2(590., 512.);
            collision_dirty = true;
            background = backdrop(&res, &fields[field]);
        }
        if reload_item {
            collision_dirty = true;
            item = scenery(&res, &objects[object], shadow);
            elapsed = 0.;
        }
        if !paused {
            elapsed += get_frame_time();
        }
        let scale = (screen_width() / 1180.)
            .min((screen_height() - 184.) / 1024.)
            .max(0.01);
        let origin = vec2((screen_width() - 1180. * scale) / 2., 184.);
        if is_mouse_button_pressed(MouseButton::Left) {
            let mouse = mouse_position();
            let clicked = (vec2(mouse.0, mouse.1) - origin) / scale;
            if mouse.1 >= origin.y {
                let nearest = map.as_ref().and_then(|map| {
                    map.rows
                        .iter()
                        .enumerate()
                        .flat_map(|(x, row)| {
                            (row.start..row.start + row.cells.len()).map(move |y| (x, y))
                        })
                        .min_by(|&(ax, ay), &(bx, by)| {
                            let a = BattlefieldPreset::cell_center(ax, ay).expect(STORED_CELL);
                            let b = BattlefieldPreset::cell_center(bx, by).expect(STORED_CELL);
                            vec2(a.0 as f32, a.1 as f32)
                                .distance_squared(clicked)
                                .total_cmp(&vec2(b.0 as f32, b.1 as f32).distance_squared(clicked))
                        })
                });
                selected = nearest;
                collision_dirty = true;
                anchor = if grid {
                    nearest
                        .and_then(|(x, y)| BattlefieldPreset::cell_center(x, y))
                        .map(|(x, y)| vec2(x as f32, y as f32 - 8.))
                        .unwrap_or(clicked)
                } else {
                    clicked
                };
            }
        }
        // A blank underlying field is explicit; terrain generation is separate.
        if collision_dirty {
            placed.clear();
            collision = map.as_ref().map(|map| {
                let mut grid = BattlefieldGrid::from_preset(
                    map,
                    CellSurface {
                        terrain: GROUNDS[ground].0,
                        flags: FLAG_10,
                    },
                    on_bridge,
                );
                grid.block_edges();
                if let (true, Ok((models, weights))) = (auto, &placer) {
                    let context = PlacerContext {
                        on_bridge,
                        ..Default::default()
                    };
                    let mut random = CombatRandom::new(seed, 1);
                    placed = place_obstacles(&grid, models, weights, &context, &mut random);
                    for (k, p) in placed.iter().enumerate() {
                        let name = format!("combat_object.{}.h4d", models[p.model].name);
                        let sprite = sprites
                            .entry(p.model)
                            .or_insert_with(|| scenery(&res, &name, shadow));
                        if let Ok(sprite) = sprite {
                            for (i, &segment) in sprite.collision.iter().enumerate() {
                                let mut obstacle = segment;
                                obstacle.at = (p.at.0 + segment.at.0, p.at.1 + segment.at.1);
                                // Manual scenery uses small ids; these start at 1000.
                                obstacle.occupant.id = 1000 + 64 * k as u32 + i as u32;
                                grid.add_obstacle(obstacle)?;
                            }
                        }
                    }
                }
                if let (Some(at), Ok(item)) = (selected, &item) {
                    for &segment in &item.collision {
                        let mut obstacle = segment;
                        obstacle.at = (at.0 as i32 + segment.at.0, at.1 as i32 + segment.at.1);
                        grid.add_obstacle(obstacle)?;
                    }
                }
                Ok::<_, &'static str>(grid)
            });
            collision_dirty = false;
        }
        clear_background(Color::from_rgba(36, 39, 43, 255));
        if let Ok(layers) = &background {
            for l in layers {
                draw_texture_ex(
                    &l.texture,
                    origin.x + l.area.left as f32 * scale,
                    origin.y + l.area.top as f32 * scale,
                    WHITE,
                    DrawTextureParams {
                        dest_size: Some(
                            vec2(l.area.width() as f32, l.area.height() as f32) * scale,
                        ),
                        ..Default::default()
                    },
                );
            }
        }
        if grid {
            if let Some(map) = &map {
                for (x, row) in map.rows.iter().enumerate() {
                    for y in row.start..row.start + row.cells.len() {
                        let (cx, cy) = BattlefieldPreset::cell_center(x, y).expect(STORED_CELL);
                        let center = origin + vec2(cx as f32, cy as f32) * scale;
                        let corners = [vec2(-16., 0.), vec2(0., -8.), vec2(16., 0.), vec2(0., 8.)];
                        let (_, base_color) =
                            preset_status(map.cell(x, y).expect(STORED_CELL), on_bridge);
                        let color = match &collision {
                            Some(Ok(grid))
                                if grid
                                    .cell(x as i32, y as i32)
                                    .is_some_and(|c| !c.occupants.is_empty()) =>
                            {
                                if grid.is_blocked(x as i32, y as i32, None) {
                                    PINK
                                } else {
                                    base_color
                                }
                            }
                            Some(Err(_)) => GRAY,
                            _ => base_color,
                        };
                        for i in 0..4 {
                            let a = center + corners[i] * scale;
                            let b = center + corners[(i + 1) % 4] * scale;
                            draw_line(
                                a.x,
                                a.y,
                                b.x,
                                b.y,
                                1.,
                                Color::new(color.r, color.g, color.b, 0.45),
                            );
                        }
                    }
                }
            }
        }
        // Interleave square pieces of compound models, including the manual
        // preview. Sorting whole images cannot handle overlapping rectangles.
        // This is a geometric viewer order; the native depth comparator and
        // its tie rules have not yet been recovered.
        let mut draw_order: Vec<(&SceneryPart, Vec2)> = Vec::new();
        if auto {
            for p in &placed {
                if let Some(Ok(sprite)) = sprites.get(&p.model) {
                    // Project directly: a model origin may be outside stored
                    // rows even when its visible pieces intersect the field.
                    let at = vec2(
                        590. + 16. * (p.at.1 - p.at.0) as f32,
                        512. + 8. * (p.at.0 + p.at.1 - 102) as f32,
                    );
                    draw_order.extend(sprite.parts.iter().map(|part| (part, at)));
                }
            }
        }
        if let Ok(item) = &item {
            draw_order.extend(item.parts.iter().map(|part| (part, anchor)));
        }
        draw_order
            .sort_by(|(a, at_a), (b, at_b)| (at_a.y + a.depth).total_cmp(&(at_b.y + b.depth)));
        for (part, at) in draw_order {
            draw_scenery(part, at + part.anchor, origin, scale, elapsed);
        }
        // Keep the collision footprint visible even beneath an opaque sprite.
        if grid {
            if let (Some(Ok(collision)), Some(map)) = (&collision, &map) {
                for (x, row) in map.rows.iter().enumerate() {
                    for y in row.start..row.start + row.cells.len() {
                        if !collision
                            .cell(x as i32, y as i32)
                            .is_some_and(|cell| !cell.occupants.is_empty())
                        {
                            continue;
                        }
                        let (cx, cy) = BattlefieldPreset::cell_center(x, y).expect(STORED_CELL);
                        let center = origin + vec2(cx as f32, cy as f32) * scale;
                        let corners = [vec2(-16., 0.), vec2(0., -8.), vec2(16., 0.), vec2(0., 8.)];
                        for i in 0..4 {
                            let a = center + corners[i] * scale;
                            let b = center + corners[(i + 1) % 4] * scale;
                            draw_line(a.x, a.y, b.x, b.y, 1.5, PINK);
                        }
                    }
                }
            }
        }
        let point = origin + anchor * scale;
        draw_line(point.x - 5., point.y, point.x + 5., point.y, 1., YELLOW);
        draw_line(point.x, point.y - 5., point.x, point.y + 5., 1., YELLOW);
        for (i, text) in [
            fields[field].clone(),
            objects[object].clone(),
            item.as_ref().map(|s| s.details.clone()).unwrap_or_else(|e| e.clone()),
            "Left/Right field | Up/Down object | G grid | B bridge mode | Space pause | S shadow | Esc exit".into(),
            match &placer {
                Ok(_) if auto => format!(
                    "O automatic obstacles: {} (R seed {seed}, T ground {})",
                    placed.len(),
                    GROUNDS[ground].1
                ),
                Ok(_) => "O automatic obstacles: off".into(),
                Err(e) => format!("automatic obstacles unavailable: {e}"),
            },
            format!("Battle on bridge: {} | red preset, pink scenery, green clear, orange inherited", if on_bridge { "yes" } else { "no" }),
        ]
        .iter()
        .enumerate()
        {
            draw_text(text, 10., 22. + i as f32 * 24., 19., WHITE);
        }
        let status = selected.and_then(|(x, y)| {
            map.as_ref().and_then(|m| m.cell(x, y)).map(|v| {
                format!(
                    "Cell ({x}, {y}): code {v}; {}",
                    match &collision {
                        Some(Ok(grid))
                            if grid
                                .cell(x as i32, y as i32)
                                .is_some_and(|c| !c.occupants.is_empty()) =>
                            "occupied by scenery",
                        Some(Err(error)) => error,
                        _ => preset_status(v, on_bridge).0,
                    }
                )
            })
        });
        if let Some(status) = status {
            draw_text(&status, 10., 166., 18., YELLOW);
        }
        if let Err(e) = &background {
            draw_text(e, 10., 184., 20., RED);
        }
        ticks += 1;
        if ticks == 3 {
            if let Ok(path) = std::env::var("H4_SCREENSHOT") {
                get_screen_data().export_png(&path);
                break;
            }
        }
        next_frame().await;
    }
}
