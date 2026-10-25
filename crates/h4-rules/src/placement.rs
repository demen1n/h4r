//! What the random objects of a map become when a game starts: random towns get a faction,
//! random monsters a creature and a stack size, random resource piles a resource...
//!
//! How the game does it (`t_adventure_map::read_from_map`, rva `d15c0`, right after the
//! tiles are read):
//!
//! 1. "Pre-placement": every object, in the order of the map file, gets `preplacement(map, 0)`
//!    (vtable slot 29). Random towns, dwellings, shrines and temples choose their kind here.
//!    An object tied to a town that is itself random answers "not yet" and is put aside.
//! 2. The put-aside objects are asked again with pass 1, 2, ... until none is left; by then
//!    every town has its type. Armies with heroes always wait for pass 1: that is when a
//!    hero the map leaves random gets its class and gender (`t_hero::preplacement`, rva
//!    `330e90`, calls `derandomize`, rva `3311f0`).
//! 3. "Placement": every object, in file order, gets `place(map, point)` (slot 28). A random
//!    object creates the real one on its spot here: a pile of a random resource, a monster
//!    army, an artifact.
//!
//! All the choices use the global generator ([`GlobalRandom`]), so with the same seed the
//! same map comes out the same. The game seeds it with the time, so a particular game can't
//! be reproduced anyway; what matters is that the rules (which creatures, how many, which
//! resources) are the game's. The order of the rolls follows the game where we know it, but
//! some objects roll numbers we don't model yet (listed in [`place_random_objects`]), so the
//! sequence is not bit-exact with the exe.

use h4_formats::map::{MapFile, MapHeader};
use h4_formats::map_army::{Army, Stack};
use h4_formats::map_body::{ObjectKind, Placed, RandomLink};

use crate::artifacts::{self, ArtifactInfo, ArtifactPool};
use crate::creatures::{self, CreatureStats, TOWN_TYPES};
use crate::heroes::BASE_CLASSES;
use crate::random::GlobalRandom;

/// What a random object turned into.
#[derive(Debug, Clone, PartialEq)]
pub enum Placement {
    /// A town of this type (index into [`TOWN_TYPES`]) drawn with `model`.
    Town { town_type: usize, model: String },
    /// A monster army: (creature id, count) stacks, the first is the main one.
    Monster { stacks: Vec<(usize, i32)> },
    /// A creature dwelling for this creature id.
    Dwelling { creature: usize, model: String },
    /// A pile of `amount` of a resource (index into [`MATERIALS`]).
    MaterialPile { material: usize, amount: i32, model: String },
    /// A building that gives `amount` of `material` every week.
    WeeklyGenerator { material: usize, amount: i32, model: String },
    /// A shrine of a magic school (index into [`TOWN_TYPES`] order: life, order, death,
    /// chaos, nature) and spell level.
    Shrine { school: usize, level: u32, model: String },
    /// A temple of an alignment (life, order, death, chaos, nature).
    Temple { alignment: usize, model: String },
    /// A teacher of a primary skill (0..=8: tactics, combat, scouting, nobility, life,
    /// order, death, chaos, nature magic).
    Teacher { skill: usize, model: String },
    /// An artifact (id, see [`crate::artifacts::KEYWORDS`]) lying on the map.
    Artifact { artifact: usize, model: String },
    /// What an army got: one entry per hero in stack order, (base class, male) once random
    /// classes and genders are chosen (`None` if still unknown: a hero with a portrait
    /// but no class takes the portrait's, which we don't have yet), and the size of each
    /// stack the map left at 0, as (stack index, count).
    Army { heroes: Vec<Option<(usize, bool)>>, counts: Vec<(usize, i32)> },
}

impl Placement {
    /// The `adv_object` model to draw, if the placement changes it.
    pub fn model(&self) -> Option<&str> {
        match self {
            Placement::Town { model, .. }
            | Placement::Dwelling { model, .. }
            | Placement::MaterialPile { model, .. }
            | Placement::WeeklyGenerator { model, .. }
            | Placement::Shrine { model, .. }
            | Placement::Temple { model, .. }
            | Placement::Artifact { model, .. }
            | Placement::Teacher { model, .. } => Some(model),
            Placement::Monster { .. } | Placement::Army { .. } => None,
        }
    }
}

/// Resources in the game's order.
pub const MATERIALS: [&str; 7] = ["gold", "wood", "ore", "crystal", "sulfur", "mercury", "gems"];

/// What the game knows from the game setup that the map doesn't say.
#[derive(Debug, Clone)]
pub struct Setup {
    /// 0..=4, easy to impossible. Random monsters are this much stronger:
    /// [`DIFFICULTY_STRENGTH`].
    pub difficulty: usize,
    /// The town type each player color plays (index into [`TOWN_TYPES`]), `None` if the
    /// color isn't in the game. Random objects tied to a player take this type.
    pub player_town_types: [Option<usize>; 6],
}

impl Setup {
    /// A setup from the map alone, for showing a map without a setup screen: a player whose
    /// header allows exactly one town type plays it; for the others we don't know what the
    /// player would pick, so they count as "not known" (`None`) and objects tied to them are
    /// chosen at random.
    pub fn from_header(header: &MapHeader, difficulty: usize) -> Setup {
        let mut player_town_types = [None; 6];
        for p in &header.players {
            let allowed: Vec<usize> = (0..6).filter(|t| p.town_types & (1 << t) != 0).collect();
            if let (&[t], Some(slot)) = (allowed.as_slice(), player_town_types.get_mut(p.color as usize)) {
                *slot = Some(t);
            }
        }
        Setup { difficulty, player_town_types }
    }
}

/// Random monster strength factor by difficulty (doubles at VA `0x990028`).
pub const DIFFICULTY_STRENGTH: [f64; 5] = [0.6667, 1.0, 1.5, 2.0, 3.0];

/// Model names of resource piles by material (VA `0xa737b4`).
const PILE_MODELS: [&str; 7] = [
    "resources.gold", "resources.wood", "resources.ore", "resources.crystal", "resources.sulfur",
    "resources.mercury", "resources.gem",
];

/// Model names of weekly generators by material (VA `0xa87e14`; there is none for gold)
/// and how much they give by default (VA `0x997ff0`).
const WEEKLY_MODELS: [&str; 7] = [
    "", "mine.weekly.woodcutters_cottage", "mine.weekly.miner's guilds", "mine.weekly.crystal pool",
    "mine.weekly.imp pit", "mine.weekly.apprentice's lab", "mine.weekly.leprechaun",
];
const WEEKLY_AMOUNTS: [i32; 7] = [1000, 10, 10, 5, 5, 5, 5];

/// Dwelling model names by creature id, after `creature generators.` (VA `0x9740d0`).
/// Empty: the creature has no dwelling.
const DWELLING_MODELS: [&str; 74] = [
    "Altar of Air", "Altar of Light", "Ballista Works", "Den of Thieves", "Behemoth Crag",
    "PillarOfEyes", "Dragon Cave", "Dragon Graveyard", "Centaur Stables", "Kennels",
    "Knight's Chapter", "Archery Range", "Barracks", "Cyclops Cave", "Spawning Pit",
    "Temple of the Damned", "Dragon Factory", "Dwarven Mines", "Altar of Earth", "Lava Tube",
    "Homestead", "Magic Forest", "Altar of Fire", "Parapet", "Altar of Wishes", "Barrow Mound",
    "Berserker Dwelling", "Golem Factory", "Griffin Cliffs", "Halfling Burrow", "Harpy Nest",
    "Hydra Pond", "Ice Cave", "Torture Chamber", "Faerie Mound", "Mage Tower", "Mantis Nest",
    "Statuary Garden", "", "Minotaur's Maze", "Monestary", "Cursed Pyramid", "Golden Pavilion",
    "Black Wood", "Nomad Tents", "Ogre Fort", "Orc Camp", "Hovel", "Funeral Pyre", "Guardhouse",
    "Pirates Cove", "Enchanted Grove", "", "Cemetery", "Squire's Guild", "Fae Trees",
    "Cliff Nest", "Cloud Castle", "Troglodyte Warren", "Troll Cave", "Unicorn Glade", "Mansion",
    "Altar of Water", "Tiger Den", "Wolf Den", "Crypt", "hot_house", "Goblin_Armory",
    "Ward_Sorcery", "Gargantuan_Dell", "Dark_Knights_Sanctum", "Siege_Workshop", "Beast_Pen", "",
];

/// Teacher models by primary skill (VA `0x971d48`, used by the `t_adv_altar` constructor,
/// rva `2d070`).
const TEACHER_MODELS: [&str; 9] = [
    "Scholars.Tactician", "Scholars.Combat", "Scholars.Scout", "Scholars.Noble", "Scholars.Priest",
    "Scholars.Scholar", "Scholars.Lich", "Scholars.Sorcerer", "Scholars.Druid",
];

/// Shrine model names use the school with a capital letter: `shrines.Life 1`.
const SCHOOLS: [&str; 5] = ["Life", "Order", "Death", "Chaos", "Nature"];

/// Temple models by alignment. Matched by name (the temple's `place`, rva `73790`, isn't
/// traced): life is "Light", death "Darkness".
const TEMPLE_MODELS: [&str; 5] = [
    "moral-luck boosters.Temple of Light", "moral-luck boosters.Temple of Order",
    "moral-luck boosters.Temple of Darkness", "moral-luck boosters.Temple of Chaos",
    "moral-luck boosters.Temple of Nature",
];

// creature ids the code treats specially (see `creatures::KEYWORDS`)
const MERMAID: usize = 38;
const PEASANT: usize = 47;
const PIRATE: usize = 50;
const SEA_MONSTER: usize = 52;

/// The answer of the "random object kind" (rva `75c950`) in the game's terms: an alignment
/// 0..=5, 6 "anything", 7 "ask me later" (a town not chosen yet).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Link {
    Alignment(usize),
    Any,
    Later,
}

/// Choose what every random object of `map` becomes. `creatures` are the creature stats in
/// game enum order ([`creatures::in_enum_order`]). The result has one entry per object of
/// the map body, `None` for objects that aren't random or that we don't handle yet.
///
/// Not handled yet (their rolls are skipped, which shifts the numbers after them):
/// the spell of a random shrine and conservatories, heroes in prisons, stacks with
/// a random count in armies, and whatever the real objects created here roll in their own
/// placement (towns, for example).
pub fn place_random_objects(
    map: &MapFile,
    creatures: &[CreatureStats],
    artifacts: &[ArtifactInfo],
    setup: &Setup,
    rng: &mut GlobalRandom,
) -> Vec<Option<Placement>> {
    let Ok(body) = &map.body else { return Vec::new() };
    let mut state = State {
        map,
        creatures,
        artifacts: ArtifactPool::new(artifacts, body.allowed_artifacts.as_deref(), map.header.expansion),
        setup,
        result: vec![None; body.objects.len()],
        town_types: Vec::new(),
    };
    // Real towns are named; random objects can be tied to them by name. A real town's type
    // is in its model (`castle.Asylum.Fort`).
    for o in &body.objects {
        if let ObjectKind::Placed(p) = &o.kind {
            if p.type_name == "town" {
                if let (Some(name), Some(t)) = (&p.name, town_type_of_model(&p.model)) {
                    state.town_types.push((name.clone(), t));
                }
            }
        }
    }

    // 1. pre-placement, pass 0, and 2. the put-aside objects until none is left
    // 0. While the map is read, a creature stack stored with a count of 0 gets a random
    // size (`t_creature::read`, rva `63dc20`), so these rolls come first, in file order.
    for (i, o) in body.objects.iter().enumerate() {
        let army = match &o.kind {
            ObjectKind::Army { army, .. } => Some(army),
            ObjectKind::Placed(p) => p.garrison.as_ref(),
        };
        let Some(army) = army else { continue };
        let counts = state.random_stack_sizes(army, rng);
        // garrisons (towns, mines) roll too, but we keep only the armies' sizes for now
        if matches!(o.kind, ObjectKind::Army { .. }) && !counts.is_empty() {
            state.result[i] = Some(Placement::Army { heroes: Vec::new(), counts });
        }
    }

    let mut later = Vec::new();
    for (i, o) in body.objects.iter().enumerate() {
        let wait = match &o.kind {
            ObjectKind::Placed(p) => state.preplace(i, p, 0, rng),
            ObjectKind::Army { army, .. } => army.stacks.iter().any(|s| matches!(s, Stack::Hero(_))),
        };
        if wait {
            later.push(i);
        }
    }
    let mut pass = 1;
    while !later.is_empty() {
        later.retain(|&i| match &body.objects[i].kind {
            ObjectKind::Placed(p) => state.preplace(i, p, pass, rng),
            ObjectKind::Army { owner, army, .. } => {
                state.derandomize_heroes(i, *owner, army, rng);
                false
            }
        });
        pass += 1;
        // Every link is a town name, and every town has a type after pass 0, so pass 1
        // answers all of them. Guard anyway rather than loop forever on a surprise.
        if pass > 10 {
            break;
        }
    }

    // 3. placement
    for (i, o) in body.objects.iter().enumerate() {
        if let ObjectKind::Placed(p) = &o.kind {
            state.place(i, p, o.x, o.y, o.level, rng);
        }
    }
    state.result
}

struct State<'a> {
    map: &'a MapFile,
    creatures: &'a [CreatureStats],
    artifacts: ArtifactPool<'a>,
    setup: &'a Setup,
    result: Vec<Option<Placement>>,
    /// Town name → town type, for objects tied to a town. Random towns are added as they
    /// get their type.
    town_types: Vec<(String, usize)>,
}

impl State<'_> {
    /// What a random object's link means right now (rva `75c950`). In pass 0 a link to a
    /// town is always "later", even if the town is a real one.
    fn link(&self, link: &Option<RandomLink>, pass: u32) -> Link {
        match link {
            None | Some(RandomLink::None) => Link::Any,
            Some(RandomLink::Player(color)) => {
                match self.setup.player_town_types.get(*color as usize).copied().flatten() {
                    Some(t) => Link::Alignment(t),
                    None => Link::Any,
                }
            }
            Some(RandomLink::Town(name)) => {
                if pass == 0 {
                    return Link::Later;
                }
                match self.town_types.iter().find(|(n, _)| n == name) {
                    Some(&(_, t)) => Link::Alignment(t),
                    None => Link::Any,
                }
            }
        }
    }

    /// One object's pre-placement. Returns true to be asked again in the next pass.
    fn preplace(&mut self, i: usize, p: &Placed, pass: u32, rng: &mut GlobalRandom) -> bool {
        let link = self.link(&p.random_link, pass);
        match p.type_name.as_str() {
            "random_town" => self.preplace_town(i, p, link, rng),
            "random_creature_dwelling" => {
                if link == Link::Later {
                    return true;
                }
                match self.pick_dwelling_creature(level_of(&p.major), link, rng) {
                    Some(c) => {
                        let model = format!("creature generators.{}", DWELLING_MODELS[c]);
                        self.result[i] = Some(Placement::Dwelling { creature: c, model });
                    }
                    None => eprintln!("random dwelling {:?}: no creature fits", p.major),
                }
                false
            }
            // `t_random_altar` pre-placement (rva `2e000`): the skills the link allows (an
            // alignment 0..=4 its magic school 4..=8, might the four might skills, any all
            // nine), cut down to the map's choice; if that leaves none, the map's choice,
            // then the link's. One roll picks among them.
            "random_teacher" => {
                let by_link: u16 = match link {
                    Link::Later => return true,
                    Link::Alignment(a) if a <= 4 => 1 << (4 + a),
                    Link::Alignment(_) => 0b1111,
                    Link::Any => 0x1ff,
                };
                let map_choice = p.teacher_skills.unwrap_or(0x1ff) & 0x1ff;
                let mut skills = by_link & map_choice;
                if skills == 0 {
                    skills = map_choice;
                }
                if skills == 0 {
                    skills = by_link;
                }
                let k = rng.next() % skills.count_ones();
                // the k-th skill in the set, counting from skill 0
                let skill = (0..9).filter(|s| skills & (1 << s) != 0).nth(k as usize).expect("k is below the count");
                self.result[i] = Some(Placement::Teacher { skill, model: TEACHER_MODELS[skill].to_string() });
                false
            }
            // the monster only remembers its alignment here; it is placed later
            "random_monster" | "random_random_monster" => link == Link::Later,
            // shrines (rva `46cc40`) and temples (rva `4736b0`): an alignment 0..=4 is
            // taken as it is, anything else (might, any) is a random one of the five
            "random_shrine" | "random_temple" => {
                let school = match link {
                    Link::Later => return true,
                    Link::Alignment(a) if a <= 4 => a,
                    _ => (rng.next() % 5) as usize,
                };
                self.result[i] = Some(if p.type_name == "random_shrine" {
                    let level = level_of(&p.major).max(1);
                    let model = format!("shrines.{} {level}", SCHOOLS[school]);
                    Placement::Shrine { school, level, model }
                } else {
                    Placement::Temple { alignment: school, model: TEMPLE_MODELS[school].to_string() }
                });
                false
            }
            _ => false,
        }
    }

    /// The heroes of an army get a class and gender if the map leaves them random
    /// (`t_hero::derandomize`, rva `3311f0`). Only heroes without a portrait (-1) are
    /// touched: a random class is a random base class of the owner's alignment (any of the
    /// eleven without an owner), a random gender a coin flip. Then the game picks one of
    /// its standard heroes of that class and gender for name and portrait, one more roll.
    fn derandomize_heroes(&mut self, i: usize, owner: u8, army: &Army, rng: &mut GlobalRandom) {
        let mut out = Vec::new();
        for s in &army.stacks {
            let Stack::Hero(h) = s else { continue };
            let mut class = usize::try_from(h.class).ok();
            let mut male = match h.gender {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            };
            if h.portrait < 0 {
                if class.is_none() {
                    // The owner's alignment. If we don't know it (see `Setup`) we take the
                    // no-owner way, which the game uses only for unowned heroes.
                    let alignment = self.setup.player_town_types.get(owner as usize).copied().flatten();
                    class = Some(match alignment {
                        Some(a) => {
                            let fitting: Vec<usize> =
                                (0..BASE_CLASSES.len()).filter(|&c| BASE_CLASSES[c].alignment == a).collect();
                            fitting[rng.next() as usize % fitting.len()]
                        }
                        None => rng.next() as usize % BASE_CLASSES.len(),
                    });
                }
                if male.is_none() {
                    male = Some(rng.next() % 2 == 1);
                }
                // the standard hero for name and portrait
                rng.next();
            }
            out.push(class.zip(male));
        }
        if out.iter().any(Option::is_some) {
            match &mut self.result[i] {
                Some(Placement::Army { heroes, .. }) => *heroes = out,
                slot => *slot = Some(Placement::Army { heroes: out, counts: Vec::new() }),
            }
        }
    }

    /// Sizes for the stacks of `army` stored with a count of 0 (rva `63dc20` calls `7f4500`
    /// with the creature's level): a strength rolled between 0.8 and 1.2 of the level's
    /// (as for random monsters, but without the difficulty factor), divided by the
    /// creature's experience, at least 1. The game keeps the division's remainder as a
    /// fraction of 256 (`+0xa4`); we drop it.
    fn random_stack_sizes(&self, army: &Army, rng: &mut GlobalRandom) -> Vec<(usize, i32)> {
        let mut out = Vec::new();
        for (slot, s) in army.stacks.iter().enumerate() {
            let Stack::Creatures { kind, count: 0, .. } = s else { continue };
            let Some(c) = usize::try_from(*kind).ok().and_then(|k| self.creatures.get(k)) else { continue };
            let Some((min, max)) = default_strength(c.level as usize) else { continue };
            let strength = (rng.next() % (max - min + 1) as u32) as i32 + min;
            let n = (strength * 256 / c.experience.max(1)).max(256) >> 8;
            out.push((slot, n));
        }
        out
    }

    /// `t_random_town` pre-placement (rva `3f5310`): the linked alignment, or for "any"
    /// the owner's town type, or for an unowned town a random one of the six.
    fn preplace_town(&mut self, i: usize, p: &Placed, link: Link, rng: &mut GlobalRandom) -> bool {
        // In pass 0 a town link means "later"; the town treats that like "any", so towns
        // are never put aside.
        let town_type = match link {
            Link::Alignment(t) => t,
            Link::Any | Link::Later => {
                let owner = p.owner.unwrap_or(6) as usize;
                match self.setup.player_town_types.get(owner).copied().flatten() {
                    Some(t) => t,
                    // The game reads the owner's town type here; a color that isn't
                    // playing can't own a town, so this is only reached for "nobody".
                    None => (rng.next() % 6) as usize,
                }
            }
        };
        // Model: town type × level × left/right (VA `0xa5b6c8`). The level (village, fort,
        // citadel, castle) comes from the town's buildings, which we don't read yet; all
        // real towns of the installed maps use a fort model, so we take that.
        let side = if p.major == "right" { " R" } else { "" };
        let model = format!("castle.{}.fort{side}", TOWN_TYPES[town_type]);
        if let Some(name) = &p.name {
            self.town_types.push((name.clone(), town_type));
        }
        self.result[i] = Some(Placement::Town { town_type, model });
        false
    }

    /// A creature for a random dwelling of `level` (pre-placement, rva `43f020`): a random
    /// one of the level's creatures that have a dwelling, again until the alignment fits
    /// and the map's expansion has the creature.
    fn pick_dwelling_creature(&self, level: u32, link: Link, rng: &mut GlobalRandom) -> Option<usize> {
        // The pool (rva `43bac0`): creatures with a dwelling model, except the pirate,
        // grouped by level, in id order.
        let pool: Vec<usize> = (0..self.creatures.len())
            .filter(|&c| !DWELLING_MODELS[c].is_empty() && c != PIRATE)
            .filter(|&c| self.creatures[c].level == level as i32)
            .collect();
        if pool.is_empty() {
            return None;
        }
        let map_expansion = self.map.header.expansion as i32;
        // The game loops until it finds one; a map that allows none would hang it. We give up
        // after many tries instead.
        for _ in 0..100_000 {
            let c = pool[rng.next() as usize % pool.len()];
            if let Link::Alignment(a) = link {
                if self.creatures[c].alignment != a {
                    continue;
                }
            }
            // As in the exe: a creature of expansion 1 counts as 2 here.
            let needs = match creatures::expansion(c) {
                1 => 2,
                e => e,
            };
            if needs <= map_expansion {
                return Some(c);
            }
        }
        None
    }

    /// One object's placement.
    fn place(&mut self, i: usize, p: &Placed, x: i32, y: i32, level: i32, rng: &mut GlobalRandom) {
        match p.type_name.as_str() {
            "random_material_pile" => {
                // rva `3920a0`, the pile's constructor `391380`
                let material = (rng.next() % 7) as usize;
                let amount = match material {
                    0 => (rng.next() % 5 + 4) as i32 * 100,
                    1 | 2 => (rng.next() % 5 + 4) as i32,
                    _ => (rng.next() % 3 + 2) as i32,
                };
                let model = PILE_MODELS[material].to_string();
                self.result[i] = Some(Placement::MaterialPile { material, amount, model });
            }
            "random_weekly_material_generator" => {
                // rva `4ccf30`: never gold; the constructor (`4cbc20`) then turns it into a
                // 500-gold generator on an even roll, keeping the look of the first choice
                let chosen = (rng.next() % 6 + 1) as usize;
                let (material, amount) = if rng.next() & 1 == 0 { (0, 500) } else { (chosen, WEEKLY_AMOUNTS[chosen]) };
                let model = WEEKLY_MODELS[chosen].to_string();
                self.result[i] = Some(Placement::WeeklyGenerator { material, amount, model });
            }
            "random_monster" | "random_random_monster" => {
                // the link was resolved in pre-placement; ask again with the last pass
                let alignment = match self.link(&p.random_link, 1) {
                    Link::Alignment(a) => Some(a),
                    Link::Any | Link::Later => None,
                };
                let water = self.is_water(x, y, level);
                if let Some(stacks) = self.place_monster(p, alignment, water, rng) {
                    self.result[i] = Some(Placement::Monster { stacks });
                }
            }
            // rva `430cf0` (by level, the major type), `4314a0` (potion), `430f80` (tome),
            // `431210` (staff)
            "random_artifact" | "random_potion" | "random_staff" | "random_tome" => {
                let picked = match p.type_name.as_str() {
                    "random_potion" => self.artifacts.pick_potion(rng),
                    "random_tome" => self.artifacts.pick_from(&[200, 202, 199, 198, 201], rng),
                    "random_staff" => self.artifacts.pick_from(&[29, 182, 181, 185, 184], rng),
                    _ => match artifacts::LEVELS.iter().position(|l| *l == p.major) {
                        Some(level) => self.artifacts.pick_level(level, rng),
                        None => None,
                    },
                };
                if let Some(artifact) = picked {
                    match artifacts::model(artifact) {
                        Some(model) => {
                            self.result[i] = Some(Placement::Artifact { artifact, model: model.to_string() })
                        }
                        None => eprintln!("artifact {} has no map model", artifacts::KEYWORDS[artifact]),
                    }
                }
            }
            _ => {}
        }
    }

    /// `t_random_monster_base::place` (rva `3f38a0`): roll the army's strength in
    /// experience points, pick a creature that fits and give it about two thirds of the
    /// strength; a second stack of the same alignment and level (or one lower) takes the
    /// rest.
    fn place_monster(
        &self,
        p: &Placed,
        alignment: Option<usize>,
        water: bool,
        rng: &mut GlobalRandom,
    ) -> Option<Vec<(usize, i32)>> {
        // levels and default strength: `random_monster` by its major type `level_N`,
        // `random_random_monster` any level with the strength of level 3 (rva `3f3cb0`..)
        let (low_level, high_level, strength_level) = if p.type_name == "random_random_monster" {
            (0, 5, 3)
        } else {
            let l = level_of(&p.major) as i32;
            (l, l, l)
        };
        let (min, max) = match p.monster_range {
            // the map maker's range, counted in peasants
            Some((lo, hi)) if lo > 0 => {
                let peasant = self.creatures[PEASANT].experience;
                (lo * peasant, hi * peasant)
            }
            _ => default_strength(strength_level as usize)?,
        };
        let roll = rng.next() as i32;
        let factor = DIFFICULTY_STRENGTH.get(self.setup.difficulty).copied().unwrap_or(1.0);
        let strength = ((roll % (max - min + 1) + min) as f64 * factor) as i32;

        let map_expansion = self.map.header.expansion as i32;
        let first = self.pick_monster(alignment, water, strength, None, low_level, high_level, map_expansion, rng)?;
        let first_xp = self.creatures[first].experience;
        // Water creatures take the whole strength, others about two thirds, rounded up.
        let count = if first == SEA_MONSTER || first == MERMAID {
            strength / first_xp
        } else {
            (first_xp * 3 - 1 + strength * 2) / (first_xp * 3)
        }
        .max(1);
        let mut stacks = vec![(first, count)];

        let rest = strength - count * first_xp;
        if rest > 0 && first != SEA_MONSTER && first != MERMAID {
            // rva `3f3240`. Assumption: the alignment asked for here (a virtual call on the
            // first stack) is the first creature's own.
            let level = self.creatures[first].level;
            let second = self.pick_monster(
                Some(self.creatures[first].alignment),
                water,
                rest.min(first_xp),
                Some(first),
                level - 1,
                level,
                map_expansion,
                rng,
            );
            if let Some(second) = second {
                let n = rest / self.creatures[second].experience;
                if n > 0 {
                    // the game adds to an existing stack of the same creature
                    match stacks.iter_mut().find(|(c, _)| *c == second) {
                        Some(s) => s.1 += n,
                        None => stacks.push((second, n)),
                    }
                }
            }
        }
        Some(stacks)
    }

    /// Pick a creature (rva `3f3310`): every creature that fits gets an equal chance, by
    /// keeping the k-th fitting one with probability 1/k (one roll for each fitting one).
    /// Fits: from the map's expansion, worth at most `strength`, a water creature exactly
    /// when the spot is water, level in range, alignment if one is given. If `same` is
    /// given, creatures worth exactly `strength` must be that one.
    #[allow(clippy::too_many_arguments)]
    fn pick_monster(
        &self,
        alignment: Option<usize>,
        water: bool,
        strength: i32,
        same: Option<usize>,
        low_level: i32,
        high_level: i32,
        map_expansion: i32,
        rng: &mut GlobalRandom,
    ) -> Option<usize> {
        let mut seen = 0;
        let mut chosen = None;
        for (id, c) in self.creatures.iter().enumerate() {
            let fits = creatures::expansion(id) <= map_expansion
                && c.experience <= strength
                && same.is_none_or(|s| id == s || c.experience != strength)
                && matches!(id, MERMAID | PIRATE | SEA_MONSTER) == water
                && (low_level..=high_level).contains(&c.level)
                && alignment.is_none_or(|a| c.alignment == a);
            if fits {
                seen += 1;
                if rng.next() % seen == 0 {
                    chosen = Some(id);
                }
            }
        }
        chosen
    }

    /// Whether a monster at the stored position stands on water: the tile is water or a
    /// river (rva `5340`). The game then also counts a water tile as land if a land
    /// transition covers enough of it (more than 255 mask pixels); we don't measure masks
    /// and take any land transition as land.
    fn is_water(&self, x: i32, y: i32, level: i32) -> bool {
        const WATER: u8 = 0;
        const RIVER: u8 = 9;
        // stored x is the row, y the column
        let (Ok(row), Ok(col), Ok(level)) = (usize::try_from(x), usize::try_from(y), usize::try_from(level)) else {
            return false;
        };
        let Some(tile) = self.map.tile(level, row, col) else { return false };
        let watery = |t: u8| t == WATER || t == RIVER;
        watery(tile.terrain) && tile.terrain_transitions.iter().all(|t| watery(t.terrain))
    }
}

/// Default strength range of a random monster of `level` 1..=4 (rva `3f44c0`): a base of
/// 155, 310, 620, 1240 (VA `0x99004c`) times 2, 2.5, 3, 4 (VA `0x990058`), from 0.8 to 1.2
/// of that. The exe multiplies in x87 registers set to double precision (the MSVC default),
/// so `f64` gives the same numbers; the conversion to an integer truncates.
fn default_strength(level: usize) -> Option<(i32, i32)> {
    const BASE: [i32; 5] = [0, 155, 310, 620, 1240];
    const FACTOR: [f64; 5] = [0.0, 2.0, 2.5, 3.0, 4.0];
    if !(1..=4).contains(&level) {
        return None;
    }
    let base = BASE[level] as f64 * FACTOR[level];
    Some(((base * 0.8) as i32, (base * 1.2) as i32))
}

/// The number in a major type like `level_2` or `level_3_dwelling` (0 if there is none).
fn level_of(major: &str) -> u32 {
    major
        .strip_prefix("level_")
        .and_then(|rest| rest.split('_').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// The town type a real town's model shows: `castle.Asylum.Fort` is an asylum.
fn town_type_of_model(model: &str) -> Option<usize> {
    let faction = model.to_lowercase().strip_prefix("castle.")?.split('.').next()?.to_string();
    TOWN_TYPES.iter().position(|t| *t == faction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_strength_ranges() {
        // 310 × 0.8 and 310 × 1.2 must not come out as 247 / 371 from rounding
        assert_eq!(default_strength(1), Some((248, 372)));
        assert_eq!(default_strength(2), Some((620, 930)));
        assert_eq!(default_strength(3), Some((1488, 2232)));
        assert_eq!(default_strength(4), Some((3968, 5952)));
        assert_eq!(default_strength(0), None);
    }

    #[test]
    fn levels_from_major_types() {
        assert_eq!(level_of("level_2"), 2);
        assert_eq!(level_of("level_4_dwelling"), 4);
        assert_eq!(level_of("left"), 0);
    }

    #[test]
    fn town_types_from_models() {
        assert_eq!(town_type_of_model("castle.Asylum.Fort"), Some(3));
        assert_eq!(town_type_of_model("castle.Stronghold.fort r"), Some(5));
        assert_eq!(town_type_of_model("castle.Outpost"), None);
    }
}
