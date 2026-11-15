# Ближний удар: порядок шагов и ответный удар (#65)

Источник — русский exe 3.0 (Бука), адреса — RVA. Имена методов взяты из отладочного
мапа (`third_party/homm4-symbols`, `battlefield.obj` и `attack.obj`) и привязаны по
порядку линковки, перекрёстным вызовам и сигнатурам. Перенесено готовым только
`t_battlefield::resolve_attacks` и конструктор `t_attack`. Всё ниже прочитано по
дизассемблеру (`tools/disasm.py`, `tools/xrefs.py`), **не портировано и не сверено
исполнением**.

## Функции

| функция | rva | привязка |
|---|---|---|
| `t_battlefield::begin_attack(creature& attacker, bool return_after)` | `16ede0` | порядок линковки; в отладочной сборке без второго аргумента |
| `t_battlefield::begin_melee_attack(creature& a, attackable& t, bool sequential, bool retaliation)` | `16e520` | 4 аргумента, как в мапе |
| `t_battlefield::end_melee_attack(int strikes_left, bool return_after)` | `16ea60` | в мапе один аргумент |
| `t_battlefield::begin_melee_retaliation(bool return_after)` | `16f130` | обработчик из `begin_attack` |
| `t_battlefield::add_attack(creature*, attackable*, bool ranged, const msg&)` | `175ea0` | сразу после `cast_spell` (`175e30`), сигнатура совпадает |
| `t_battlefield::resolve_attacks()` | `176160` | перенесённое имя |
| `t_battlefield::get_retaliation_time_stamp()` | `177db0` | `[bf+0x28]->get_adjusted_speed()` |
| очередь продолжений: `push(handler, bool front)` | `175a70` | список `bf+0x460` |
| `t_attack::t_attack(bf, attacker, target, bool ranged, int damage)` | `152960` | перенесённое имя |
| `t_attack::check_random_curse` | `152a20` | 9 проклятий, бросок `% число` |
| `t_attack::resolve` | `1531b0` | порядок `attack.obj` |
| `t_attack::check_vampirism` | `154740` | порядок `attack.obj` |
| число ударов `get_number_of_attacks(bool ranged)` (имя условное) | `1f1d10` | |
| «может действовать» (имя в переносе ошибочно: `on_battlefield_destruction…`) | `155ea0` | |
| `set_retaliated(bool)` / сброс по метке | `1f6b20` / `1f6b60` | |

Поля поля боя: `+0x28` — ходящее существо, `+0x504` — цель текущего действия,
`+0x2f0` — «идёт обмен ударами с ответом», `+0x314` — пройденное расстояние
(тот же `moved`, что идёт в `charging`, см. `damage.md`), `+0x44`/`+0x48` — очередь
ударов `t_attack` и её длина, `+0x478` — обработчик `resolve_attacks`, `+0x4f8` —
клетка, с которой атакующий начал (для `strike_and_return`).

Слоты vftable `t_attackable_object` у `t_combat_creature` (таблица rva `582450`,
проверено по адресам): `+0x00` `add_damage`, `+0x04` яд (`1fa9e0`), `+0x08`
`apply_damage`, `+0x10` `can_retaliate`, `+0x30` `has_ability`, `+0x34` «жив»,
`+0x24` — сам объект-цель. Смысл `+0x14` и `+0x38` не установлен.

## Число ударов (`1f1d10`)

```
n = 1; если эффект 0x7a: n = 2; если эффект 0x82: n += 1
ближний: + strikes_twice (0x46) + extra_melee_attack (0x12)
дальний: + shoots_twice (0x3b) + extra_ranged_attack (0x13), не больше выстрелов (не меньше 1)
```

## «Может действовать» (`155ea0`)

Жив (слот `+0x34`) и нет эффектов 0x0b, 0xba, 0x9a (часть списка «недееспособен»
из `retaliation.md`).

## `begin_attack` — начало ближней атаки

```
target    = [bf+0x504]
strikes   = число ударов атакующего (ближний)
bf.+0x2f0 = 0
очередь  <- resolve_attacks
target.slot14()
если не target.can_retaliate(attacker, ranged=false, n=1):
    очередь <- end_melee_attack(strikes - 1, return_after)
    begin_melee_attack(attacker, target, sequential=true, retaliation=false)
иначе:
    a_first = attacker has first_strike (0x17) и target не имеет first_strike_immunity (0x18)
    d_first = target  has first_strike и attacker не имеет first_strike_immunity
    если цель не может действовать, или attacker.slot38, или target.slot38:
        a_first = true; d_first = false
    иначе если не a_first и bf.+0x314 < 4:
        d_first = false
    retaliator = цель как существо
    если a_first == d_first:            # обычный случай — удары одновременно
        retaliator.set_retaliated(нет unlimited_retaliation 0x4e)
        очередь <- end_melee_attack(strikes - 1, return_after)
        bf.+0x2f0 = 1
        begin_melee_attack(attacker,   target,   sequential=false, retaliation=false)
        begin_melee_attack(retaliator, attacker, sequential=false, retaliation=true)
    иначе если a_first:                 # первый удар атакующего
        очередь <- begin_melee_retaliation(return_after)
        begin_melee_attack(attacker, target, true, false)
    иначе:                              # цель бьёт первой
        bf.+0x2f0 = 1
        retaliator.set_retaliated(нет 0x4e)
        очередь <- end_melee_attack(strikes, return_after)   # без «- 1»
        begin_melee_attack(retaliator, attacker, true, true)
```

**Главное.** Без первого удара ответ идёт **одновременно** с ударом атакующего: обе
`begin_melee_attack` вызываются подряд, урон обоих бросается до того, как потери
применены (урон ожидающий, `apply_damage.md`). Поэтому ответ бьёт полным стеком.
Ответ возможен только если `can_retaliate` разрешил его **до** удара.

Непонятное: условие `bf.+0x314 < 4` отключает первый удар защитника, если атакующий
прошёл меньше 4 (единицы поля). Проверить исполнением.

## `begin_melee_retaliation` — ответ после первого удара атакующего

```
retaliator = [bf+0x504]; n = число ударов атакующего - 1
если retaliator не может действовать: выход   # и ничего не ставится в очередь
bf.+0x2f0 = 1
retaliator.set_retaliated(нет 0x4e)
очередь <- resolve_attacks
очередь <- end_melee_attack(n, return_after)
begin_melee_attack(retaliator, attacker, true, true)
```

`can_retaliate` повторно не проверяется. Если цель погибла или обездвижена первым
ударом, функция выходит, не поставив `end_melee_attack`: оставшиеся удары атакующего
и возврат `strike_and_return` в этом месте не запускаются (проверить, не делает ли
этого кто-то ещё).

## `begin_melee_attack` — один удар

Анимация, затем:

- `arc_breath_attack` (0x54), `hydra_strike` (0x1d) или `3_headed_attack` (0x00) —
  ветвь `16cfa0` (несколько целей, не разобрано);
- иначе: `t_combat_flinch::add(target, !sequential)`, затем
  `add_attack(attacker, target, ranged=false, msg)`; при `breath_attack` (0x09) —
  ещё `16da90` (клетки за целью, не разобрано).

`retaliation` выбирает только текст сообщения.

## `add_attack` — урон сразу, эффекты потом

```
если ranged:
    damage = 1ee240(target, ranged=true, moved=0, 0) / max(1, range_factor)   # 170800
иначе:
    moved = (attacker == bf.+0x28) ? bf.+0x314 : 0
    damage = 1ee240(target, ranged=false, moved, x)      # x = 1701f0(две точки)
если target.slot38: damage = 0
иначе: damage = target.add_damage(damage, attacker, luck=true, permanent=false, msg)
если не ranged: очередь ударов поля <- t_attack(bf, attacker, target, false, damage)
если attacker has poison (0x31) и цель — существо и эффект 0x6f может на неё подействовать:
    p = damage; если target has death_protection (0x10): p >>= 1
    target.slot04(p >> 2, attacker)       # яд: четверть урона
```

`1ee240` — бросок урона с модификаторами (`damage.md`), то есть `damage::attack_hit`
без финальных пределов: пределы и удача цели — внутри `add_damage`
(`apply_damage.md`, шаги 4–6). Удача цели бросается при каждом ударе (третий
аргумент `add_damage` = 1). Четвёртый аргумент `1ee240` здесь — результат `1701f0` от двух точек (по
положению в `battlefield.obj` похоже на `crosses_wall` → `t_wall_bonus`, не проверено);
в `damage.md` этот аргумент описан как удача 0/1/2 — перепроверить, что это.

## `end_melee_attack` — что после анимации удара

```
если target has panic (0x30), bf.+0x2f0 и attacker.slot0c(target):
    attacker: эффект 6 снимается (1f1f80); выход
если strikes_left > 0 и атакующий может действовать и цель жива:
    очередь <- end_melee_attack(strikes_left - 1, return_after)
    target.slot14()
    begin_melee_attack(attacker, target, true, false)          # следующий удар
иначе:
    attacker: эффект 6 снимается
    если attacker has panic (0x30): target.slot0c(attacker)
    если return_after, атакующий может действовать, нет эффекта 9 и он не на исходной клетке:
        вернуться на bf.+0x4f8 (16c300)
```

Повторные удары (`strikes_twice` и т. п.) идут **после** ответа и без ответа на них
(ответ разрешается только в `begin_attack`).

## `resolve_attacks` и `t_attack::resolve`

Обработчик `bf+0x478` = `resolve_attacks` ставится в очередь перед продолжением,
поэтому после анимации удара сначала применяются потери, потом следующий шаг.
`resolve_attacks`: для каждого `t_attack` — `resolve`, затем для каждого —
`check_vampirism`, затем очередь очищается.

`resolve` (крупно, по вызовам): ауры и `fire_shield` (0x16, только ближний,
`(damage + 3) / 4`), затем `target.apply_damage(bf, true)`; если цель жива —
`check_random_curse` (0x32), `check_stone_gaze`, затем остальные эффекты после удара
(привязка `binding` через `set_binding`, бросок `203280` в нескольких местах).
Подробно не разобрано.

## Флаг «уже отвечал» (`+0xa9`) и метка `+0xb70`

- `set_retaliated(v)` (`1f6b20`): если значение меняется на true —
  `+0xb70 = get_retaliation_time_stamp() - 3 000 000`. Метка — приоритет
  (`get_adjusted_speed`, `initiative.md`) существа, которое ходит сейчас.
  Существо с `unlimited_retaliation` вызывает `set_retaliated(false)`.
- Сброс раунда (`1ecfb0`): если флаг стоит, `+0xb70 += 3 000 000`.
- Выбор следующего ходящего (`167d10`, после `select`): всем существам поля
  `clear_if(stamp = приоритет нового ходящего)` (`1f6b60`): флаг снимается, если
  `stamp <= +0xb70`.

Итог: ответ восстанавливается **через раунд в той же точке очереди**: в следующем
раунде, когда ходит существо с приоритетом не выше того, что было у атакующего в
момент ответа. В том же раунде флаг не снимется (метка на 3 000 000 ниже любого
приоритета). Если такой точки в следующем раунде не было, флаг снимется с первым
ходом через раунд.

`can_retaliate` из `begin_attack` вызывается с `n = 1`, поэтому флаг блокирует ответ
(условие `+0xa9 && n < 2`, `retaliation.md`).

## Что дальше для порта

1. Сверить исполнением (Unicorn): `1f1d10`, `1f6b20`/`1f6b60`/кусок `1ecfb0`,
   решение `begin_attack` (одновременно / первый / защитник первым) на флагах.
2. Портировать в `BattleState`: удар без подхода — `begin_attack` с одновременным
   ответом, повторные удары, флаг ответа с меткой, `add_damage` с удачей цели (#66).
3. Остаются: ветви 0x54/0x1d/0x00/0x09, `t_attack::resolve` по эффектам, слоты
   `+0x14`/`+0x38`, смысл `bf+0x314 < 4`, четвёртый аргумент `1ee240`.
