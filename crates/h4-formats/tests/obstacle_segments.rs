use h4_formats::combat_object::CombatObject;

fn model(input: &[i32]) -> CombatObject {
    let [w, h, attr, flag, kind, elevation, left, top, ox, oy, depth, mode, count] = *input else {
        panic!("fixture input")
    };
    let mut data = 2_u16.to_le_bytes().to_vec();
    data.extend([
        w as u8,
        h as u8,
        elevation as u8,
        10,
        flag as u8,
        attr as u8,
        0,
        0,
        0,
        0,
    ]);
    data.extend((kind as u16).to_le_bytes());
    data.extend(0_u16.to_le_bytes());
    data.extend(2_u16.to_le_bytes());
    data.extend((count as u16).to_le_bytes());
    for index in 0..count {
        let (left, top, mode) = (left - 9 * index, top + 7 * index, (mode + 2 * index) % 5);
        data.extend(3_u16.to_le_bytes());
        data.extend(0_u16.to_le_bytes());
        data.extend([0, 0, 0, 0, 0, 255, 0, 255, 0]);
        data.extend(0_u16.to_le_bytes());
        data.push(depth as u8);
        for v in [left, top, left + 96, top + 64] {
            data.extend(v.to_le_bytes());
        }
        for y in 0..64_u32 {
            data.extend(0_u16.to_le_bytes());
            data.extend(96_u16.to_le_bytes());
            data.extend((y * 96).to_le_bytes());
        }
        let pixels: Vec<u8> = (0..64)
            .flat_map(|y| {
                (0..96).map(move |x| {
                    if mode == 0
                        || (mode == 1 && (x + 3 * y) % 11 < 4)
                        || (mode == 2 && (32..48).contains(&x) && (24..32).contains(&y))
                        || (mode == 3 && [(0, 0), (95, 63), (40, 32)].contains(&(x, y)))
                    {
                        (1 + (x + y) % 2) as u8
                    } else {
                        0
                    }
                })
            })
            .collect();
        data.extend(&pixels);
        if depth == 4 {
            let alpha: Vec<u8> = pixels
                .iter()
                .enumerate()
                .map(|(k, &v)| if (k / 96 + k % 96) % 7 == 0 { 0 } else { v * 5 })
                .collect();
            data.extend(alpha.chunks(2).map(|a| a[0] | (a[1] << 4)));
        }
    }
    data.extend(ox.to_le_bytes());
    data.extend(oy.to_le_bytes());
    CombatObject::read(&data).unwrap()
}

#[test]
fn full_native_segmentation_vectors() {
    let mut cases = 0;
    for line in include_str!("fixtures/obstacle_segments.txt")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let (input, expected) = line.split_once(" -> ").unwrap();
        let values: Vec<i32> = input
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        let actual = model(&values)
            .segments()
            .unwrap()
            .into_iter()
            .map(|s| {
                let mut fields = vec![
                    s.size.to_string(),
                    s.at.0.to_string(),
                    s.at.1.to_string(),
                    s.image.offset.0.to_string(),
                    s.image.offset.1.to_string(),
                ];
                for layer in s
                    .image
                    .frames
                    .iter()
                    .chain(&s.image.shadows)
                    .chain(&s.image.base)
                {
                    let r = layer.rect;
                    fields.extend([r.left, r.top, r.right, r.bottom].map(|v| v.to_string()));
                    let mut digest = 0xcbf29ce484222325_u64;
                    for pixel in layer.to_rgba().chunks(4) {
                        let alpha = pixel[3] / 17;
                        let index = if alpha == 0 {
                            0
                        } else if pixel[0] != 0 {
                            1
                        } else {
                            2
                        };
                        for v in [index, alpha] {
                            digest = (digest ^ u64::from(v)).wrapping_mul(0x100000001b3);
                        }
                    }
                    fields.push(format!("{digest:016x}"));
                }
                fields.join(",")
            })
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(actual, expected, "input {input}");
        cases += 1;
    }
    assert_eq!(cases, 200);
}

#[test]
fn segmentation_rejects_extreme_coordinates_before_building_masks() {
    let mut model = model(&[2, 4, 1, 0, 37, 0, 0, 0, 0, 0, 1, 0, 1]);
    model.image.offset.0 = i32::MIN;
    assert!(model.segments().is_err());
    model.image.offset.0 = 0;
    model.image.frames[0].rect.left = i32::MAX - 96;
    model.image.frames[0].rect.right = i32::MAX;
    assert!(model.segments().is_err());
}
