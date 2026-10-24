# h4r — реверс Heroes of Might and Magic IV

Рабочая папка для разбора движка «Героев 4»: форматы данных, структура `heroes4.exe` и утилиты. Цель — понять, насколько реально переписать движок (нативно под macOS или постепенной заменой функций в оригинальном exe). Пока это исследование, а не проект с кодом движка.

Отвечать по-русски; код, комментарии и коммиты — по-английски.

## Окружение

- Игра: версия от Буки с патчем + Equilibris, русская озвучка.
- Запуск: Mac M1 Pro, macOS 15, Sikarugir (Wine 10 + Rosetta), обёртка `~/Applications/Sikarugir/heroes4.app`.
- Папка игры (дальше `$G`):
  `~/Applications/Sikarugir/heroes4.app/Contents/SharedSupport/prefix/drive_c/Program Files (x86)/Heroes of Might and Magic IV/`
- Python — системный 3.9 (`datetime.UTC` нет, `match` нет). Скрипты запускать с `python3 -I`.
- Соседний репозиторий `~/Documents/projects/h4-mp3dec-fast` (GitHub `demen1n/h4-mp3dec-fast`) — быстрый `Mp3dec.asi`, плюс там же копии `h4r_quiet.py`/`h4r_merge.py`. Здесь — их версии на общем модуле `tools/h4r.py`.

### Что уже изменено в установленной игре (не забыть при экспериментах)

| файл | изменение | оригинал |
|---|---|---|
| `$G/Mp3dec.asi` | наш декодер на minimp3/SSE2 (x87 под Rosetta тормозил) | `Mp3dec.asi.orig` |
| `$G/data/updates.h4r` | + 178 звуков `sound.spell.*`, приглушённых до −24 dBFS | `updates.h4r.orig` |
| `$G/ddraw.ini` | секция `[heroes4]`: окно 1800×1048 в (0,44), `maintas=false` | `ddraw.ini.bak`, `.bak2` |
| `Contents/Info.plist` | `WINEDEBUG=-all`, `WINEESYNC=0` | `Info.plist.bak` |

Рендер идёт через cnc-ddraw из `Frameworks/renderer/cnc_ddraw` обёртки; `$G/DDRAW.dll` (Heroes4GLD) не используется.

## Rust-движок (начало)

Цель — открытый кроссплатформенный (Mac/Windows/Linux) движок. Пользователь не силён в программировании и учит Rust на этом проекте: код пишем понятным, с комментариями «зачем», без хитрых трюков; объяснять решения.

Workspace в корне (`Cargo.toml`), Rust 1.99:
- `crates/h4-formats` — чтение данных без графики: `archive` (`.h4r`, порядок загрузки, алиасы, чтение по требованию — `heroes4.h4r` 677 МБ), `sprite` (слой, группа слоёв, `ActorSequence` v0–v3), `adv_object` (модели объектов карты), `catalog` (актёры → действия → направления), `png` (запись RGBA). `examples/check_sequences.rs` — разбор всех 13 336 анимаций (сейчас 0 ошибок).
- `crates/h4-rules` — правила без графики. `random` — оба генератора оригинала бит-в-бит (глобальный LCG как MSVC `rand`, засев `time()`; боевой — число по индексу из (id, id, назначение, счётчик), `decomp/random.md`), `damage` — урон по `decomp/damage.md` (бросок, модификаторы, пределы, журнал `DamageEvent`), `creatures` — статы из `table.creatures` (читатель таблиц — `h4-formats/src/table.rs`, ресурс `table.creatures.h4d`).
- `crates/h4-viewer` — просмотрщик на macroquad (Metal на маке). `h4-viewer [data]`; клавиши: ←/→ актёр, ↑/↓ действие, Q/E поворот, Space, `,` `.`, `[` `]` скорость, `-` `=` зум, S тень. `h4-dump <actor> <action> <dir> out.png` — кадры в PNG без окна (для проверки).
- `crates/h4-game` — прототип карты приключений (macroquad): `world.rs` (тайлы, генератор тестовой карты 40×40, поиск пути BFS), `terrain_view.rs` (запекание земли в текстуры 1024², изометрия `Iso`), `actor_view.rs` (ленивые анимации, направление шага), `main.rs` (камера, герой ходит по клику, стоящие существа, сортировка по глубине). `H4_SCREENSHOT=out.png` — снять кадр и выйти (для проверки без человека).
- Стыковка земли (найдено экспериментом, `examples/terrain_tiling.rs`): метатайлы кладутся каждые 5 тайлов по обеим осям, внахлёст; первое число в имени — набор (вид), второе — чередование шахматкой. Дыр нет, швов не видно. Дороги — так же (`terrain.road.<вид>.<1|2>`), прокладывать без диагоналей, иначе пунктир. Границы между видами земли пока резкие (нет масок `transition`).
- `adv_actor.*` (`h4-formats/src/adv_actor.rs`, все 154 читаются): u16 3, u8 1, u16 prewalk/walk/postwalk length (шаг на клетку = 32), u16 число действий; действие = имя, u8 кадров/с, 8 имён последовательностей (ne,e,se,s,sw,w,nw,n); в конце offset и flag_offset (i32×2 каждый). Значения полей подтверждены именами методов из мапа (`get_frames_per_second`, `get_walk_length`, `get_flag_offset`). В `h4-game/src/mover.rs` движение по фазам prewalk → walk → postwalk, скорость = длина / длительность анимации (ноги не скользят), в покое wait + изредка fidget.
- Привязка спрайта: i32×2 после слоёв `ActorSequence` (`offset`, у грифона −361,−347) переводят координаты слоёв в систему «ноги в (0,0)». У героев нет `wait`, бездействие — `stand`.
- `ActorSequence` = `t_image_sequence` (rva `349930`): u16 внешний (2 в файлах, 1 в объектах), u16 версия 0–3; у v3 ещё 3×u16 (режим, base frame, base shadow — игра пересчитывает их по именам слоёв), у v1 после offset строка. Слои `base frame`/`base shadow` (анимированные постройки) — в `ActorSequence::base`, рисуются под каждым кадром.
- `adv_object.*` (`h4-formats/src/adv_object.rs`, все 2487 читаются ровно до конца, `examples/check_adv_objects.rs`; подробности и rva — `decomp/adv_object.md`): u16 версия (6), u8 кадров/с, u16 флаг кромок, u16 w, h; биты непроходимости `w*h` (индекс `y*w+x`), кромки-триггеры, блокировка кромок, высоты вершин; тип (3× id + имя: `decorative/tree/autumn`); u8 режим плоских клеток; флаг + смещение флага; u8 overlay/underlay; картинка = u16 версия + `ActorSequence`; у overlay u32 n + n×i16 глубины 32-пиксельных столбцов. Привязка: (0,0) после offset — верхний угол первой клетки; ось x футпринта — экран вниз-влево (`row`), y — вниз-вправо (`col`). В `h4-game/src/object_view.rs` объекты стоят на карте (лес, горы, постройки у старта), непроходимые клетки блокируют путь, картинка режется на столбцы и сортируется вместе с героями; тени рисуются сразу после земли. `H4_FOOTPRINTS=1` — обвести клетки объектов.
- Путь к данным: аргумент → `$H4_DATA` → обёртка Sikarugir (`h4_formats::archive::data_dir`).

```sh
cargo build --release && ./target/release/h4-viewer
cargo test --release --workspace   # ~10 s; tests on game data skip themselves without the game
cargo run --release --example check_sequences -- "$H4_DATA"
```

## Содержимое папки

```
tools/h4r.py          list/extract .h4r; read_dir()/unpack() — общий модуль
tools/h4d_table.py    table.*.h4d -> TSV
tools/pe_info.py      секции, импорты, RTTI, имена исходников из exe
tools/h4r_quiet.py    приглушить громкие PCM-звуки (нужно для updates.h4r)
tools/h4r_merge.py    слить .h4r, первый архив выигрывает
re/                   вывод pe_info.py по heroes4.exe
extracted/text/       сырые файлы из text.h4r
extracted/tables/     они же в TSV (UTF-8)
```

`extracted/` — данные игры, никуда не публиковать. Перегенерировать:

```sh
python3 -I tools/h4r.py extract "$G/data/text.h4r" extracted/text
for f in extracted/text/table.*.h4d; do b=$(basename "$f" .h4d); python3 -I tools/h4d_table.py "$f" "extracted/tables/${b#table.}.tsv"; done
python3 -I tools/pe_info.py "$G/heroes4.exe" re
```

## heroes4.exe

- PE32, MSVC 6 (linker 6.00), собран 2003-03-25 14:33 UTC. PDB нет.
- `.text` 5.7 МБ, ~12 тыс. функций (уникальные цели `call rel32`, грубо). Прикидка исходника — 150–300 тыс. строк C++.
- Импорты (`re/imports.txt`): DirectDraw (`DirectDrawCreateEx`, чистое 2D), DirectSound, Miles 6.1c только через RIB (`RIB_find_files_provider`, `RIB_request_interface`), Bink (ролики), WSOCK32 по ординалам (сеть), реестр, `timeGetTime`. Никаких скриптовых движков.
- **RTTI сохранён** (`re/rtti_types.txt`, `re/classes.txt`): ~1100 классов, нотация NWC `t_*`. Основные группы:
  - `t_adv_*` (~70) — объекты карты: `t_adv_sphinx`, `t_adv_windmill`, `t_adv_teleporter_entrance`…
  - `t_script_*` (~120) — скрипты карт как дерево выражений/действий: `t_script_expression_or`, `t_script_give_creatures`, `t_script_action_base<N, T>` (N — id действия в формате карты).
  - `t_combat_*` (~45) — бой: `t_combat_creature`, `t_battlefield`, `t_combat_path_finder`, ИИ — `t_combat_ai_{melee,ranged_single,ranged_area,spell,move,wait,defend}_action`.
  - Заклинания: обычные описаны таблицей, особые — классами через `t_spell_factory<T>` (`t_fireball`, `t_chain_lightning`, `t_quicksand_spell`, `t_steal_enchantment`, `t_spell_martyr`, `t_summoning_spell`, `t_teleport_spell`…), массовые — в `mass_spell.cpp` (`t_spell_prayer`, `t_spell_plague`, `t_spell_mana_flare`…).
  - Свойства артефактов — вложенные классы `t_artifact_prop::*`.
  - UI/обработчики — шаблоны `t_bound_handler_N`, `t_abstract_function_N` (самописные делегаты).
  - Сеть: `t_network_manager`, `t_network_message`, `network_combat.cpp`.
- **Имена исходников** (`re/source_files.txt`, 89 шт.): утекли через анонимные неймспейсы в RTTI (`.?AVt_acid@?%E:\HMM4\SOURCE\game\resurrection_spell.cpp...@@`) и строки assert. Корень `E:\HMM4\SOURCE\game\`. Важные для логики: `attack.cpp` (вампиризм, огненный щит, пожирание), `run_combat.cpp`, `combat_ai.cpp`, `adventure_ai.cpp`, `combat_creature.cpp`, `hero.cpp`, `town.cpp` (условия построек: `t_requires`, `t_and`, `t_or`, `t_not`), `panic.cpp`.
- Строковые ключи интерфейса и сообщений (`no_spell_points.has.spells`, `immune_to_fire.are.spells`…) — ключи в `strings.Text.h4d`/`table.Interface.h4d`.

Следующий шаг по exe — Ghidra + восстановление классов по RTTI (vtable → методы → имена классов). После этого искать формулы урона в `attack.cpp`/`t_combat_creature`.

## Архивы .h4r

`"H4R\x05"`, u32 смещение каталога, gzip-блобы (заголовок с XFL/OS = `00 0b`). Каталог: u32 число записей; запись = u32 offset, csize, size, time; три строки с u16-длиной (имя, исходный путь, alias); u32 тип. Запись с alias своих данных не имеет.

Порядок загрузки `heroes4.exe` (lsof на работающей игре): `x2_override`, `updates`, `text`, `music`, `movies`, `x2`, `storm`, `heroes4`. Первый найденный ресурс выигрывает, поэтому подменять что-то удобно через `updates.h4r`. Строки exe упоминают ещё `new.h4r`, `storm_updates`, `x2_updates` (файлов нет) и `storm_override` (файл есть, но в lsof его не было — не проверено). `new_mod.h4r` из `h4.dll` этот exe не грузит.

В `$G/data` есть ещё `equi.h4r`, `equi_text.h4r`, `equi_music.h4r`, `equi_hd_16_9.h4r`, `res_mod.h4r` и `*.aop` — `heroes4.exe` их не открывает, видимо, это для `h4mod.exe` (Equilibris). `updates.h4r` кроме звуков содержит `table.combat_obstacles`, `table.combat_grid_colors`, `table.spell_artifact_costs`.

Что лежит где (по префиксу имени):

- `heroes4.h4r`: `actor_sequence` 11285, `adv_object` 2767, `combat_object` 1048, `sound` 1028, `layers` 656, `animation` 325, `combat_actor` 139, `adv_actor` 137, `terrain` 61, `battlefield_preset_map` 24, `font` 21, `castle` 19, `game_maps` 7.
- `x2.h4r` / `storm.h4r`: дополнения (layers, sound, кампании `game_maps.*_campaign.*`, `bink.*_intro`, `bitmap_raw.menu.main.*`). В `x2` ещё копия таблиц.
- `text.h4r`: 13 `table.*.h4d` и 5 `strings.*.h4d` — русский текст, перекрывает английские таблицы в `x2.h4r`.

### Ресурс sound

19 байт заголовка: byte0 — формат (0 PCM, 1 MP3), byte3 — бит на сэмпл, byte4 — каналы, далее u32 частота; затем данные. Заклинания — PCM 16/8 бит моно 22050 Гц. MP3 декодируется через Miles `Mp3dec.asi`.

### table.*.h4d

u32 число строк; строка = u16 число ячеек + ячейки (u16 длина + cp1251). Первые 2 строки — шапка с объединёнными ячейками, данные с 3-й строки. Есть байты 0x98 (не определены в cp1251) — декодировать с `errors='replace'`.

Таблицы: `creatures` (80 строк: уровень, школа, HP, урон, атака, защита, движение, скорость, выстрелы, мана, прирост, цены, опыт, описания), `Spells` (197: уровень, цена, школа, сила, флаги целей/иммунитетов, AI-веса), `creature_abilities`, `skills`, `skill_weights`, `heroes`, `Artifacts`, `buildings`, `creature_banks`, `Adventure Object`, `combat_obstacles`, `random_names`, `Interface`.

Вывод: цифры (статы, цены, флаги) — в данных; в коде — формулы, особые заклинания и способности, ИИ, скрипты.

### Графика: t_bitmap_layer_24 (`tools/h4_sprite.py`)

Читатель — rva `195570` (`t_bitmap_layer_24::read`, найден по порядку линковки: после трёх `adjust_*` и хелпера `read` в `1954e0`; перенесённое имя на `1cf00` «t_actor_sequence_24::read» — ошибка, там STL-дерево). Слой:
u16 размер палитры (256), u16 флаг block-alpha, палитра **BGR**; u16-строка имя (`frame 001`, `shadow 001`, `Pressed`…); u8 глубина альфы (4); i32 left, top, right, bottom; строки по 8 байт (u16 x0, u16 x1 относительно left, u32 смещение); N байт индексов; 4-битная альфа `(N+1)>>1` (младший полубайт — чётный пиксель); при флаге — ещё `(N+131)>>7` байт (block alpha, не используется).

- `layers.*`: u16 число слоёв + слои (кнопки: Disabled/Highlighted/Pressed/Released).
- `actor_sequence.*`: u16 2, u16 2, u16 число слоёв (кадры, затем столько же теней) + слои + хвост 10 байт (2×i32, видимо точка привязки, + u16).
- Проверено на выборке: `actor_sequence` 29/30, `animation` 30/30, `layers` 30/30, часть `font`, `bitmap_raw`, `combat_object`.

### Земля: метатайлы `terrain.*` (`crates/h4-formats/src/terrain.rs`)

Все 61 файл читаются. Метатайл = 10×10 изометрических тайлов 64×32 с общей палитрой (BGR); тайл — 64 столбца высотой 1,1,3,3…31,31…1 по центру, данные столбца — чередование `skip` / `count + индексы`. Края метатайла рваные и угловые тайлы пустые: игра накладывает метатайлы друг на друга и смешивает масками `transition.Transitions` (не разобран). Варианты `<type>.1.1/1.2/2.1/2.2`; `magic.*` — маленькие. `examples/dump_terrain.rs` рисует все в PNG.

### Переходы: `transition.Transitions.h4d` (`h4-formats/src/transition.rs`)

u8 число наборов (15: `land 1–3`, `road 1–2`, `water 1–2`, `river 1–2`, `water_to_land 1–3`, `land_to_water 1–3`); набор = u16-строка имя, u32 число масок (93), маска = i32 код, u8 глубина (1 или 4 бита), данные 160/544 байт (по столбцам ромба, столбец добит до байта). Размеры из таблицы в exe `0x9978cc`, читатель маски — rva `4c4e00` (имя в мапе съехало).
Код = цифра на угол ромба: тысячи E, сотни S, десятки W, единицы N; 1 — с ребра перед углом (по часовой от N), 2 — с ребра после, 3 — с обоих, 4 — только диагональный сосед (`corner_code`). Проверено по покрытию масок.
Тип земли в игре — пара (тип, подтип), enum из exe (`0x5958d8`): water, grass, rough, swamp, volcanic, snow, sand, dirt, subterranean, river, lava, ice, special_*. Порядок обработки (`0x996828`): реки, затем 0..8, затем special — взят как приоритет наложения (выше рисуется поверх). Подтипов у 0..8 по 2.
В `h4-game/src/terrain_view.rs`: тайл собирается из 4 метатайлов, поверх — старшие соседи через маску (`land N`; суша над водой — 4-битные `land_to_water N`; подтипы воды — `water N`), вариант набора — хэш клетки (это для сгенерированной карты).
В картах переходы уже посчитаны редактором: в тайле список (terrain, subtype, id, mask_set) и для дорог (road, id, mask_set). `mask_set` 0–19 → имя набора по таблице exe `6877a0` (0–4 `water_to_land`, 5–9 `land_to_water`, 10–14 `road`, 15–19 `land`; `MAP_MASK_SETS`), `id` — индекс маски в порядке файла (`Transitions::map_mask`). Игра рисует: свою землю, затем переходы в сохранённом порядке; дорога: если у тайла её нет — чужие дороги через маски, если есть — своя дорога, переходы с road 0 вырезают её край, с другим типом — рисуют поверх. Подробно — `decomp/terrain_draw.md`, код — `TerrainView::build_map`.

### Сверить с игрой (визуально, когда будет возможность)

- Темп анимаций и скорость героя на карте (по ощущению пользователя «немного не так»).
- Приоритет наложения земли (кто поверх кого) и выбор набора масок; береговые 4-битные маски дают «мелководье» ступеньками — возможно, неверно выбираются маски для угловых клеток.
- Привязка спрайтов к клетке (`offset` анимации + `offset` adv_actor).

### Карты `.h4c`

`"H4CAMPAIGN"`, u32 0x16, u8 0 (не мультисценарий), затем один gzip-поток (`t_adventure_map::read_from_map`, rva `d15c0`): заголовок карты → события, разрешённые герои/артефакты/заклинания, AI-карты, объекты (не разобрано) → тайлы всех уровней → по байту высоты на вершину. Подробно — `decomp/h4c.md`. Читатель — `h4-formats/src/map.rs`, прогон — `examples/check_maps.rs` (106/106). Заголовок (версии 20..29, в установке 27 и 29): размер 76/152/228/304, уровни, до 6 игроков, имя, описание, команды, тексты победы и поражения, сцены, заметки. Карта — ромб в квадрате size×size (`row_ranges`, порт `base_initialize`), на экране это прямоугольник. Тайл: terrain (enum из 19), subtype, road, rock, visibility u32, плюс **готовые переходы** земли и дорог (id маски ≤ 92, mask_set ≤ 19). Начало тайлов пока находится поиском с конца (единственное смещение, при котором тайлы и высоты сходятся ровно до конца потока). Показать карту: `H4_MAP=<.h4c> [H4_LEVEL=1] [H4_START=col,row] h4-game` — земля, реки, магические земли и дороги по сохранённым переходам, без объектов; область вне ромба не рисуется. Ходьба пока считает реки водой. Одиночных `.h4s` в установке нет — все карты `.h4c`.

### Не разобрано

- `strings.*.h4d` — другой формат (u32, u32, u16-строка с заголовком «General string table for Heroes 4»…).
- `combat_object` (большая часть), `adv_actor`/`combat_actor` (описатели: списки анимаций по действиям и направлениям, `Griffin.combat.die.ne`…), `terrain`, `castle`, `battlefield_preset_map`, `transition`, `font` (часть) — свои заголовки, внутри, вероятно, те же слои.
- `game_maps.*`, карты `.h4c`/`.h4s` (`$G/maps`, сейвы в `$G/games`), кампании `.h4c`.
- `terrain`, `font`, `battlefield_preset_map`.

## Прочие бинарники в папке игры

- `h4mod.exe` (6.9 МБ, 2016) — видимо, exe Equilibris; интересно сравнить с `heroes4.exe` (что моддеры патчили).
- `c_e_mod.exe` (2008) / `campaign_editor.exe` (2003) — редактор кампаний и карт.
- `h4.dll` (2021), `h4_camp.dll` (2019) — от мода; `h4.dll` знает про `new_mod.h4r`.
- `Mss32.dll` — Miles 6.1c. `binkw32.dll` — Bink.

## Символы: sushi-shi/homm4-symbols

Клон в `third_party/homm4-symbols` (123 МБ, без лицензии, не наш). Основа — слитый линкер-мап отладочной сборки `heroes4_debug.exe` (MSVC 6, октябрь 2002): `symbols/heroes4_debug.map`, 71 988 символов с полными mangled-сигнатурами, 832 объектника в порядке линковки (`symbols/objs.tsv`). Исходники лежали в `C:\work\game\`. `src-structure/game/` — сгенерированные заглушки .cpp по единицам трансляции (без тел).

Готовые карты имён есть только для двух exe: `cht-1.x` (китайская 1.x) и `complete-3.0` (англ. Complete DVD 3.0WoW). Наш `heroes4.exe` — тоже 3.0WoW, но русская сборка Буки, sha256 `175eacb4…36ff`, не совпадает ни с одной. Проверено: RTTI type descriptor'ы из `complete-3.0/names.tsv` у нас лежат со сдвигами, разными по участкам (+72…+2096 байт), — `names.tsv` напрямую не применить. Перенос имён у автора делается сравнением ассемблера двух exe (`tools/release_align.py`), а их бинарники в репо не входят (`binaries/` локальный). Варианты: достать англ. Complete/GOG exe и выровнять наш к нему их пайплайном; или повторить их подход сами (Ghidra + vftable/RTTI + сигнатуры из мапа). Тулзы ждут nix-Python (`tools/py`) с pefile+capstone — заменяется venv.

Даже без привязки к нашему exe мап полезен как справочник: имена методов, сигнатуры, какие функции в каком .cpp (например `t_abstract_combat_creature::get_damage_low`, `get_damage_spell_value(t_battlefield&, bool, t_spell, int)` в `combat_creature.obj`).

## Ghidra

Установка: `brew install ghidra` (12.1.4, тянет openjdk@25 keg-only). Headless:

```sh
export JAVA_HOME="$(brew --prefix openjdk@25)/libexec/openjdk.jdk/Contents/Home" GHIDRA_HEADLESS_MAXMEM=8G
AH="$(brew --prefix ghidra)/libexec/support/analyzeHeadless"
"$AH" ghidra heroes4 -process heroes4.exe -noanalysis -readOnly -scriptPath tools/ghidra \
  -postScript Decompile.java "$PWD/decomp/out.c" <depth> <rva> [rva...]
```

- Проект `ghidra/heroes4.gpr`, программа `heroes4.exe` (копия exe в `bin/`). Автоанализ ~3 мин.
- Уже сделано: автоанализ → `PaddedFunctionStarts.java 4fc000` (+9047 функций; EH-funclet'ы начинаются ~`0x4fc000`) → `ExportFeatures.java` → `transfer_names.py` → `ApplyNames.java`. Итого ~55k функций, 5293 имени.
- `tools/transfer_names.py`: перенос имён complete-3.0 → наш exe по форме функций (размер, число инструкций, `ret N`): 2522 якоря + 1780 в полосах между ними из 4823, плюс 991 vftable по RTTI-имени. Проверка: виртуальные методы попадают в vftable своего класса/наследника (расхождения — формат имён анонимных неймспейсов). Цель `rus-3.0` заведена в клоне: `maps/rus-3.0/{target.json,names.tsv}`, `work/rus-3.0/features/`, `binaries/rus-3.0/`.
- Python для их тулз: `HOMM4_PYTHON=.venv/bin/python third_party/homm4-symbols/tools/py tools/<x>.py rus-3.0` (venv с pefile+capstone).
- `tools/ghidra/Decompile.java` — декомпиляция по rva с обходом вызываемых на заданную глубину.
- Декомпилят x87-кода (float, `__ftol`) местами теряет аргументы — смотреть дизассемблер.

Результаты: `decomp/damage.md` (+ `damage.c`) — формула урона; `decomp/random.md` — генераторы случайных чисел. `re/enum_creature_ability.tsv` — enum способностей (значение = число в таблице exe − 1).

## Смежные проекты

- `Anbrose/hommiv-remake-macos` — Swift + Metal, ~32k строк за месяц (явно с LLM), без лицензии. README отстаёт от кода: есть заклинания, звук, ИИ, сейвы. Клон: `scratchpad/remake` прошлой сессии (временный).
- `ihhub/engine4heroes` (GPL-3.0, автор fheroes2), `tallowmere/free-heroes4` (openh4).
- Для ориентира по срокам: VCMI (H3) ~15 лет, fheroes2 (H2) 10+ лет, OpenMW ~15 лет.
