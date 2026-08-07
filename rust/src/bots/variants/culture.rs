//! CultureBot -- the theater/opera culture-rate engine.
//!
//! Ports `engine/bots/variants/culture.py`; read that file's own module doc
//! comment for the full citation trail. Short version: this plays Camp B of
//! the "Age I culture" row in `docs/EXPERT_STRATEGY.md`'s disagreements
//! table. Camp A (the majority) says culture is irrelevant in Age I and that
//! building it early is the classic way to throw a game; Camp B is a real,
//! cited position ("I find it hard to catch up if you totally ignore culture
//! production in Age I ... Temple + Drama + Wonder(s) give me about 4-5
//! culture per turn" -- old.reddit r/throughtheages).
//!
//! The priority list this profile encodes:
//!
//! 1. A culture rate, built early and compounded -- [`PROFILE`]'s
//!    `prod_weights.culture` and `build_bonus` (theater/temple/library).
//!    Damped as the table grows (2.0/1.7/1.6 at 2/3/4p): early culture is
//!    much more dangerous with more opponents able to point a war at the
//!    leader.
//! 2. Theaters for culture, not happiness (Drama -> Opera -> Movies;
//!    Printing Press -> Journalism -> Multimedia, which carry culture AND
//!    science so this line does not starve on tech) -- `tech_bonus`.
//! 3. Shakespeare and Bach ("+1 culture per theater, cheap theater tech"),
//!    Bach's bonus keyed 2p-vs-multiplayer per the sources' explicit split.
//! 4. Taj Mahal, the only variant that pays above 1 CA for it (`card_bonus`
//!    lifts [`super::best_take`]'s "Taj/Great Wall at 1 CA only" guard).
//! 5. St. Peter's Basilica doubles as this line's happiness solution.
//! 6. Age III impacts are the payoff (Impact of Buildings/Variety/Happiness
//!    all reward a tall urban civilisation).
//! 7. Military still to the floor -- rule 7 is not optional: "gaining
//!    culture instead of building your infrastructure and military is a
//!    recipe for defeat," and a culture engine at +30/turn "makes you the
//!    War over Culture target." [`RULES`] keeps `MilitaryFloor` ahead of
//!    `WonderStep`, unlike [`super::wonder`]'s roster entry.
//!
//! KNOWN WEAKNESS, stated honestly (the point of the variant): this is the
//! "spam culture generators, then lose two wars over culture" mistake --
//! training against this pool should learn to punish it by declaring war.
use super::{MilStance, Pc, Profile, RuleId, DEFAULT_PROFILE};
use crate::cards::CardType;

pub(crate) const PROFILE: Profile = Profile {
    prod_weights: super::ProdWeights {
        food: Pc::flat(1.0),
        resources: Pc::flat(0.9),
        science: Pc::flat(1.0),
        culture: Pc { p2: 2.0, p3: 1.7, p4: 1.6 },
        happy: Pc::flat(1.2),
        strength: Pc::flat(1.0),
    },
    tech_bonus: &[
        ("Drama", Pc::flat(5.0)),
        ("Opera", Pc::flat(8.0)),
        ("Movies", Pc::flat(6.0)),
        ("Printing Press", Pc::flat(5.0)),
        ("Journalism", Pc::flat(6.0)),
        ("Multimedia", Pc::flat(5.0)),
        ("Theology", Pc::flat(6.0)),
        ("Organized Religion", Pc::flat(4.0)),
        ("Architecture", Pc::flat(5.0)),
        ("Code of Laws", Pc::flat(4.0)),
        ("Alchemy", Pc::flat(3.0)),
        ("Oil", Pc::flat(-6.0)),
    ],
    tech_veto: &[("Oil", Pc::flat(true))],
    card_bonus: &[
        ("Taj Mahal", Pc::flat(5.0)),
        ("St. Peter's Basilica", Pc::flat(4.0)),
        ("Eiffel Tower", Pc::flat(3.0)),
        ("Hollywood", Pc::flat(3.0)),
        ("Endowment for the Arts", Pc::flat(3.0)),
    ],
    leader_bonus: &[
        ("William Shakespeare", Pc::flat(5.0)),
        ("J. S. Bach", Pc { p2: 5.0, p3: 2.0, p4: 2.0 }),
        ("Joan of Arc", Pc::flat(3.0)),
        ("Homer", Pc { p2: 0.0, p3: 1.5, p4: 1.5 }),
        ("Michelangelo", Pc::flat(1.0)),
    ],
    wonder_bonus: &[
        ("Taj Mahal", Pc::flat(1.8)),
        ("St. Peter's Basilica", Pc::flat(1.3)),
        ("Eiffel Tower", Pc::flat(1.3)),
        ("Hollywood", Pc::flat(1.4)),
        ("Great Wall", Pc::flat(0.7)),
    ],
    build_bonus: &[(CardType::Theater, 3.0), (CardType::Temple, 2.0), (CardType::Library, 2.0), (CardType::Arena, 0.5)],
    wonder_appetite: 1.2,
    wonder_max: 3,
    pop_appetite: 1.2,
    mil_stance: MilStance::Floor,
    mil_margin: Pc::flat(0),
    max_take_cost: [2, 2, 2, 3, 3],
    ..DEFAULT_PROFILE
};

/// Placing workers (onto theaters/temples/libraries) and upgrading urban
/// buildings in place are promoted above the wonder step: the rate compounds
/// and a wonder stage does not pay until it is finished.
pub(crate) const RULES: &[RuleId] = &[
    RuleId::Round1,
    RuleId::Revolution,
    RuleId::PlayLeader,
    RuleId::Happiness,
    RuleId::MilitaryFloor,
    RuleId::Population,
    RuleId::PlaceWorker,
    RuleId::Upgrade,
    RuleId::WonderStep,
    RuleId::Develop,
    RuleId::ActionCard,
    RuleId::TakeCard,
];

#[cfg(test)]
mod tests {
    use crate::bots::variants::{Archetype, VariantBot};
    use crate::cards::{CardType, Production};
    use crate::game;

    /// The whole thesis of this archetype: a card that produces culture is
    /// worth strictly more to CultureBot than to the neutral
    /// `DEFAULT_PROFILE` -- `prod_weights.culture` is 1.6-2.0x depending on
    /// table size, versus the default's flat 1.0x. Revert
    /// `PROFILE.prod_weights.culture` to `Pc::flat(1.0)` and this fails.
    #[test]
    fn culture_bot_values_culture_production_more_than_the_neutral_profile_does() {
        let state = game::new_game(2, 1);
        let ctx = crate::bots::book::Ctx::new(&state, 0, 2, Default::default());
        let culture_prod = Production { food: 0, resources: 0, science: 0, culture: 2, happy: 0, strength: 0 };
        let default_v = super::super::prod_value(culture_prod, &ctx, &super::super::DEFAULT_PROFILE);
        let culture_v = super::super::prod_value(culture_prod, &ctx, &super::PROFILE);
        assert!(culture_v > default_v, "culture-weighted value ({culture_v}) should exceed the neutral one ({default_v})");
    }

    /// Placing a free worker: CultureBot's `build_bonus` gives a Theater a
    /// flat bonus a Farm does not get (rule 2 -- "theaters are for culture,
    /// not happiness"). Revert `PROFILE.build_bonus`'s Theater entry and
    /// this fails.
    #[test]
    fn culture_bot_prefers_a_theater_over_an_equivalently_priced_farm() {
        let theater_bonus = super::super::table_lookup_type(super::PROFILE.build_bonus, CardType::Theater, 0.0);
        let farm_bonus = super::super::table_lookup_type(super::PROFILE.build_bonus, CardType::Farm, 0.0);
        assert!(theater_bonus > farm_bonus, "a theater must carry a strictly bigger build bonus than a farm");
    }

    /// This is the ONE variant that pays more than 1 CA for the Taj Mahal --
    /// every other archetype's shared `best_take` guard refuses a Taj/Great
    /// Wall pick above 1 CA unless the archetype's own `card_bonus` names it
    /// (see `super::best_take`). Revert `PROFILE.card_bonus`'s "Taj Mahal"
    /// entry and CultureBot loses this distinction, making this test fail.
    #[test]
    fn culture_bot_is_willing_to_pay_above_one_civil_action_for_the_taj_mahal() {
        assert!(
            super::PROFILE.card_bonus.iter().any(|&(name, _)| name == "Taj Mahal"),
            "CultureBot must name the Taj Mahal in card_bonus to lift the 1-CA-only guard"
        );
    }

    /// Rule 7: military is not optional even for the culture line. Starting
    /// from a state where this bot is the weakest player at the table, it
    /// must still be willing to arm rather than only building culture --
    /// i.e. `RULES` keeps `MilitaryFloor` ahead of `WonderStep`/`Develop`.
    #[test]
    fn culture_bot_keeps_military_floor_ahead_of_the_wonder_step_in_its_rule_order() {
        let mil_pos = super::RULES.iter().position(|&r| r == crate::bots::variants::RuleId::MilitaryFloor);
        let wonder_pos = super::RULES.iter().position(|&r| r == crate::bots::variants::RuleId::WonderStep);
        assert!(mil_pos < wonder_pos, "military floor must be checked before the wonder step");
    }

    /// End-to-end sanity: CultureBot finishes a legal 2p game without
    /// panicking (also covered by `super::super::tests`, repeated narrowly
    /// here so a future refactor that isolates this file still catches a
    /// break in this one archetype specifically).
    #[test]
    fn culture_bot_completes_a_two_player_game_without_panicking() {
        let bot = VariantBot::new(Archetype::Culture);
        let mut state = game::new_game(2, 4242);
        let mut turns = 0;
        while !state.game_over {
            let mv = bot.pick(&state);
            crate::apply::apply(&mut state, mv);
            turns += 1;
            assert!(turns < 5000);
        }
    }
}
