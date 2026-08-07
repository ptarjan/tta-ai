//! InfraBot -- the orthodox Iron + Irrigation line (the 3-4 player book).
//!
//! Ports `engine/bots/variants/infrastructure.py`; read that file's own
//! module doc comment for the full citation trail. This is Camp A of the
//! "Iron vs 3-Bronze" row in `docs/EXPERT_STRATEGY.md`'s disagreements
//! table -- the line the ~100-game 3-4p strategy guide teaches, and the
//! direct opponent of [`super::tempo`].
//!
//! The priority list this profile encodes:
//!
//! 1. Reach 2 farm / 3 mine / 2 lab by turn 3 -- `build_bonus`'s mine/farm/
//!    lab entries (mine biggest: "Bronze needs more workers because stone is
//!    much more valuable than food in the first age").
//! 2. Upgrade the three Bronze mines to Iron -- `tech_bonus`'s Iron entry,
//!    player-count-keyed (strongest at 3-4p: "not taking Iron creates a big
//!    risk where Coal is hate drafted", weakest at 2p per the same corpus).
//! 3. Irrigation at the end of Age I ("predestined to be developed at the
//!    end of Age I, not significantly earlier or later" -- rushing it causes
//!    corruption and happiness trouble; `RULES` does not special-case
//!    timing, the price/hand-size discipline every archetype shares already
//!    discourages rushing a card this expensive).
//! 4. Hit the published rock benchmarks (`prod_weights.resources: 1.3`, the
//!    biggest axis weight in this profile).
//! 5. Alchemy for the science engine, targeting ~4 science/turn at the end
//!    of Age I.
//! 6. Constitutional Monarchy as the government target -- "generally the
//!    best peaceful development in the game" (`tech_bonus`).
//! 7. Military to the floor only; economy first ([`RULES`] is the shared
//!    `DEFAULT_PROFILE` order unchanged).
//!
//! WHY IT IS NOT JUST BOOKBOT: `BookBot` v2 already leans orthodox, so this
//! is deliberately the closest roster member to it. What differs: the mine
//! and farm tracks carry explicit priority bonuses instead of falling out
//! of generic production value, and upgrading in place is promoted above
//! taking cards -- [`RULES`] here is the shared default, while
//! [`super::tempo::RULES`] is its exact inverse (`Upgrade` last instead of
//! mid-list), which is what makes the pair a real disagreement rather than
//! two flavours of one plan.
use super::{Pc, Profile, RuleId, DEFAULT_PROFILE};
use crate::cards::CardType;

pub(crate) const PROFILE: Profile = Profile {
    tech_bonus: &[
        ("Iron", Pc { p2: 4.0, p3: 7.0, p4: 8.0 }),
        ("Coal", Pc { p2: 2.0, p3: 4.0, p4: 5.0 }),
        ("Irrigation", Pc::flat(6.0)),
        ("Selective Breeding", Pc::flat(4.0)),
        ("Alchemy", Pc::flat(6.0)),
        ("Scientific Method", Pc::flat(4.0)),
        ("Code of Laws", Pc::flat(5.0)),
        ("Constitutional Monarchy", Pc::flat(6.0)),
        ("Architecture", Pc::flat(3.0)),
        ("Masonry", Pc::flat(-3.0)),
        ("Oil", Pc::flat(-6.0)),
    ],
    tech_veto: &[("Oil", Pc::flat(true))],
    card_bonus: &[
        ("Universitas Carolina", Pc::flat(2.0)),
        ("Engineering Genius (A)", Pc::flat(3.0)),
        ("Rich Land (A)", Pc::flat(2.0)),
    ],
    leader_bonus: &[
        ("Aristotle", Pc::flat(2.0)),
        ("Frederick Barbarossa", Pc::flat(2.0)),
        ("Moses", Pc::flat(-1.0)),
    ],
    prod_weights: super::ProdWeights {
        food: Pc::flat(1.1),
        resources: Pc::flat(1.3),
        science: Pc::flat(1.0),
        culture: Pc::flat(0.9),
        happy: Pc::flat(1.0),
        strength: Pc::flat(1.0),
    },
    build_bonus: &[(CardType::Mine, 1.5), (CardType::Farm, 0.5), (CardType::Lab, 0.5)],
    pop_appetite: 1.2,
    max_take_cost: [2, 2, 2, 3, 3],
    wonder_appetite: 1.0,
    wonder_max: 3,
    ..DEFAULT_PROFILE
};

/// Upgrading in place is promoted above the card row -- one civil action, no
/// new worker, and it frees the old card's production immediately. This is
/// the shared `DEFAULT_PROFILE` order unchanged; see this module's top doc
/// comment for why that IS the point.
pub(crate) const RULES: &[RuleId] = &[
    RuleId::Round1,
    RuleId::Revolution,
    RuleId::PlayLeader,
    RuleId::Happiness,
    RuleId::MilitaryFloor,
    RuleId::WonderStep,
    RuleId::Population,
    RuleId::PlaceWorker,
    RuleId::Upgrade,
    RuleId::Develop,
    RuleId::ActionCard,
    RuleId::TakeCard,
];

#[cfg(test)]
mod tests {
    use crate::bots::variants::{Archetype, VariantBot};
    use crate::game;

    /// Rule 2: the Iron bonus grows with table size (2p weakest, 4p
    /// strongest) -- the opposite parameterization from a bot that plays
    /// Iron for its own sake regardless of player count. Revert `tech_bonus`'s
    /// Iron entry to a flat value and this fails.
    #[test]
    fn infra_bot_values_iron_more_at_larger_tables() {
        let entry = super::PROFILE
            .tech_bonus
            .iter()
            .find(|&&(name, _)| name == "Iron")
            .map(|&(_, pc)| pc)
            .expect("InfraBot must bonus Iron");
        assert!(entry.resolve(4) > entry.resolve(2), "the 4p Iron bonus should exceed the 2p one");
    }

    /// Rule 1/4: InfraBot values a resource-producing card more than
    /// TempoBot's shared-default profile does (`prod_weights.resources` is
    /// 1.3 here, vs. `DEFAULT_PROFILE`'s 1.0). Revert the resources axis to
    /// 1.0 and this fails.
    #[test]
    fn infra_bot_values_resource_production_more_than_the_neutral_profile_does() {
        let state = game::new_game(2, 2);
        let ctx = crate::bots::book::Ctx::new(&state, 0, 2, Default::default());
        let mine_prod = crate::cards::Production { food: 0, resources: 2, science: 0, culture: 0, happy: 0, strength: 0 };
        let default_v = super::super::prod_value(mine_prod, &ctx, &super::super::DEFAULT_PROFILE);
        let infra_v = super::super::prod_value(mine_prod, &ctx, &super::PROFILE);
        assert!(infra_v > default_v, "resource-weighted value ({infra_v}) should exceed the neutral one ({default_v})");
    }

    /// End-to-end sanity: InfraBot finishes a legal 2p game without
    /// panicking.
    #[test]
    fn infra_bot_completes_a_two_player_game_without_panicking() {
        let bot = VariantBot::new(Archetype::Infra);
        let mut state = game::new_game(2, 808);
        let mut turns = 0;
        while !state.game_over {
            let mv = bot.pick(&state);
            crate::apply::apply(&mut state, mv);
            turns += 1;
            assert!(turns < 5000);
        }
    }
}
