//! Per-decision state digest, the same JSON shape as `tools/r7_export.py` `digest()`
//! (DIGEST_VERSION 1). Diagnostic and parity only, never on the hot path.

use crate::card::{CId, KRef};
use crate::combat::{Combat, PLAYER};
use crate::creature::Side;
use std::fmt::Write;

pub const DIGEST_VERSION: u32 = 1;

impl Combat {
    fn digest_powers(&self, c: CId, out: &mut String) {
        out.push('[');
        for (i, p) in self.cr(c).power_list().iter().enumerate() {
            let p = &self.powers[*p as usize];
            if i > 0 {
                out.push(',');
            }
            write!(out, "[\"{}\",{}]", p.id.name(), p.amount).unwrap();
        }
        out.push(']');
    }

    fn digest_pile(&self, pile: &[KRef], out: &mut String) {
        out.push('[');
        for (i, &k) in pile.iter().enumerate() {
            let c = &self.cards[k as usize];
            if i > 0 {
                out.push(',');
            }
            write!(out, "[\"{}\",{},{},{}]", c.id.name(), c.upgraded, c.cost, c.instance_id).unwrap();
        }
        out.push(']');
    }

    /// JSON digest of the combat after a decision.
    pub fn digest_json(&self) -> String {
        let mut o = String::with_capacity(2048);
        let side = match self.side { Side::Player => "PLAYER", Side::Enemy => "ENEMY" };
        write!(o, "{{\"v\":{DIGEST_VERSION},\"round\":{},\"side\":\"{side}\",\"turns\":{},\"over\":{},\"won\":{},",
            self.round, self.turn_count, self.is_over, self.won).unwrap();
        let p = self.player();
        write!(o, "\"player\":[{},{},{},{},{},", p.hp, p.max_hp, p.block, self.players[PLAYER].energy, self.players[PLAYER].stars).unwrap();
        self.digest_powers(PLAYER, &mut o);
        o.push(']');
        if self.players[PLAYER].orbs.capacity > 0 {
            // `tools/r7_export.py`: "orbs": [capacity, [[kind, val], ...]] once a slot exists.
            write!(o, ",\"orbs\":[{},[", self.players[PLAYER].orbs.capacity).unwrap();
            for (i, orb) in self.players[PLAYER].orbs.orbs.iter().enumerate() {
                if i > 0 {
                    o.push(',');
                }
                write!(o, "[\"{}\",{}]", orb.kind.name(), orb.val).unwrap();
            }
            o.push_str("]]");
        }
        o.push_str(",\"hand\":");
        self.digest_pile(&self.players[PLAYER].hand, &mut o);
        o.push_str(",\"draw\":");
        self.digest_pile(&self.players[PLAYER].draw, &mut o);
        o.push_str(",\"discard\":");
        self.digest_pile(&self.players[PLAYER].discard, &mut o);
        o.push_str(",\"exhaust\":");
        self.digest_pile(&self.players[PLAYER].exhaust, &mut o);
        o.push_str(",\"enemies\":[");
        for (i, &e) in self.enemies.iter().enumerate() {
            let c = self.cr(e);
            if i > 0 {
                o.push(',');
            }
            write!(o, "[\"{}\",{},{},{},", c.monster_id, c.hp, c.max_hp, c.block).unwrap();
            self.digest_powers(e, &mut o);
            match c.ai.as_ref().and_then(|ai| ai.peek_move().map(|m| ai.state_id(m))) {
                Some(id) => write!(o, ",\"{id}\"").unwrap(),
                None => o.push_str(",null"),
            }
            write!(o, ",{}]", c.is_dead()).unwrap();
        }
        if !self.allies.is_empty() {
            // `tools/r7_export.py`: "allies" (pets, Osty) = [monster_id, hp, max_hp, block, powers, dead].
            o.push_str("],\"allies\":[");
            for (i, &a) in self.allies.iter().enumerate() {
                let c = self.cr(a);
                if i > 0 {
                    o.push(',');
                }
                write!(o, "[\"{}\",{},{},{},", c.monster_id, c.hp, c.max_hp, c.block).unwrap();
                self.digest_powers(a, &mut o);
                write!(o, ",{}]", c.is_dead()).unwrap();
            }
        }
        o.push_str("],\"potions\":[");
        for (i, pt) in self.run.potions.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            match pt {
                Some(id) => write!(o, "\"{}\"", id.name()).unwrap(),
                None => o.push_str("null"),
            }
        }
        let r = &self.run.rngs;
        write!(o, "],\"rng\":{{\"combat\":{},\"combat_card_generation\":{},\"combat_card_selection\":{},\
            \"combat_energy_costs\":{},\"combat_orbs\":{},\"combat_potion\":{},\"combat_potion_generation\":{},\
            \"combat_targets\":{},\"monster_ai\":{},\"niche\":{},\"rewards\":{},\"shops\":{},\"shuffle\":{},\
            \"transformations\":{},\"treasure_room\":{},\"unknown_map_point\":{},\"up_front\":{}}}}}",
            self.rng.counter, r.combat_card_generation.counter, r.combat_card_selection.counter,
            r.combat_energy_costs.counter, r.combat_orbs.counter, r.combat_potion_generation.counter,
            r.combat_potion_generation.counter, r.combat_targets.counter, r.monster_ai.counter, r.niche.counter,
            r.rewards.counter, r.shops.counter, r.shuffle.counter, r.transformations.counter,
            r.treasure_room.counter, r.unknown_map_point.counter, r.up_front.counter).unwrap();
        o
    }
}
