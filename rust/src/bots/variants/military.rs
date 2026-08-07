//! MilitaryBot -- the top-2 military position line that actually cashes in.
//!
//! Ports `engine/bots/variants/military.py`; read that file's own module doc
//! comment for the full citation trail. `docs/STRENGTH_CHECK.md` found the
//! Python-era champion ending 3-player games "+6.6 strength ahead of
//! BookBot and losing by 19 culture": "It bought an army it never converted
//! into anything." This variant exists to be the opponent that does
//! convert, so training against the pool cannot get away with either
//! ignoring military or hoarding it.
//!
//! The priority list this profile encodes:
//!
//! 1. Top 2, not merely "not last" -- [`PROFILE`]'s `mil_stance: Top2`.
//!    "Top 2 military position guarantees most military events benefit you"
//!    (lightningshroud). [`super::mil_goal`]'s `econ_first_until_age: 0`
//!    holds to `BookBot`'s plain floor through Age A only while food/
//!    resources are still short -- measured (see the field's own doc
//!    comment reference in `super::Profile`) at 46.9% vs 33.8%/16.9% for
//!    gating through every age / not gating at all (n=80, 2p, paired seeds).
//! 2. The red techs in the published order (Knights/Swordsmen Age I, Cannon
//!    Age II, Air Forces + Strategy Age III, the latter two the only cards
//!    this roster reaches into the 3-CA slots for via `must_buy_3ca`).
//! 3. The 3rd military action is the key Age I breakpoint -- Warfare/
//!    Strategy in `tech_bonus`.
//! 4. Aggression thresholds are numeric (`agg_lead`), not vibes: "it takes a
//!    strength lead of 5 to guarantee a successful Age I aggression."
//! 5. Attack the leader, repeatedly, saving culture theft for later
//!    ([`AGG_ORDER`], flipped once `ctx.age >= 3`) -- see [`super::politics`].
//! 6. The war window is end of Age II / start of Age III (`war_from_age: 2`).
//! 7. Tactics discipline: hold an Age II/III tactic (exactly one copy each)
//!    until Age III or the last round -- [`super::r_tactics`], the one rule
//!    function in this port that exists only because this archetype uses it.
//! 8. Do not over-invest -- `mil_margin` is small and player-count-keyed
//!    (bigger at 2p, where there is nobody else to be attacked instead).
use super::{MilStance, Pc, Profile, RuleId, DEFAULT_PROFILE};
use crate::cards::CardType;

/// "Save the attacks that steal culture for later, start with the ones that
/// steal resources, science and yellow cubes" (BGG 2801950). Higher = fire
/// first; flipped by [`super::politics`] once `ctx.age >= 3`.
const AGG_ORDER: &[(&str, f64)] = &[
    ("Aggression: Plunder (I)", 3.0),
    ("Aggression: Plunder (II)", 3.0),
    ("Aggression: Plunder (III)", 3.0),
    ("Aggression: Spy", 3.0),
    ("Aggression: Raid (I)", 2.5),
    ("Aggression: Raid (II)", 2.5),
    ("Aggression: Raid (III)", 2.5),
    ("Aggression: Enslave", 2.5),
    ("Aggression: Annex", 2.0),
    ("Aggression: Infiltrate", 1.5),
    ("Aggression: Armed Intervention", 1.0),
];

/// Rules 1 + 8: the absolute floors published for a 3p game ("Age I: 3-4
/// units + Knights/Swordsmen + a matching tactic ~= 10; Age II: 4-6 units,
/// Age II tactic, Cannon = 15-25; Age III: ~30") -- what "top 2" means in
/// absolute terms once every bot at the table is militarily weak.
const AGE_STRENGTH_FLOOR: [i32; 5] = [0, 10, 18, 30, 30];

pub(crate) const PROFILE: Profile = Profile {
    mil_stance: MilStance::Top2,
    econ_first_until_age: Some(0),
    mil_margin: Pc { p2: 3, p3: 2, p4: 2 },
    agg_lead: [99, 4, 3, 3, 3],
    war_lead: 5,
    war_from_age: 2,
    seed_events_when_weakest: false,
    tech_bonus: &[
        ("Knights", Pc::flat(8.0)),
        ("Swordsmen", Pc::flat(5.0)),
        ("Cannon", Pc::flat(8.0)),
        ("Riflemen", Pc::flat(4.0)),
        ("Cavalrymen", Pc::flat(4.0)),
        ("Air Forces", Pc::flat(15.0)),
        ("Rockets", Pc::flat(4.0)),
        ("Modern Infantry", Pc::flat(4.0)),
        ("Tanks", Pc::flat(4.0)),
        ("Alchemy", Pc::flat(4.0)),
        ("Warfare", Pc::flat(5.0)),
        ("Strategy", Pc::flat(9.0)),
        ("Military Theory", Pc::flat(4.0)),
        ("Fundamentalism", Pc::flat(3.0)),
        ("Masonry", Pc::flat(-3.0)),
        ("Oil", Pc::flat(-6.0)),
    ],
    tech_veto: &[("Oil", Pc::flat(true))],
    must_buy_3ca: &["Air Forces", "Strategy"],
    card_bonus: &[
        ("Air Forces", Pc::flat(8.0)),
        ("Strategy", Pc::flat(5.0)),
        ("Knights", Pc::flat(4.0)),
        ("Cannon", Pc::flat(4.0)),
    ],
    leader_bonus: &[
        ("Napoleon Bonaparte", Pc::flat(5.0)),
        ("Julius Caesar", Pc { p2: 1.0, p3: 3.0, p4: 3.0 }),
        ("Alexander the Great", Pc::flat(2.0)),
        ("Joan of Arc", Pc::flat(2.0)),
        ("Genghis Khan", Pc::flat(1.0)),
        ("Michelangelo", Pc::flat(-2.0)),
    ],
    wonder_bonus: &[("Great Wall", Pc::flat(1.7)), ("Colossus", Pc { p2: 1.0, p3: 1.4, p4: 1.4 })],
    prod_weights: super::ProdWeights {
        food: Pc::flat(1.0),
        resources: Pc::flat(1.2),
        science: Pc::flat(1.0),
        culture: Pc::flat(0.8),
        happy: Pc::flat(1.0),
        strength: Pc::flat(1.6),
    },
    build_bonus: &[(CardType::Lab, 1.0), (CardType::Mine, 1.0)],
    max_take_cost: [2, 2, 3, 3, 3],
    wonder_appetite: 0.8,
    wonder_max: 2,
    pop_appetite: 1.0,
    age_strength_floor: Some(AGE_STRENGTH_FLOOR),
    agg_order: Some(AGG_ORDER),
    ..DEFAULT_PROFILE
};

/// Military is promoted above the wonder and population rules: this is the
/// variant that spends its civil actions on the army. It stays below the
/// happiness rule because an uprising costs the whole production phase.
pub(crate) const RULES: &[RuleId] = &[
    RuleId::Round1,
    RuleId::Revolution,
    RuleId::PlayLeader,
    RuleId::Happiness,
    RuleId::Tactics,
    RuleId::MilitaryFloor,
    RuleId::Population,
    RuleId::PlaceWorker,
    RuleId::WonderStep,
    RuleId::Upgrade,
    RuleId::Develop,
    RuleId::ActionCard,
    RuleId::TakeCard,
];

#[cfg(test)]
mod tests {
    use crate::bots::book::Ctx;
    use crate::bots::variants::{Archetype, VariantBot};
    use crate::game;

    /// Rule 1: with a real strength gap on the table, MilitaryBot's `Top2`
    /// target is strictly higher than the SAME profile's target would be
    /// under the shared `Floor` stance (margin reset to the default's 0,
    /// every other knob -- `age_strength_floor`, `econ_first_until_age` --
    /// held identical so only rule 1 can move the result). At game start
    /// every player is equally weak (strength 0), where `AGE_STRENGTH_FLOOR`'s
    /// Age I floor (10) alone already dominates both stances' outputs, so
    /// this test gives the rival a real army first -- see
    /// `crate::state::Tableau::insert`'s use in `effects.rs`'s own tests for
    /// the same construction pattern. Revert `PROFILE.mil_stance`/
    /// `mil_margin` to the shared default's and this fails.
    #[test]
    fn military_bot_keeps_its_strength_at_or_above_the_table_maximum() {
        let mut state = game::new_game(2, 3);
        state.age_civil = crate::cards::Age::I;
        // Every player starts with 1 Warrior already in the tableau, so bump
        // its worker count rather than `insert` a duplicate.
        let warriors = crate::cards::CardId::by_name("Warriors").expect("Warriors is the starting infantry");
        state.players[1].techs.get_mut(warriors).expect("every player starts with Warriors").workers = 15;
        let ctx = Ctx::new(&state, 0, 2, Default::default());
        let military_goal = super::super::mil_goal(&state, &state.players[0], &ctx, &super::PROFILE);
        let floor_profile = super::Profile {
            mil_stance: super::super::MilStance::Floor,
            mil_margin: super::super::Pc::flat(0),
            ..super::PROFILE
        };
        let floor_goal = super::super::mil_goal(&state, &state.players[0], &ctx, &floor_profile);
        assert!(
            military_goal > floor_goal,
            "MilitaryBot's top-2 target ({military_goal}) should exceed the same profile's floor-stance target ({floor_goal}) against a real strength gap"
        );
    }

    /// Rule 5: among two otherwise-equal aggression choices, MilitaryBot's
    /// `agg_order` prefers the resource/science/cube theft (`AGG_ORDER`
    /// weight 2.5-3.0) over the culture-stealing "Armed Intervention"
    /// (weight 1.0) before Age III -- "save the attacks that steal culture
    /// for later."
    #[test]
    fn military_bot_prefers_a_resource_theft_aggression_over_a_culture_theft_before_age_three() {
        let plunder = super::super::table_lookup_str(super::AGG_ORDER, "Aggression: Plunder (I)", 2.0);
        let armed_intervention = super::super::table_lookup_str(super::AGG_ORDER, "Aggression: Armed Intervention", 2.0);
        assert!(plunder > armed_intervention, "plunder should be cashed in before armed intervention pre-Age-III");
    }

    /// End-to-end sanity: MilitaryBot finishes a legal 2p game without
    /// panicking.
    #[test]
    fn military_bot_completes_a_two_player_game_without_panicking() {
        let bot = VariantBot::new(Archetype::Military);
        let mut state = game::new_game(2, 555);
        let mut turns = 0;
        while !state.game_over {
            let mv = bot.pick(&state);
            crate::apply::apply(&mut state, mv);
            turns += 1;
            assert!(turns < 5000);
        }
    }
}
