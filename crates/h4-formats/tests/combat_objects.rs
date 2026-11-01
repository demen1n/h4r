mod common;
use h4_formats::combat_object::CombatObject;

#[test]
fn installed_objects_read_to_end_and_decode() {
    let Some(res) = common::resources() else {
        return;
    };
    let names = common::names_with_prefix(&res, "combat_object.");
    let mut failures = Vec::new();
    let mut versions = [0; 3];
    for name in &names {
        match res.read(name).and_then(|b| CombatObject::read(&b)) {
            Ok(model) => {
                versions[model.version as usize] += 1;
                for layer in model
                    .image
                    .frames
                    .iter()
                    .chain(&model.image.shadows)
                    .chain(&model.image.base)
                {
                    let _ = layer.to_rgba();
                }
                for segment in model.segments().unwrap() {
                    for layer in segment
                        .image
                        .frames
                        .iter()
                        .chain(&segment.image.shadows)
                        .chain(&segment.image.base)
                    {
                        let _ = layer.to_rgba();
                    }
                }
            }
            Err(e) => failures.push(format!("{name}: {e}")),
        }
    }
    common::assert_no_failures("combat objects", &failures);
    assert_eq!(names.len(), 1048);
    assert_eq!(versions, [6, 0, 1042]);
}

#[test]
fn versions_and_corrupt_input() {
    for version in 0_u16..=2 {
        let mut data = version.to_le_bytes().to_vec();
        data.push(3);
        if version == 2 {
            data.push(7);
        }
        data.extend_from_slice(&[42, 10, 1]);
        if version >= 1 {
            data.extend_from_slice(&[1, 2, 3, 4, 5]);
            data.extend_from_slice(&4_u16.to_le_bytes());
            data.extend_from_slice(&4_u16.to_le_bytes());
            data.extend_from_slice(b"rock");
        }
        // Unwrapped image sequence: version 2, no layers, offset (-8, 12).
        data.extend_from_slice(&[2, 0, 0, 0]);
        data.extend_from_slice(&(-8_i32).to_le_bytes());
        data.extend_from_slice(&12_i32.to_le_bytes());
        let model = CombatObject::read(&data).unwrap();
        assert_eq!(model.properties, [3, if version == 2 { 7 } else { 3 }]);
        assert_eq!(model.frames_per_second, 10);
        assert_eq!(model.image.offset, (-8, 12));
        assert_eq!(model.type_name, if version == 0 { "" } else { "rock" });
        for end in 0..data.len() {
            assert!(CombatObject::read(&data[..end]).is_err());
        }
        data.push(0);
        assert!(CombatObject::read(&data).is_err());
        data[0] = 3;
        assert!(CombatObject::read(&data).is_err());
    }
}
