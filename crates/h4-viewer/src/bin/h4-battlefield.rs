//! Inspect preset backdrops and combat scenery; this is not a playable battle.
use h4_formats::archive::{data_dir, Resources};
use h4_formats::battlefield::BattlefieldPreset;
use h4_formats::combat_object::CombatObject;
use h4_formats::sprite::Rect;
use h4_rules::battlefield::{
    apply_preset_cell, cell_is_blocked, BattlefieldGrid, CellSurface, PlacedObstacle,
};
use macroquad::prelude::*;

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
struct Scenery {
    frames: Vec<Texture2D>,
    area: Rect,
    offset: Vec2,
    fps: f32,
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
fn backdrop(res: &Resources, name: &str) -> Result<Vec<Backdrop>, String> {
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
    let area = model.image.bounds().ok_or("empty object image")?;
    let frames = (0..model.image.frames.len().max(1))
        .map(|i| {
            texture(
                area.width(),
                area.height(),
                &model.image.compose(i, area, shadow),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let collision = if !model.blocks() || model.skips_blocking() {
        Vec::new()
    } else {
        model
            .segments()
            .map_err(|e| e.to_string())?
            .iter()
            .enumerate()
            .map(|(i, segment)| {
                PlacedObstacle::from_segment(&model, segment, (0, 0), i as u32 + 1)
                    .expect("resource dimensions fit battlefield coordinates")
            })
            .collect()
    };
    Ok(Scenery {
        frames,
        area,
        collision,
        offset: vec2(model.image.offset.0 as f32, model.image.offset.1 as f32),
        fps: f32::from(model.frames_per_second.max(1)),
        details: format!(
            "type {} / {}, properties {:?}",
            model.type_id, model.type_name, model.properties
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
    let fields = names(&res, "battlefield_preset_map.");
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
    let mut anchor = vec2(590., 520.);
    let mut grid = std::env::var_os("H4_GRID").is_some();
    // This is a user-selected battle context, independent of resource names.
    let mut on_bridge = std::env::var("H4_BRIDGE").is_ok_and(|v| v == "1");
    let mut selected = Some((51, 51));
    let mut map = res
        .read(&fields[field])
        .and_then(|b| BattlefieldPreset::read(&b))
        .ok();
    let mut collision = None;
    let mut collision_dirty = true;
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
            map = res
                .read(&fields[field])
                .and_then(|b| BattlefieldPreset::read(&b))
                .ok();
            selected = Some((51, 51));
            anchor = vec2(590., 520.);
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
            .min((screen_height() - 160.) / 1024.)
            .max(0.01);
        let origin = vec2((screen_width() - 1180. * scale) / 2., 160.);
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
                        .map(|(x, y)| vec2(x as f32, y as f32))
                        .unwrap_or(clicked)
                } else {
                    clicked
                };
            }
        }
        // A blank underlying field is explicit; terrain generation is separate.
        if collision_dirty {
            collision = map.as_ref().map(|map| {
                let mut grid = BattlefieldGrid::from_preset(
                    map,
                    CellSurface {
                        terrain: 1,
                        flags: 0x10,
                    },
                    on_bridge,
                );
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
        if let Ok(item) = &item {
            let index = (elapsed * item.fps) as usize % item.frames.len();
            let pos = origin
                + (anchor + item.offset + vec2(item.area.left as f32, item.area.top as f32))
                    * scale;
            draw_texture_ex(
                &item.frames[index],
                pos.x,
                pos.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(
                        vec2(item.area.width() as f32, item.area.height() as f32) * scale,
                    ),
                    ..Default::default()
                },
            );
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
            draw_text(&status, 10., 142., 18., YELLOW);
        }
        if let Err(e) = &background {
            draw_text(e, 10., 160., 20., RED);
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
