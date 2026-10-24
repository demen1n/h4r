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

## Настоящий бросок — rva 1ee240 (имени нет)

`get_melee_damage` выше — это среднее (для ИИ и подсказки). Настоящий удар бросает кости
генератором боя (`decomp/random.md`, назначение 0, ключ — id цели и атакующего):
```
range = high - low + 1
if number < 10: sum = Σ по каждому существу (low + roll % range)
else:           sum = number * (10 бросков) / 10
return modify_damage(sum, target, ranged, moved, luck)
```
Рядом: rva 1ee380 — `modify_damage(high * number, ...)` (максимум), 1ee3c0 — `low * number` (минимум).
Слоты vftable атакующего (`this+0x98`, проверено по vftable `t_hero` и `t_combat_creature`):
`+0x20` high, `+0x24` low, `+0x28` get_defense_reduction(bool), `+0x30` hit points,
`+0x34` get_luck, `+0x44` get_offense(bool), `+0x68` has_ability.

## modify_damage — FUN_005edf30 (rva 1edf30)

Аргументы: base, target, ranged, moved (в melee по умолчанию слот `+0x18`), luck (0/1/2).
По дизассемблеру, по порядку:

1. **charging** (0x0b), только melee, moved > 500: `dmg = (moved*10/350 + 85) * dmg / 100`.
2. Если у цели слот `+0x40` истинен («великан»?): атакующий с 0x1c, или (ranged ? 0x35 : 0x2b)
   → `dmg *= 2`; иначе эффект заклинания 0x8c у атакующего → `dmg += dmg >> 1`.
   Имена 0x1c (`insubstantial`) и 0x2b (`mechanical`) в нашем enum здесь не подходят.
3. **Удача**: множитель `m = {1.0, 1.25, 1.5, 2.0}[obj->[+0x60]]`, `obj = [this+0xa18]`
   (таблица double по VA 0x97cb68, rva 17b8f0). luck 1: `dmg = trunc(m * dmg)`,
   luck 2: `dmg = trunc(dmg / m)`.
4. `ratio = get_damage_modifier(...)`, `dmg = trunc(ratio * dmg)`.
5. Оберег по школе атакующего (слот `+4`: 0 life — способность 0x22 или эффект 0x77;
   1 order — 0x79; 2 death — эффект 0x76 или способность 0x10; 3 chaos — 0x75 или 0x0a;
   4 nature — 0x78; might — нет): `dmg = dmg * 100 / 150`.
6. fire_attack / breath_attack / arc_breath_attack (или large_area_effect при стрельбе)
   против fire_resistance: `dmg >>= 1`. cold_attack против cold_resistance: `dmg >>= 1`.
7. Не меньше 1.

## get_damage_modifier — rva 250a60 (подробно)

```
bonus = target.has_defense[+4](ranged)    // множитель защиты
red   = this[+0x28](ranged)               // get_defense_reduction
if red <= bonus: offense *= red
else if bonus > 1: bonus = 1
ratio = offense / (target.has_defense[+8](ranged) * bonus)   // +8 — базовая защита
clamp: сначала ratio < 0.05 (и NaN) → 0.05, потом ratio > 20 → 20
```

## finalize_damage — FUN_005ee540 (rva 1ee540)

cdecl: (attacker, number, target, already_dealt, dmg).
- Атакующий со способностью 0x42 против живого `t_combat_creature` (не 0x4d undead, не 0x2a,
  не 0x11 elemental): `x = (FUN_5f1de0(...) * vf90()) / 100` (если FUN_5b6140), и если
  `x * number >= 500` — `dmg += trunc(attacker.hp * x * number * 0.001)`. Смысл не разобран.
- `dmg = min(dmg, max(0, target.total_hp(+0x2c(1,-1)) - already_dealt))`.
- Цель с 0x07 (block): `dmg = (dmg * 7 + 5) / 10`.

## Что не хватает для полной картины

- Где решается удача атакующего (аргумент luck) и как настоящий удар собирает бросок,
  модификаторы и `finalize_damage` (`attack.cpp`).
- Слот `+0x40` цели, эффект 0x8c, способность 0x42, поле `[+0x60]` для силы удачи.
- Структура `t_abstract_creature` (`+0x198`/`+0x408`/`+0x354` — что-то про полученный урон).
- Сверка с известными формулами сообщества (Equilibris `equilibris.chm`, `Skill_Guide.pdf`).
