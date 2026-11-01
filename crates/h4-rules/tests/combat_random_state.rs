use h4_rules::random::CombatRandom;

#[test]
fn restore_preserves_native_small_index_cache_quirk() {
    let mut original = CombatRandom::new(1, 1);
    original.roll(0, 0, 43);
    original.roll(0, 0, 43); // index 131: builds the first checkpoint
    let saved = original.snapshot();
    assert!(!saved.checkpoints.is_empty());
    let mut restored = CombatRandom::from_snapshot(saved.clone()).unwrap();
    let expected = original.roll(0, 0, 0); // index 44: uses old cache, then clears it
    assert_eq!(restored.roll(0, 0, 0), expected);
    assert_eq!(restored.snapshot(), original.snapshot());
    let mut discarded_cache = saved;
    discarded_cache.checkpoints.clear();
    let mut wrong = CombatRandom::from_snapshot(discarded_cache).unwrap();
    assert_ne!(wrong.roll(0, 0, 0), expected);
}

#[test]
fn counters_serialize_stably_regardless_of_hash_map_insertion_order() {
    let mut first = CombatRandom::new(1234, 15);
    let mut second = first.clone();
    first.roll(1, 2, 0);
    first.roll(2, 1, 1);
    second.roll(2, 1, 1);
    second.roll(1, 2, 0);
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    let mut restored: CombatRandom =
        serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap();
    assert_eq!(restored.roll(1, 2, 0), first.roll(1, 2, 0));
}

#[test]
fn malformed_rng_snapshots_are_rejected_without_panics() {
    let mut random = CombatRandom::new(3, 15);
    random.roll(1, 2, 0);
    let good = random.snapshot();
    let mut bad = good.clone();
    bad.id_modulus = 0;
    assert!(CombatRandom::from_snapshot(bad).is_err());
    let mut bad = good.clone();
    bad.counters.push(bad.counters[0].clone());
    assert!(CombatRandom::from_snapshot(bad).is_err());
    let mut bad = good.clone();
    bad.counters[0].first = 15;
    assert!(CombatRandom::from_snapshot(bad).is_err());
    let mut bad = good;
    bad.checkpoints[0] ^= 1;
    assert!(CombatRandom::from_snapshot(bad).is_err());
}

#[test]
fn restored_roll_counter_wraps_like_native_u32_arithmetic() {
    let mut random = CombatRandom::new(1, 1);
    random.roll(0, 0, 0);
    let mut state = random.snapshot();
    state.counters[0].count = u32::MAX;
    let mut restored = CombatRandom::from_snapshot(state).unwrap();
    assert_eq!(restored.roll(0, 0, 0), 41); // count wraps to 0, index 0
    assert_eq!(restored.snapshot().counters[0].count, 0);
}
