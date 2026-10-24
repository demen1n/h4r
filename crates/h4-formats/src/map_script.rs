//! Map scripts and events, as stored in `.h4c` maps.
//!
//! A script is a tree. Every node starts with its type as a *string* (`"seq"`, `"if"`,
//! `"give_creature"`, `"=="`, ...), the game looks the string up in a table and then the node
//! reads its own fields. There are three families of nodes, read by three factories:
//!
//! - actions (`t_script_action`, 54 types, names at rva `41bb00`), the factory is at rva `10790`;
//! - numeric expressions (21 types, names at rva `428010`), factory at rva `112d0`;
//! - boolean expressions (24 types, names at rva `4286c0`), factory at rva `115f0`.
//!
//! The field layout of each type was taken from its `read` method (slot 4 of the vtable of
//! `t_script_action<N>` for actions, slot 9 of `t_script_*_expression<N>` for expressions).
//! A script at the top of an event starts with a u16 version that is passed down to every
//! node; only `text` (and versions < 2 of player targets, not seen in our maps) look at it.
//!
//! We keep the tree generic ([`Script`]): type name, numbers, texts, child nodes. That's
//! enough to read every map to the end and to show or interpret scripts later, without a
//! Rust type per node.
//!
//! Events wrap scripts: a "discrete event" is a script plus a message and seven resource
//! amounts; timed, triggerable, continuous and placed events add a name and a few fields.

use crate::cp1251;
use crate::map_army::{read_army, read_artifact, Army};
use crate::reader::{bad, Reader};
use crate::Result;

/// Which factory a node came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFamily {
    Action,
    Numeric,
    Boolean,
}

/// One node of a script tree.
#[derive(Debug, Clone)]
pub struct Script {
    pub family: ScriptFamily,
    /// The type name as stored in the file, e.g. `"give_creature"`.
    pub kind: String,
    /// Plain number fields in file order (targets, ids, amounts...).
    pub numbers: Vec<i64>,
    /// Text fields in file order (messages, variable and event names).
    pub texts: Vec<String>,
    /// Child nodes in file order (sub-actions, operands, conditions).
    pub children: Vec<Script>,
    /// The army of a `combat` action.
    pub army: Option<Army>,
}

impl Script {
    fn new(family: ScriptFamily, kind: String) -> Script {
        Script { family, kind, numbers: Vec::new(), texts: Vec::new(), children: Vec::new(), army: None }
    }

    /// Call `f` on this node and every node below it.
    pub fn visit(&self, f: &mut dyn FnMut(&Script)) {
        f(self);
        for c in &self.children {
            c.visit(f);
        }
    }
}

/// A u16-length string of cp1251 text (all texts in maps are Russian in our version).
pub(crate) fn text(r: &mut Reader) -> Result<String> {
    let n = r.u16()? as usize;
    Ok(cp1251::decode(r.take(n)?))
}

/// A u16 that must be 0: most structures start with a version and only 0 exists.
pub(crate) fn zero_version(r: &mut Reader, what: &str) -> Result<()> {
    let at = r.pos;
    let v = r.u16()?;
    if v != 0 {
        return bad(format!("{what}: unknown version {v} at {at:#x}"));
    }
    Ok(())
}

/// A top-level script: u16 version, then one action (rva `10720`).
pub(crate) fn read_script(r: &mut Reader) -> Result<Script> {
    let version = r.u16()?;
    read_action(r, version)
}

/// One action node (factory at rva `10790`).
fn read_action(r: &mut Reader, version: u16) -> Result<Script> {
    let at = r.pos;
    let mut s = Script::new(ScriptFamily::Action, r.str16()?);
    // Small helpers so each case below reads like the field list of its type.
    fn u8(r: &mut Reader, s: &mut Script) -> Result<()> {
        s.numbers.push(r.u8()? as i64);
        Ok(())
    }
    fn u16(r: &mut Reader, s: &mut Script) -> Result<()> {
        s.numbers.push(r.u16()? as i64);
        Ok(())
    }
    match s.kind.as_str() {
        // player target only
        "build" | "change_owner" | "clear_lc_text" | "clear_l_msg" | "clear_vc_text" | "clear_v_msg"
        | "disable_vc" | "enable_vc" | "lose" | "win" => u8(r, &mut s)?,
        // army target, the enemy army, then what happens on win and on loss
        "combat" => {
            u8(r, &mut s)?;
            s.army = Some(read_army(r)?);
            s.children.push(read_action(r, version)?);
            s.children.push(read_action(r, version)?);
        }
        // condition, then the "then" and "else" actions
        "if" => {
            s.children.push(read_boolean(r)?);
            s.children.push(read_action(r, version)?);
            s.children.push(read_action(r, version)?);
        }
        // target, u16 amount
        "dec_mana" | "inc_mana" | "dec_pop" | "inc_pop" | "give_spell" => {
            u8(r, &mut s)?;
            u16(r, &mut s)?;
        }
        // hero attribute: target, u16 amount, u8 flag
        "dec_damage" | "dec_max_hp" | "dec_max_mana" | "dec_speed" | "inc_damage" | "inc_max_hp"
        | "inc_max_mana" | "inc_speed" => {
            u8(r, &mut s)?;
            u16(r, &mut s)?;
            u8(r, &mut s)?;
        }
        // target, u8 amount
        "dec_luck" | "dec_morale" | "inc_luck" | "inc_morale" | "inc_move" | "inc_level" => {
            u8(r, &mut s)?;
            u8(r, &mut s)?;
        }
        // name of the objects to delete / of the event to trigger
        "detonate" | "gosub" => s.texts.push(text(r)?),
        // message, then (since version 1) actions to run after it
        "text" => {
            s.texts.push(text(r)?);
            if version >= 1 {
                read_sequence(r, version, &mut s)?;
            }
        }
        // target, u16 count, artifacts
        "give_artifact" | "rem_artifact" => {
            u8(r, &mut s)?;
            for _ in 0..r.u16()? {
                s.numbers.push(read_artifact(r)? as i64);
            }
        }
        // target, u16 creature type, u16 count
        "give_creature" | "take_creature" => {
            u8(r, &mut s)?;
            u16(r, &mut s)?;
            u16(r, &mut s)?;
        }
        // player target, seven resource amounts
        "give_material" | "take_material" => {
            u8(r, &mut s)?;
            for _ in 0..7 {
                s.numbers.push(r.i32()? as i64);
            }
        }
        // hero target, skill, mastery level
        "give_skill" | "inc_skill" => {
            for _ in 0..3 {
                u8(r, &mut s)?;
            }
        }
        // hero target, u32 experience
        "inc_exp" => {
            u8(r, &mut s)?;
            s.numbers.push(r.u32()? as i64);
        }
        "no_op" | "rem_event" | "rem_this" => {}
        // question, then the "yes" and "no" actions
        "ask" => {
            s.texts.push(text(r)?);
            s.children.push(read_action(r, version)?);
            s.children.push(read_action(r, version)?);
        }
        "seq" => read_sequence(r, version, &mut s)?,
        // variable name, new value
        "set_bool" => {
            s.texts.push(text(r)?);
            s.children.push(read_boolean(r)?);
        }
        "set_num" => {
            s.texts.push(text(r)?);
            s.children.push(read_numeric(r)?);
        }
        // player target, message
        "set_lc_text" | "set_l_msg" | "set_vc_text" | "set_v_msg" => {
            u8(r, &mut s)?;
            s.texts.push(text(r)?);
        }
        other => return bad(format!("unknown script action {other:?} at {at:#x}")),
    }
    Ok(s)
}

/// `t_script_sequence::read` (rva `433c50`): u16 count, then that many actions.
fn read_sequence(r: &mut Reader, version: u16, s: &mut Script) -> Result<()> {
    for _ in 0..r.u16()? {
        s.children.push(read_action(r, version)?);
    }
    Ok(())
}

/// One numeric expression (factory at rva `112d0`).
fn read_numeric(r: &mut Reader) -> Result<Script> {
    let at = r.pos;
    let mut s = Script::new(ScriptFamily::Numeric, r.str16()?);
    match s.kind.as_str() {
        // the current date
        "day" | "dow" | "month" | "week" | "wom" => {}
        // arithmetic on two operands
        "/" | "-" | "+" | "%" | "*" => {
            s.children.push(read_numeric(r)?);
            s.children.push(read_numeric(r)?);
        }
        "neg" => s.children.push(read_numeric(r)?),
        // army target, u16 creature type
        "creatures" => {
            s.numbers.push(r.u8()? as i64);
            s.numbers.push(r.u16()? as i64);
        }
        // one target byte
        "exp_level" | "player" | "total_creatures" | "total_heroes" => s.numbers.push(r.u8()? as i64),
        // target and skill / material
        "mastery" | "materials" => {
            s.numbers.push(r.u8()? as i64);
            s.numbers.push(r.u8()? as i64);
        }
        "lit" => s.numbers.push(r.i32()? as i64),
        // lowest and highest value
        "rand" => {
            s.numbers.push(r.i32()? as i64);
            s.numbers.push(r.i32()? as i64);
        }
        "var" => s.texts.push(text(r)?),
        other => return bad(format!("unknown numeric expression {other:?} at {at:#x}")),
    }
    Ok(s)
}

/// One boolean expression (factory at rva `115f0`).
pub(crate) fn read_boolean(r: &mut Reader) -> Result<Script> {
    let at = r.pos;
    let mut s = Script::new(ScriptFamily::Boolean, r.str16()?);
    match s.kind.as_str() {
        "and" | "or" => {
            s.children.push(read_boolean(r)?);
            s.children.push(read_boolean(r)?);
        }
        "not" => s.children.push(read_boolean(r)?),
        // comparisons of two numbers
        "==" | ">" | ">=" | "<" | "<=" => {
            s.children.push(read_numeric(r)?);
            s.children.push(read_numeric(r)?);
        }
        "true" | "false" => {}
        // target and alignment / color
        "has_alignment" | "is_alignment" | "is_color" => {
            s.numbers.push(r.u8()? as i64);
            s.numbers.push(r.u8()? as i64);
        }
        // target and hero / town name
        "has_hero" | "owns_hero" | "owns_town" => {
            s.numbers.push(r.u8()? as i64);
            s.texts.push(text(r)?);
        }
        // hero target, skill, mastery
        "give_skill?" => {
            for _ in 0..3 {
                s.numbers.push(r.u8()? as i64);
            }
        }
        // u8 "is a player target", u8 target, the artifact
        "has_artifact" => {
            s.numbers.push(r.u8()? as i64);
            s.numbers.push(r.u8()? as i64);
            s.numbers.push(read_artifact(r)? as i64);
        }
        // hero name / variable name
        "is_dead" | "is_imprisoned" | "var" => s.texts.push(text(r)?),
        "is_computer" | "is_eliminated" | "is_human" => s.numbers.push(r.u8()? as i64),
        other => return bad(format!("unknown boolean expression {other:?} at {at:#x}")),
    }
    Ok(s)
}

/// A u16 version (here always 0) followed by a boolean expression (rva `411780`).
pub(crate) fn read_versioned_boolean(r: &mut Reader) -> Result<Script> {
    r.u16()?;
    read_boolean(r)
}

/// Any kind of event: a script with what the game shows and gives when it runs.
#[derive(Debug, Clone)]
pub struct Event {
    pub script: Script,
    /// Message shown when the event fires (empty for continuous events).
    pub message: String,
    /// Resources given: gold, wood, ore, crystal, sulfur, mercury, gems (order as in the
    /// game's material enum; not checked against the game yet).
    pub resources: [i32; 7],
    /// The event's name (scripts refer to events by name, e.g. `gosub`).
    pub name: String,
    /// Timed events: first day and repeat interval.
    pub first_day: u16,
    pub repeat: u16,
    /// Bit mask of the players (colors) the event applies to, where stored.
    pub players: u8,
    /// Two flag bits stored next to `players` (meaning not checked).
    pub flags: u8,
    /// Continuous owned events: run only during the owner's turn (name from the setter
    /// `set_run_only_during_owners_turn` in the debug map).
    pub owners_turn_only: bool,
}

impl Event {
    fn empty(script: Script) -> Event {
        Event {
            script,
            message: String::new(),
            resources: [0; 7],
            name: String::new(),
            first_day: 0,
            repeat: 0,
            players: 0,
            flags: 0,
            owners_turn_only: false,
        }
    }
}

/// `t_discrete_event` (rva `705b20`): script, message, seven resource amounts.
fn read_discrete(r: &mut Reader) -> Result<Event> {
    let mut e = Event::empty(read_script(r)?);
    e.message = text(r)?;
    for amount in e.resources.iter_mut() {
        *amount = r.i32()?;
    }
    Ok(e)
}

fn read_players_and_flags(r: &mut Reader, e: &mut Event) -> Result<()> {
    e.players = r.u8()?;
    e.flags = r.u8()?;
    Ok(())
}

/// The four lists of global events right after the map header.
#[derive(Debug, Clone, Default)]
pub struct GlobalEvents {
    pub timed: Vec<Event>,
    pub triggerable: Vec<Event>,
    pub continuous: Vec<Event>,
    pub placed: Vec<Event>,
}

/// `read_timed/triggerable/continuous/placed_events_from_map` of `t_adventure_map`
/// (rva `cfd80`, `cffe0`, `d0240`, `d04f0`): each is a u16 count and the events.
pub(crate) fn read_global_events(r: &mut Reader) -> Result<GlobalEvents> {
    let mut g = GlobalEvents::default();
    for _ in 0..r.u16()? {
        // rva 726a80: version 0, timed base (88c8c0), players, flags
        zero_version(r, "timed event")?;
        let mut e = read_timed_base(r)?;
        read_players_and_flags(r, &mut e)?;
        g.timed.push(e);
    }
    for _ in 0..r.u16()? {
        // rva 726d00 -> 8c8ce0: version 0, discrete event, name
        zero_version(r, "triggerable event")?;
        let mut e = read_discrete(r)?;
        e.name = text(r)?;
        g.triggerable.push(e);
    }
    for _ in 0..r.u16()? {
        // rva 726d70 -> 63b8b0: version 0, script, name
        zero_version(r, "continuous event")?;
        let mut e = Event::empty(read_script(r)?);
        e.name = text(r)?;
        g.continuous.push(e);
    }
    for _ in 0..r.u16()? {
        // rva 326f20: version 0 or 1, discrete event, (v1) players and flags, name
        let at = r.pos;
        let version = r.u16()?;
        if version > 1 {
            return bad(format!("placed event: unknown version {version} at {at:#x}"));
        }
        let mut e = read_discrete(r)?;
        if version >= 1 {
            read_players_and_flags(r, &mut e)?;
        }
        e.name = text(r)?;
        g.placed.push(e);
    }
    Ok(g)
}

/// `t_timed_event` base (rva `88c8c0`): discrete event, name, first day, repeat interval.
fn read_timed_base(r: &mut Reader) -> Result<Event> {
    let mut e = read_discrete(r)?;
    e.name = text(r)?;
    e.first_day = r.u16()?;
    e.repeat = r.u16()?;
    Ok(e)
}

/// Events attached to a town, hero, army or garrison. Same event kinds as the global ones,
/// each followed by the owner filter (rva `7d32d0`, `7d3590`, `7d3830`, `7d3af0`).
#[derive(Debug, Clone, Default)]
pub struct OwnedEvents {
    /// A fixed number of "built-in" events (4 for towns and garrisons, 3 for heroes and armies).
    pub built_in: Vec<Event>,
    pub timed: Vec<Event>,
    pub triggerable: Vec<Event>,
    pub continuous: Vec<Event>,
}

/// One built-in event: version 0, discrete event, players, flags.
pub(crate) fn read_built_in(r: &mut Reader) -> Result<Event> {
    zero_version(r, "built-in event")?;
    let mut e = read_discrete(r)?;
    read_players_and_flags(r, &mut e)?;
    Ok(e)
}

pub(crate) fn read_owned_timed(r: &mut Reader) -> Result<Vec<Event>> {
    let mut out = Vec::new();
    for _ in 0..r.u16()? {
        zero_version(r, "owned timed event")?;
        let mut e = read_timed_base(r)?;
        read_players_and_flags(r, &mut e)?;
        out.push(e);
    }
    Ok(out)
}

pub(crate) fn read_owned_triggerable(r: &mut Reader) -> Result<Vec<Event>> {
    let mut out = Vec::new();
    for _ in 0..r.u16()? {
        zero_version(r, "owned triggerable event")?;
        let mut e = read_discrete(r)?;
        e.name = text(r)?;
        read_players_and_flags(r, &mut e)?;
        out.push(e);
    }
    Ok(out)
}

pub(crate) fn read_owned_continuous(r: &mut Reader) -> Result<Vec<Event>> {
    let mut out = Vec::new();
    for _ in 0..r.u16()? {
        zero_version(r, "owned continuous event")?;
        let mut e = Event::empty(read_script(r)?);
        e.name = text(r)?;
        e.owners_turn_only = r.u8()? != 0;
        read_players_and_flags(r, &mut e)?;
        out.push(e);
    }
    Ok(out)
}

/// `built_in` events, then timed, triggerable and continuous lists: the order used by
/// towns, heroes and armies.
pub(crate) fn read_owned_events(r: &mut Reader, built_in: usize) -> Result<OwnedEvents> {
    let mut e = OwnedEvents::default();
    for _ in 0..built_in {
        e.built_in.push(read_built_in(r)?);
    }
    e.timed = read_owned_timed(r)?;
    e.triggerable = read_owned_triggerable(r)?;
    e.continuous = read_owned_continuous(r)?;
    Ok(e)
}
