# Урон в бою (heroes4.exe rus-3.0)

Сырой декомпилят: `damage.c`. Имена `t_combat_creature::*` перенесены с complete-3.0 (tier D, но
сигнатуры и `ret N` совпадают, поведение кода им соответствует). `FUN_*` — без имени; ниже
предполагаемые роли. Способности — `re/enum_creature_ability.tsv`.

## get_melee_damage(target, count = -1) — rva 1ee4d0

```
count = count < 0 ? creature.get_number() : count
base  = (damage_low + damage_high) * this->number / 2      // среднее × размер стека
dmg   = modify_damage(base, target, ranged=false, count)   // FUN_005edf30
return finalize_damage(number, target, dmg)                // FUN_005ee540
```

## get_ranged_damage(target, ...) — rva 1eebf0

Только при `has_ability(ranged)` и оставшихся выстрелах:
```
n    = shooters_alive(target)            // FUN_005ee400: учитывает уже полученный урон
base = (low + high) * n / 2
dmg  = modify_damage(base, target, ranged=true)
dmg /= max(1, range_penalty(target))     // FUN_00570800 -> t_abstract_creature::get_range_effect(дистанция до края)
return finalize_damage(n, target, dmg)
```

## modify_damage — FUN_005edf30 (rva 1edf30)

1. `ratio = get_damage_modifier(target_defense, ranged, attack)` — rva 250a60:
   атака / защита (виртуальные геттеры с учётом ranged), зажато в **[0.05, 20.0]**
   (константы `0x97458c`, `0x9853c8`). `dmg = base * ratio`.
2. Оберег от школы атакующего (switch по школе 0..4: life → `life_protection`, chaos →
   `chaos_protection`, … ): `dmg = dmg * 100 / 150`.
3. Атакующий с `fire_attack` / `breath_attack` / `arc_breath_attack` (или `large_area_effect`
   при стрельбе) против `fire_resistance`: `dmg >>= 1`.
4. `cold_attack` против `cold_resistance`: `dmg >>= 1`.
5. Не меньше 1.

Ещё в начале — проверки `insubstantial` / `mechanical` / `ranged_giantslayer` и флагов (0x8c),
смысл не разобран. Ветки `param_6 == 1/2` (удача/неудача?) декомпилированы плохо — x87 + `__ftol`.

## finalize_damage — FUN_005ee540 (rva 1ee540)

- Если у атакующего способность 0x42 и цель — живое существо (не 0x4d undead, не 0x11 elemental,
  не 0x2a) и произведение > 499 — добавка к урону (похоже на смертельный удар/вытягивание; не разобрано).
- Урон ограничен оставшимися хитами цели.
- Цель с способностью 0x07: `dmg = (dmg * 7 + 5) / 10` (−30%, с округлением).

## Что не хватает для полной картины

- Структура `t_abstract_creature` (смещение `+0x98` в `t_combat_creature`, `+0xb0` — число в стеке,
  `+0x198`/`+0x408`/`+0x354` — что-то про полученный урон).
- Имена виртуальных методов по слотам vftable (`+0x18`, `+0x20`, `+0x24`, `+0x30`, `+0x44`, `+0x68` = has_ability).
- Сверка с известными формулами сообщества (Equilibris `equilibris.chm`, `Skill_Guide.pdf`).
