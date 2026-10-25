//! Artifacts: the game's numbering, what the table says about each, the adventure map
//! model, and how a random artifact is picked.

use h4_formats::table::Table;

use crate::random::GlobalRandom;

/// Artifact levels in the game's order (keyword table around VA `0x97c4fc`). The major
/// types of `random_artifact` objects (`item`, `treasure`, `minor`, `major`) are these
/// levels: the game passes the type's id straight to the picker.
pub const LEVELS: [&str; 7] = ["item", "treasure", "minor", "major", "fragment", "relic", "divine"];

/// Slot names in the game's order (same keyword table); `potion` is 17.
pub const SLOTS: [&str; 20] = [
    "bow", "feet", "head", "left ring", "misc_1", "misc_2", "misc_3", "misc_4", "neck", "right ring",
    "left hand", "shoulders", "torso", "right hand", "invalid", "both hands", "backpack", "potion",
    "miscellaneous", "ring",
];
pub const POTION_SLOT: usize = 17;

/// Artifacts that are never picked at random (`+0x61` of the artifact traits, list at VA
/// `0x97c608`): campaign items like the chapters of a book, keys, crowns.
pub const DISABLED: [usize; 40] = [
    89, 48, 160, 164, 208, 35, 168, 98, 149, 54, 22, 38, 40, 39, 37, 18, 159, 145, 49, 92, 213, 67,
    122, 162, 63, 93, 109, 175, 20, 97, 148, 147, 157, 11, 84, 99, 187, 203, 214, 23,
];

/// The scroll and the parchment carry a spell; random pickers skip them when they refill.
const SPELL_CARRIERS: [usize; 2] = [124, 166];

/// Which game an artifact came with: 1 for ids 232..=247 (VA `0x97c6a8`, `+0x64`), else 0.
pub fn expansion(id: usize) -> u16 {
    if (232..248).contains(&id) {
        1
    } else {
        0
    }
}

/// Artifact keywords in the game's enum order: `KEYWORDS[id]`. From the exe's keyword
/// table at VA `0x97bdc8` (pairs of id + name); the same ids key the model tables below,
/// whose names agree (checked: only renames like "compass" / "sextant" differ).
pub const KEYWORDS: [&str; 248] = [
    "adamantine_armor", "adamantine_shield", "amulet_of_fear", "amulet_of_the_undertaker",
    "ankh_of_life", "apprentices_handbook", "archmages_spellbook", "armor_of_chaos",
    "armor_of_darkness", "armor_of_light", "armor_of_order", "arms_of_legion", "arrow_of_slaying",
    "arrow_of_stunning", "axe", "axe_of_legends", "badge_of_courage", "bag_of_gold", "binding",
    "binding_liquid", "blank_shield", "barbarian_throwing_club", "book_of_enchantment",
    "boots_of_levitation", "boots_of_speed", "bow_of_the_white_stag", "brazier_of_sulfur",
    "breastplate_of_regeneration", "brimstone_breastplate", "caduceus", "cap_of_knowledge",
    "cape_of_protection", "cart_of_lumber", "cart_of_ore", "centaurs_spear", "chain", "chainmail",
    "chapter_four", "chapter_one", "chapter_three", "chapter_two", "circlet_of_wisdom",
    "cloak_of_confusion", "cloud_of_despair", "compass", "cowl_of_resistance", "crest_of_valor",
    "crossbow", "crown", "crown_of_dragon_teeth", "crown_of_the_mind", "crystal_of_light",
    "crystal_of_memory", "cube_of_crystals", "dark_ruby", "davids_sling", "demon_slayer",
    "demonary", "dragon_scale_armor", "dragon_scale_shield", "druids_chain", "dwarven_hammer",
    "dwarven_shield", "ebony_key", "elven_chainmail", "emerald_longbow", "equestrians_gloves",
    "fire_snake", "fireproof_boots", "fizbin_of_misfortune", "flaming_arrow", "flaming_sword",
    "flask_of_mercury", "four_leaf_clover", "gamblers_deck", "giant_slayer", "gias_gems",
    "golden_plate_mail", "greater_ring_of_vulnerability", "greatsword", "gryphonhearts_plate_mail",
    "guildmasters_compendium", "halberd_of_speed", "hawkins_bow_of_speed", "head_of_legion",
    "helm_of_command", "helm_of_power", "helm_of_vision", "hideous_mask", "holy_crown",
    "holy_water", "horseshoe", "infant_dragon_wings", "ivory_key", "journeymans_notebook",
    "kreegan_fire", "leather_armor", "left_elephant_tusk", "left_onyx", "legs_of_legion",
    "leprechauns_ring", "lesser_ring_of_vulnerability", "lions_shield_of_courage",
    "logbook_of_the_master_sailor", "longbow", "longsword", "mages_robe", "mages_staff",
    "magic_amplifier", "mahogany_key", "mantle_of_spell_turning", "marantheas_mug",
    "masters_journal", "medal_of_honor", "mind_shield", "minotaurs_battleax",
    "mirror_of_spell_turning", "monks_mace", "mullichs_helm_of_leadership", "necklace_of_charm",
    "neeners_invulnerable_cloak", "ogs_sandals", "orb_of_summoning", "breeze_the_falcon",
    "parchment", "nomad_blackbow", "plate_mail", "poison_arrow", "poison_ring", "potion_of_cold",
    "potion_of_endurance", "potion_of_fire_resistance", "potion_of_healing", "potion_of_health",
    "potion_of_luck", "potion_of_mana", "potion_of_mirth", "potion_of_precognition",
    "potion_of_quickness", "potion_of_resistance", "potion_of_restoration", "potion_of_speed",
    "potion_of_strength", "purse_of_gold", "purse_of_pennypinching", "rams_horn", "deadwood_staff",
    "rhino_horn", "right_elephant_tusk", "right_onyx", "ring_of_elementals", "ring_of_health",
    "ring_of_permanency", "ring_of_protection", "ring_of_regeneration", "ring_of_speed",
    "ring_of_strength", "rising_sun", "robe_of_the_guardian", "rod_of_chaos", "ruby",
    "sack_of_gold", "sandalwood_key", "sandwalker_sandals", "sapphire", "scale_mail_of_strength",
    "scroll", "seamans_hat", "setting", "shackles_of_war", "shield", "shield_of_chaos",
    "shield_of_darkness", "shield_of_light", "shield_of_order", "shield_of_resistance",
    "snipers_crossbow", "snowshoes", "soul_stealer", "spiders_silk_arrow",
    "staads_scarab_of_summoning", "staff_of_death", "staff_of_enchantment", "staff_of_power",
    "staff_of_summoning", "staff_of_the_witch_king", "statesmans_medal", "statue_of_legion",
    "steadfast_shield", "supreme_crown_of_the_magi", "surefooted_boots", "swamp_boots",
    "sword_of_swiftness", "sword_of_the_gods", "tavins_sling", "telescope", "throwing_spear",
    "thunder_hammer", "tome_of_chaos", "tome_of_death", "tome_of_life", "tome_of_nature",
    "tome_of_order", "torso_of_legion", "tynans_dagger_of_despair", "unnatural_armor",
    "unnatural_shield", "valders_bow_of_sloth", "vampiric_amulet", "vial_of_acid",
    "vial_of_blinding_smoke", "vial_of_choking_gas", "vial_of_poison", "vial_of_sulfur",
    "victory_banner", "wand_of_animate_dead", "wand_of_bless", "wand_of_curse", "wand_of_fire",
    "wand_of_fireball", "wand_of_haste", "wand_of_healing", "wand_of_ice", "wand_of_illusion",
    "wand_of_weakness", "warding_robe", "warlords_ring", "winged_sandals", "wizards_ring",
    "true_gryphonheart_blade", "false_gryphonheart_blade", "grail", "cloak_of_darkness",
    "ring_of_light", "tiger_armor", "tiger_helm", "frost_hammer", "harmonic_chainmail",
    "necklace_of_muses", "aiffes_mandolin", "necklace_of_balance", "flame_of_chaos", "ice_scales",
    "archmages_hat", "staff_of_disruption", "wayfaring_boots", "ring_of_flares",
    "angelfeather_cloak",
];

/// Adventure map models by id (`adv_object.<model>`), built like the exe does (rva
/// `2f340`): a folder prefix plus a name from tables at VA `0x972278`..`0x972580`,
/// `0x972588` .. `0xa4b1d8` (prefixes at `0xa4c0f0`..). The Winds of War artifacts (232..=247) have no entry in this
/// exe; their models are in `updates.h4r`, matched here by name. `None`: no model (mostly
/// campaign artifacts, which are never random).
const MODELS: [Option<&str>; 248] = [
    Some("artifacts.armor.adamantine armor"), // 0 adamantine_armor
    Some("artifacts.shields.adamantine shield"), // 1 adamantine_shield
    Some("artifacts.amulets.amulet of fear"), // 2 amulet_of_fear
    Some("artifacts.amulets.amulet of the undertaker"), // 3 amulet_of_the_undertaker
    Some("artifacts.amulets.anhk of life"), // 4 ankh_of_life
    Some("artifacts.tomes-books-manuals.apprentices handbook"), // 5 apprentices_handbook
    Some("artifacts.tomes-books-manuals.archmages codex"), // 6 archmages_spellbook
    Some("artifacts.armor.armor of chaos"), // 7 armor_of_chaos
    Some("artifacts.armor.armor of death"), // 8 armor_of_darkness
    Some("artifacts.armor.armor of light"), // 9 armor_of_light
    Some("artifacts.armor.armor of order"), // 10 armor_of_order
    Some("artifacts.miscellaneous.arms of legion"), // 11 arms_of_legion
    Some("artifacts.miscellaneous.arrow of slaying"), // 12 arrow_of_slaying
    Some("artifacts.miscellaneous.arrow of stunning"), // 13 arrow_of_stunning
    Some("artifacts.weapons.axe"), // 14 axe
    Some("artifacts.weapons.axe of legends"), // 15 axe_of_legends
    Some("artifacts.miscellaneous.badge of courage"), // 16 badge_of_courage
    Some("artifacts.miscellaneous.bag of gold"), // 17 bag_of_gold
    None, // 18 binding
    Some("artifacts.potions.vial of binding liquid"), // 19 binding_liquid
    None, // 20 blank_shield
    Some("artifacts.weapons.barbarian throwing club"), // 21 barbarian_throwing_club
    None, // 22 book_of_enchantment
    Some("artifacts.boots.boots of levitation"), // 23 boots_of_levitation
    Some("artifacts.boots.boots of travel"), // 24 boots_of_speed
    None, // 25 bow_of_the_white_stag
    Some("artifacts.miscellaneous.brazier of sulfur"), // 26 brazier_of_sulfur
    Some("artifacts.armor.breastplate of regeneration"), // 27 breastplate_of_regeneration
    Some("artifacts.armor.brimstone breastplate"), // 28 brimstone_breastplate
    Some("artifacts.staves.caduceus"), // 29 caduceus
    Some("artifacts.helms.cap of knowledge"), // 30 cap_of_knowledge
    Some("artifacts.cloaks.cape of protection"), // 31 cape_of_protection
    Some("artifacts.miscellaneous.cart of lumber"), // 32 cart_of_lumber
    Some("artifacts.miscellaneous.cart of ore"), // 33 cart_of_ore
    Some("artifacts.weapons.centaur's spear"), // 34 centaurs_spear
    None, // 35 chain
    Some("artifacts.armor.chain mail"), // 36 chainmail
    None, // 37 chapter_four
    None, // 38 chapter_one
    None, // 39 chapter_three
    None, // 40 chapter_two
    Some("artifacts.helms.circlet of wisdom"), // 41 circlet_of_wisdom
    Some("artifacts.cloaks.cloak of distraction"), // 42 cloak_of_confusion
    Some("artifacts.potions.vial of cloud of despair"), // 43 cloud_of_despair
    Some("artifacts.miscellaneous.sextant"), // 44 compass
    Some("artifacts.cloaks.cowl of resistance"), // 45 cowl_of_resistance
    Some("artifacts.miscellaneous.crest of valor"), // 46 crest_of_valor
    Some("artifacts.bows.crossbow"), // 47 crossbow
    None, // 48 crown
    None, // 49 crown_of_dragon_teeth
    Some("artifacts.helms.crown of the mind"), // 50 crown_of_the_mind
    Some("artifacts.miscellaneous.viewing crystal"), // 51 crystal_of_light
    Some("artifacts.miscellaneous.crystal of memory"), // 52 crystal_of_memory
    Some("artifacts.miscellaneous.crystal_figurine"), // 53 cube_of_crystals
    None, // 54 dark_ruby
    Some("artifacts.bows.davids sling"), // 55 davids_sling
    Some("artifacts.weapons.demon_slayer"), // 56 demon_slayer
    Some("artifacts.tomes-books-manuals.demonary"), // 57 demonary
    Some("artifacts.armor.dragonscale armor"), // 58 dragon_scale_armor
    Some("artifacts.shields.dragonScale shield"), // 59 dragon_scale_shield
    Some("artifacts.amulets.Druids chain"), // 60 druids_chain
    Some("artifacts.weapons.dwarven hammer"), // 61 dwarven_hammer
    Some("artifacts.shields.dwarven shield"), // 62 dwarven_shield
    None, // 63 ebony_key
    Some("artifacts.armor.elven chainmail"), // 64 elven_chainmail
    Some("artifacts.bows.emerald_longbow"), // 65 emerald_longbow
    Some("artifacts.item.Equestrian gloves"), // 66 equestrians_gloves
    None, // 67 fire_snake
    Some("artifacts.boots.FireProofBoots"), // 68 fireproof_boots
    Some("artifacts.amulets.collar of misfortune"), // 69 fizbin_of_misfortune
    Some("artifacts.miscellaneous.flaming arrow"), // 70 flaming_arrow
    Some("artifacts.weapons.flaming sword"), // 71 flaming_sword
    Some("artifacts.miscellaneous.flask of mercury"), // 72 flask_of_mercury
    Some("artifacts.miscellaneous.four leaf clover"), // 73 four_leaf_clover
    Some("artifacts.miscellaneous.gamblers deck"), // 74 gamblers_deck
    Some("artifacts.weapons.giant_slayer"), // 75 giant_slayer
    Some("artifacts.miscellaneous.gem casket"), // 76 gias_gems
    Some("artifacts.armor.golden platemail"), // 77 golden_plate_mail
    Some("artifacts.rings.ring_greater_negation"), // 78 greater_ring_of_vulnerability
    Some("artifacts.weapons.greatsword"), // 79 greatsword
    Some("artifacts.armor.gryphonhearts platemail"), // 80 gryphonhearts_plate_mail
    Some("artifacts.tomes-books-manuals.guildmasters compendium"), // 81 guildmasters_compendium
    Some("artifacts.weapons.halberd swiftwatch"), // 82 halberd_of_speed
    Some("artifacts.bows.bow of the elf king"), // 83 hawkins_bow_of_speed
    Some("artifacts.miscellaneous.head of legion"), // 84 head_of_legion
    Some("artifacts.helms.helm of command"), // 85 helm_of_command
    Some("artifacts.helms.helm of power"), // 86 helm_of_power
    Some("artifacts.helms.helm of vision"), // 87 helm_of_vision
    Some("artifacts.helms.hideous mask"), // 88 hideous_mask
    None, // 89 holy_crown
    Some("artifacts.potions.holy water"), // 90 holy_water
    Some("artifacts.miscellaneous.horseshoe"), // 91 horseshoe
    None, // 92 infant_dragon_wings
    None, // 93 ivory_key
    Some("artifacts.tomes-books-manuals.journeymens notebook"), // 94 journeymans_notebook
    Some("artifacts.potions.vial of kreegan fire"), // 95 kreegan_fire
    Some("artifacts.armor.leather"), // 96 leather_armor
    None, // 97 left_elephant_tusk
    None, // 98 left_onyx
    Some("artifacts.miscellaneous.legs of legion"), // 99 legs_of_legion
    Some("artifacts.rings.leprechauns ring"), // 100 leprechauns_ring
    Some("artifacts.rings.lesser ring of vulnerability"), // 101 lesser_ring_of_vulnerability
    Some("artifacts.shields.lions shield of courage"), // 102 lions_shield_of_courage
    Some("artifacts.tomes-books-manuals.logbook of the master sailor"), // 103 logbook_of_the_master_sailor
    Some("artifacts.bows.longbow"), // 104 longbow
    Some("artifacts.weapons.longsword"), // 105 longsword
    Some("artifacts.cloaks.mages robes"), // 106 mages_robe
    Some("artifacts.staves.mage staff"), // 107 mages_staff
    Some("artifacts.helms.magic amplifier"), // 108 magic_amplifier
    None, // 109 mahogany_key
    Some("artifacts.cloaks.mantle of spell turning"), // 110 mantle_of_spell_turning
    Some("artifacts.miscellaneous.marantheas mug of mirth"), // 111 marantheas_mug
    Some("artifacts.tomes-books-manuals.master's spellbook"), // 112 masters_journal
    Some("artifacts.amulets.medal of honor"), // 113 medal_of_honor
    Some("artifacts.helms.mind shield"), // 114 mind_shield
    Some("artifacts.weapons.bull rune axe"), // 115 minotaurs_battleax
    Some("artifacts.amulets.mirror revenge"), // 116 mirror_of_spell_turning
    Some("artifacts.weapons.crusaders mace"), // 117 monks_mace
    Some("artifacts.helms.leadership"), // 118 mullichs_helm_of_leadership
    Some("artifacts.amulets.neckless of charm"), // 119 necklace_of_charm
    Some("artifacts.cloaks.neeners"), // 120 neeners_invulnerable_cloak
    Some("artifacts.boots.Surefooted_boots"), // 121 ogs_sandals
    None, // 122 orb_of_summoning
    Some("artifacts.miscellaneous.breeze_falcon"), // 123 breeze_the_falcon
    Some("artifacts.tomes-books-manuals.parchment"), // 124 parchment
    None, // 125 nomad_blackbow
    Some("artifacts.armor.plate mail"), // 126 plate_mail
    Some("artifacts.miscellaneous.poison arrow"), // 127 poison_arrow
    Some("artifacts.rings.poison ring"), // 128 poison_ring
    Some("artifacts.potions.potion of cold"), // 129 potion_of_cold
    Some("artifacts.potions.potion of endurance"), // 130 potion_of_endurance
    Some("artifacts.potions.potion of fire resistance"), // 131 potion_of_fire_resistance
    Some("artifacts.potions.potion of healing"), // 132 potion_of_healing
    Some("artifacts.potions.potion of health"), // 133 potion_of_health
    Some("artifacts.potions.potion of luck"), // 134 potion_of_luck
    Some("artifacts.potions.potion of mana"), // 135 potion_of_mana
    Some("artifacts.potions.potion of mirth"), // 136 potion_of_mirth
    Some("artifacts.potions.potion of precognition"), // 137 potion_of_precognition
    Some("artifacts.potions.potion of quickness"), // 138 potion_of_quickness
    Some("artifacts.potions.potion of resistance"), // 139 potion_of_resistance
    Some("artifacts.potions.potion of restoration"), // 140 potion_of_restoration
    Some("artifacts.potions.potion of speed"), // 141 potion_of_speed
    Some("artifacts.potions.potion of strength"), // 142 potion_of_strength
    Some("artifacts.miscellaneous.purse of gold"), // 143 purse_of_gold
    Some("artifacts.miscellaneous.purse of PennyPinching"), // 144 purse_of_pennypinching
    None, // 145 rams_horn
    Some("artifacts.staves.deadwood_staff"), // 146 deadwood_staff
    None, // 147 rhino_horn
    None, // 148 right_elephant_tusk
    None, // 149 right_onyx
    Some("artifacts.rings.ring of the elements"), // 150 ring_of_elementals
    Some("artifacts.rings.ring of health"), // 151 ring_of_health
    Some("artifacts.rings.ring of permanency"), // 152 ring_of_permanency
    Some("artifacts.rings.ring of protection"), // 153 ring_of_protection
    Some("artifacts.rings.ring of regeneration"), // 154 ring_of_regeneration
    Some("artifacts.rings.ring of speed"), // 155 ring_of_speed
    Some("artifacts.rings.ring of strength"), // 156 ring_of_strength
    None, // 157 rising_sun
    Some("artifacts.cloaks.robe of the guardian"), // 158 robe_of_the_guardian
    None, // 159 rod_of_chaos
    None, // 160 ruby
    Some("artifacts.miscellaneous.sack of gold"), // 161 sack_of_gold
    None, // 162 sandalwood_key
    Some("artifacts.boots.Sandwalker Sandals"), // 163 sandwalker_sandals
    None, // 164 sapphire
    Some("artifacts.armor.scalemail of strength"), // 165 scale_mail_of_strength
    Some("artifacts.tomes-books-manuals.scroll"), // 166 scroll
    Some("artifacts.helms.helm_seamans hat"), // 167 seamans_hat
    None, // 168 setting
    Some("artifacts.miscellaneous.shackles of war"), // 169 shackles_of_war
    Some("artifacts.shields.shield"), // 170 shield
    Some("artifacts.shields.shield of chaos"), // 171 shield_of_chaos
    Some("artifacts.shields.death shield"), // 172 shield_of_darkness
    Some("artifacts.shields.life shield"), // 173 shield_of_light
    Some("artifacts.shields.shield of order"), // 174 shield_of_order
    None, // 175 shield_of_resistance
    Some("artifacts.bows.sniper's crossbow"), // 176 snipers_crossbow
    Some("artifacts.boots.Snow Shoes"), // 177 snowshoes
    Some("artifacts.weapons.soul stealer"), // 178 soul_stealer
    Some("artifacts.miscellaneous.spiders silk"), // 179 spiders_silk_arrow
    Some("artifacts.amulets.scarab summoning"), // 180 staads_scarab_of_summoning
    Some("artifacts.staves.staff of death"), // 181 staff_of_death
    Some("artifacts.staves.staff of enchantment"), // 182 staff_of_enchantment
    Some("artifacts.staves.staff of power"), // 183 staff_of_power
    Some("artifacts.staves.staff of summoning"), // 184 staff_of_summoning
    Some("artifacts.staves.staff of the WitchKing"), // 185 staff_of_the_witch_king
    Some("artifacts.amulets.statesmans medal"), // 186 statesmans_medal
    Some("artifacts.miscellaneous.statue of legion"), // 187 statue_of_legion
    Some("artifacts.shields.steadfast shield"), // 188 steadfast_shield
    Some("artifacts.helms.helm_supreme crown of the magi"), // 189 supreme_crown_of_the_magi
    Some("artifacts.boots.boots of the explorer"), // 190 surefooted_boots
    Some("artifacts.boots.crocodile"), // 191 swamp_boots
    Some("artifacts.weapons.sword of swiftness"), // 192 sword_of_swiftness
    Some("artifacts.weapons.sword of the gods"), // 193 sword_of_the_gods
    Some("artifacts.bows.war sling"), // 194 tavins_sling
    Some("artifacts.item.telescope"), // 195 telescope
    Some("artifacts.weapons.throwing spear"), // 196 throwing_spear
    Some("artifacts.weapons.thunder_hammer"), // 197 thunder_hammer
    Some("artifacts.tomes-books-manuals.tome of chaos"), // 198 tome_of_chaos
    Some("artifacts.tomes-books-manuals.tome of death"), // 199 tome_of_death
    Some("artifacts.tomes-books-manuals.tome of life"), // 200 tome_of_life
    Some("artifacts.tomes-books-manuals.tome of nature"), // 201 tome_of_nature
    Some("artifacts.tomes-books-manuals.tome of order"), // 202 tome_of_order
    Some("artifacts.miscellaneous.torso of legion"), // 203 torso_of_legion
    Some("artifacts.weapons.tynans_dagger"), // 204 tynans_dagger_of_despair
    Some("artifacts.armor.unnatural armor"), // 205 unnatural_armor
    Some("artifacts.shields.unnatural shield"), // 206 unnatural_shield
    Some("artifacts.bows.valders bow of sloth"), // 207 valders_bow_of_sloth
    None, // 208 vampiric_amulet
    Some("artifacts.potions.vial of acid"), // 209 vial_of_acid
    Some("artifacts.potions.vial of blinding smoke"), // 210 vial_of_blinding_smoke
    Some("artifacts.potions.vial of choking gas"), // 211 vial_of_choking_gas
    Some("artifacts.potions.vial of poison"), // 212 vial_of_poison
    None, // 213 vial_of_sulfur
    Some("artifacts.miscellaneous.victory banner"), // 214 victory_banner
    Some("artifacts.wands.wand of animatedead"), // 215 wand_of_animate_dead
    Some("artifacts.wands.wand of bless"), // 216 wand_of_bless
    Some("artifacts.wands.wand of curse"), // 217 wand_of_curse
    Some("artifacts.wands.wand of fire"), // 218 wand_of_fire
    Some("artifacts.wands.wand of fireball"), // 219 wand_of_fireball
    Some("artifacts.wands.wand of haste"), // 220 wand_of_haste
    Some("artifacts.wands.wand of healing"), // 221 wand_of_healing
    Some("artifacts.wands.wand of ice"), // 222 wand_of_ice
    Some("artifacts.wands.wand of illusion"), // 223 wand_of_illusion
    Some("artifacts.wands.wand of weakness"), // 224 wand_of_weakness
    Some("artifacts.cloaks.warding robe"), // 225 warding_robe
    Some("artifacts.rings.warlords ring"), // 226 warlords_ring
    Some("artifacts.boots.Winged Sandals"), // 227 winged_sandals
    Some("artifacts.rings.wizards ring"), // 228 wizards_ring
    Some("artifacts.weapons.gryphonhearts_true_blade"), // 229 true_gryphonheart_blade
    Some("artifacts.weapons.gryphonhearts_false_blade"), // 230 false_gryphonheart_blade
    Some("artifacts.miscellaneous.grail"), // 231 grail
    Some("artifacts.cloaks.Darkness"), // 232 cloak_of_darkness
    Some("artifacts.Rings.Light"), // 233 ring_of_light
    Some("artifacts.armor.Tiger_Breastplate"), // 234 tiger_armor
    Some("artifacts.Helms.Tiger"), // 235 tiger_helm
    Some("artifacts.weapons.Frost_Hammer"), // 236 frost_hammer
    Some("artifacts.armor.Harmonic_Chainmail"), // 237 harmonic_chainmail
    Some("artifacts.amulets.Necklace_of_Muses"), // 238 necklace_of_muses
    Some("artifacts.miscellaneous.Aiffe's_Mandolin"), // 239 aiffes_mandolin
    Some("artifacts.amulets.Necklace_of_Balance"), // 240 necklace_of_balance
    Some("artifacts.miscellaneous.Flame_of_Chaos"), // 241 flame_of_chaos
    Some("artifacts.miscellaneous.Ice_Scales"), // 242 ice_scales
    Some("artifacts.miscellaneous.Archmage's_Hat"), // 243 archmages_hat
    Some("artifacts.staves.Disruption"), // 244 staff_of_disruption
    Some("artifacts.boots.Wayfaring"), // 245 wayfaring_boots
    Some("artifacts.Rings.Flares"), // 246 ring_of_flares
    Some("artifacts.cloaks.Angel_Feather"), // 247 angelfeather_cloak
];

/// The `adv_object` model of an artifact lying on the map.
pub fn model(id: usize) -> Option<&'static str> {
    MODELS.get(id).copied().flatten()
}

/// What `table.Artifacts` says about one artifact.
#[derive(Debug, Clone, PartialEq)]
pub struct ArtifactInfo {
    /// Index into [`LEVELS`].
    pub level: usize,
    /// Index into [`SLOTS`].
    pub slot: usize,
    /// "Allowed By Default": allowed on maps that don't list their own.
    pub allowed_by_default: bool,
}

/// The table's rows in enum order (`result[id]`). Rows are matched to ids by keyword, as
/// the game does (rva `14ffb0`). Fails on a missing keyword or an unknown level or slot.
pub fn from_table(table: &Table) -> Result<Vec<ArtifactInfo>, String> {
    let col = |title: &str| table.column(title).ok_or(format!("table.Artifacts has no column {title:?}"));
    let (keyword, slot, level, allowed) = (col("Keyword")?, col("Slot")?, col("Level")?, col("Allowed By Default")?);
    let rows = table.data_rows();
    KEYWORDS
        .iter()
        .map(|k| {
            let row = rows
                .iter()
                .find(|r| r.get(keyword).is_some_and(|c| c.trim().eq_ignore_ascii_case(k)))
                .ok_or(format!("table.Artifacts has no {k:?}"))?;
            let text = |c: usize| row.get(c).map(|s| s.trim().to_lowercase()).unwrap_or_default();
            let level_name = text(level);
            let slot_name = text(slot);
            Ok(ArtifactInfo {
                level: LEVELS.iter().position(|l| *l == level_name).ok_or(format!("{k}: level {level_name:?}"))?,
                slot: SLOTS.iter().position(|s| *s == slot_name).ok_or(format!("{k}: slot {slot_name:?}"))?,
                allowed_by_default: !text(allowed).is_empty(),
            })
        })
        .collect()
}

/// The map's artifact bookkeeping for random picks: which artifacts are allowed and which
/// are "used" (already handed out). Picks avoid used ones until a kind runs out.
pub struct ArtifactPool<'a> {
    info: &'a [ArtifactInfo],
    allowed: Vec<bool>,
    used: Vec<bool>,
    map_expansion: u16,
}

impl<'a> ArtifactPool<'a> {
    /// `allowed` is the map's own list (shorter lists leave the rest out) or `None` for the
    /// table's defaults. Assumption: the game starts with every not-allowed artifact marked
    /// used, since its first count only looks at "used"; the start of that set isn't traced.
    pub fn new(info: &'a [ArtifactInfo], allowed: Option<&[bool]>, map_expansion: u16) -> ArtifactPool<'a> {
        let allowed: Vec<bool> = (0..info.len())
            .map(|i| match allowed {
                Some(a) => a.get(i).copied().unwrap_or(false),
                None => info[i].allowed_by_default,
            })
            .collect();
        let mut used: Vec<bool> = allowed.iter().map(|a| !a).collect();
        for c in SPELL_CARRIERS {
            if let Some(u) = used.get_mut(c) {
                *u = true;
            }
        }
        ArtifactPool { info, allowed, used, map_expansion }
    }

    /// A random artifact of a level (rva `4d9920`), for `random_artifact`.
    pub fn pick_level(&mut self, level: usize, rng: &mut GlobalRandom) -> Option<usize> {
        let candidates: Vec<usize> =
            (0..self.info.len()).filter(|&i| !DISABLED.contains(&i) && self.info[i].level == level).collect();
        self.pick(&candidates, rng)
    }

    /// A random potion of any level (rva `4d9f20`), for `random_potion`.
    pub fn pick_potion(&mut self, rng: &mut GlobalRandom) -> Option<usize> {
        let candidates: Vec<usize> =
            (0..self.info.len()).filter(|&i| !DISABLED.contains(&i) && self.info[i].slot == POTION_SLOT).collect();
        self.pick(&candidates, rng)
    }

    /// One of a fixed list (rva `4d9c40`): the tomes `[200, 202, 199, 198, 201]` (VA
    /// `0x972738`) for `random_tome`, the staffs `[29, 182, 181, 185, 184]` (`0x97274c`)
    /// for `random_staff`.
    pub fn pick_from(&mut self, list: &[usize], rng: &mut GlobalRandom) -> Option<usize> {
        self.pick(list, rng)
    }

    /// The common part of the three pickers: count the unused candidates the map's
    /// expansion has; if none, mark the allowed ones unused again (not the scroll and
    /// parchment); if still none, every one. Then one roll picks among the unused and
    /// marks it used.
    fn pick(&mut self, candidates: &[usize], rng: &mut GlobalRandom) -> Option<usize> {
        let fits = |p: &Self, c: usize| expansion(c) <= p.map_expansion;
        let free = |p: &Self| candidates.iter().filter(|&&c| !p.used[c] && fits(p, c)).count();
        if free(self) == 0 {
            for &c in candidates {
                if !SPELL_CARRIERS.contains(&c) && self.allowed[c] && fits(self, c) {
                    self.used[c] = false;
                }
            }
        }
        if free(self) == 0 {
            for &c in candidates {
                if !SPELL_CARRIERS.contains(&c) && fits(self, c) {
                    self.used[c] = false;
                }
            }
        }
        let n = free(self);
        if n == 0 {
            return None;
        }
        let mut k = rng.next() as usize % n;
        for &c in candidates {
            if !self.used[c] && fits(self, c) {
                if k == 0 {
                    self.used[c] = true;
                    return Some(c);
                }
                k -= 1;
            }
        }
        None
    }
}
