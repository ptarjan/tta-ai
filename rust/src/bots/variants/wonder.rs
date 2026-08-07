//! WonderBot -- the Michelangelo / wonder-spam line, included ON PURPOSE.
//!
//! Ports `engine/bots/variants/wonder.py`; read that file's own module doc
//! comment for the full citation trail. Every other archetype in this
//! roster is a line some strong player defends. This one is the line strong
//! players call a noob trap ("Michelangelo is bad in BGA meta ... invites
//! novice players to over-invest"), included because a training pool with
//! no bad-but-coherent opponent teaches a bot nothing about how to punish
//! one. A bot that cannot beat this has not learned that tempo and military
//! beat monuments.
//!
//! The priority list this profile encodes:
//!
//! 1. Start a wonder as early as possible and always continue it -- the
//!    trap, made structural: [`RULES`] puts `WonderStep` ABOVE
//!    `MilitaryFloor`, the single ordering the corpus says loses games
//!    ("Neglecting military, then getting culture-warred" made deliberate
//!    rather than accidental). Contrast [`super::culture`], which keeps
//!    `MilitaryFloor` ahead of `WonderStep` even though it also overbuys
//!    culture.
//! 2. Take Michelangelo (`leader_bonus`, player-count-keyed: "in four player
//!    you are either going to win with him or finish fourth") and Masonry
//!    (`tech_bonus`, "two wonder stages per action" -- the enabler the
//!    sources separately call terrible on its own economics).
//! 3. Camp A of the Hanging Gardens disagreement (`wonder_bonus`'s biggest
//!    entry) -- the oldest, base-game-only source ranks it the best Age A
//!    wonder; every other archetype in this roster plays Camp B.
//! 4. Chase Age III wonders for the one-shot culture bomb and Impact of
//!    Wonders.
//! 5. Keep the resource production a wonder habit needs (`build_bonus`'s
//!    mine entry, `tech_bonus`'s Iron/Coal).
//!
//! WHAT THIS BOT IS NOT ALLOWED TO DO: it is a trap, not a clown. The shared
//! roster discipline still applies (the convex CA price ladder, the "no
//! leader for 3 CA before Age III" rule, the "don't spend 3 CA until 5-6
//! actions" threshold) -- `price_scale` is only slightly loosened (0.85),
//! since each already-completed wonder adds +1 CA to the next one's price,
//! which is the self-limiting mechanism the trap ignores. Its failure mode
//! is over-investment in monuments, not indiscriminate overpaying.
use super::{MilStance, Pc, Profile, RuleId, DEFAULT_PROFILE};
use crate::cards::CardType;

pub(crate) const PROFILE: Profile = Profile {
    wonder_appetite: 1.8,
    wonder_max: 99,
    wonder_bonus: &[
        ("Hanging Gardens", Pc::flat(1.8)),
        ("St. Peter's Basilica", Pc::flat(1.4)),
        ("Pyramids", Pc::flat(1.2)),
        ("Library of Alexandria", Pc::flat(1.1)),
        ("Hollywood", Pc::flat(1.3)),
        ("Fast Food Chains", Pc::flat(1.3)),
        ("Internet", Pc::flat(1.2)),
        ("First Space Flight", Pc::flat(1.2)),
    ],
    leader_bonus: &[("Michelangelo", Pc { p2: 6.0, p3: 7.0, p4: 8.0 }), ("Homer", Pc::flat(2.0))],
    tech_bonus: &[
        ("Masonry", Pc::flat(7.0)),
        ("Architecture", Pc::flat(5.0)),
        ("Engineering", Pc::flat(4.0)),
        ("Iron", Pc::flat(4.0)),
        ("Coal", Pc::flat(3.0)),
        ("Oil", Pc::flat(-4.0)),
    ],
    tech_veto: &[("Oil", Pc::flat(true))],
    card_bonus: &[
        ("Hanging Gardens", Pc::flat(3.0)),
        ("St. Peter's Basilica", Pc::flat(3.0)),
        ("Engineering Genius (A)", Pc::flat(4.0)),
        ("Engineering Genius (I)", Pc::flat(4.0)),
        ("Engineering Genius (II)", Pc::flat(3.0)),
    ],
    prod_weights: super::ProdWeights {
        food: Pc::flat(1.0),
        resources: Pc::flat(1.2),
        science: Pc::flat(0.9),
        culture: Pc::flat(1.2),
        happy: Pc::flat(1.0),
        strength: Pc::flat(0.8),
    },
    build_bonus: &[(CardType::Mine, 1.0)],
    price_scale: 0.85,
    max_take_cost: [2, 2, 2, 3, 3],
    pop_appetite: 1.0,
    mil_stance: MilStance::Floor,
    mil_margin: Pc::flat(0),
    ..DEFAULT_PROFILE
};

/// The trap, made explicit: `WonderStep` sits ABOVE `MilitaryFloor`, so a
/// stage of a monument outranks not being the weakest civilisation at the
/// table -- the single ordering the corpus says loses games, and the reason
/// this archetype is in the roster.
pub(crate) const RULES: &[RuleId] = &[
    RuleId::Round1,
    RuleId::Revolution,
    RuleId::PlayLeader,
    RuleId::Happiness,
    RuleId::WonderStep,
    RuleId::MilitaryFloor,
    RuleId::Population,
    RuleId::PlaceWorker,
    RuleId::Upgrade,
    RuleId::Develop,
    RuleId::ActionCard,
    RuleId::TakeCard,
];

#[cfg(test)]
mod tests {
    use crate::bots::variants::{Archetype, RuleId, VariantBot};
    use crate::game;

    /// Rule 1, the trap made structural: `WonderStep` is checked before
    /// `MilitaryFloor` in this archetype's rule order, unlike every other
    /// archetype in the roster (see `culture::tests`'s mirror-image
    /// assertion). Revert `RULES`'s ordering and this fails.
    #[test]
    fn wonder_bot_checks_the_wonder_step_before_the_military_floor() {
        let wonder_pos = super::RULES.iter().position(|&r| r == RuleId::WonderStep);
        let mil_pos = super::RULES.iter().position(|&r| r == RuleId::MilitaryFloor);
        assert!(wonder_pos < mil_pos, "WonderBot must check the wonder step before the military floor");
    }

    /// Rule 2: Michelangelo's leader bonus grows with table size (2p: 6.0,
    /// 4p: 8.0) -- "in four player you are either going to win with him or
    /// finish fourth." Revert `leader_bonus`'s Michelangelo entry to a flat
    /// value and this fails.
    #[test]
    fn wonder_bot_values_michelangelo_more_at_larger_tables() {
        let entry = super::PROFILE
            .leader_bonus
            .iter()
            .find(|&&(name, _)| name == "Michelangelo")
            .map(|&(_, pc)| pc)
            .expect("WonderBot must bonus Michelangelo");
        assert!(entry.resolve(4) > entry.resolve(2), "the 4p Michelangelo bonus should exceed the 2p one");
    }

    /// End-to-end sanity: WonderBot finishes a legal 2p game without
    /// panicking.
    #[test]
    fn wonder_bot_completes_a_two_player_game_without_panicking() {
        let bot = VariantBot::new(Archetype::Wonder);
        let mut state = game::new_game(2, 707);
        let mut turns = 0;
        while !state.game_over {
            let mv = bot.pick(&state);
            crate::apply::apply(&mut state, mv);
            turns += 1;
            assert!(turns < 5000);
        }
    }
}
