//! ScienceBot -- Alchemy first, a high science rate, Age III science impacts.
//!
//! Ports `engine/bots/variants/science.py`; read that file's own module doc
//! comment for the full citation trail. The science line is the one place
//! the corpus gives a calibrated numeric scale rather than opinion (MasN's
//! science-per-turn scale at the end of Age I: "1 = a way to throw the
//! game ... 4 = a good amount ... 7 = over-invested"), so this variant is
//! built around hitting those numbers, not maximising the stat forever.
//!
//! The priority list this profile encodes:
//!
//! 1. Alchemy is the Age I priority (`tech_bonus`'s biggest single entry).
//! 2. Then the lab track: Alchemy -> Scientific Method -> Computers.
//! 3. Universitas Carolina (and Library of Alexandria, the Age A version of
//!    the same idea) are this line's wonders -- `wonder_bonus`/`card_bonus`.
//! 4. Aristotle, Leonardo, Newton (`leader_bonus`) -- Leonardo's own
//!    "needs a lab tech in hand or in play" condition is enforced by
//!    [`crate::bots::book::leader_rank`], shared by every archetype.
//! 5. Stop at the published ceiling -- [`SCIENCE_CEILING`], read through
//!    [`Profile::science_ceiling`]: once science production reaches the
//!    scale's diminishing-returns point for the age, [`super::best_build`]
//!    refuses to staff another new lab (existing ones may still be
//!    upgraded, a different move kind this filter does not touch).
//! 6. Convert science into technologies every turn -- `RULES` promotes
//!    `Develop` above the card row and even above the wonder step.
//! 7. The Age III payoff is rank-based (Impact of Science pays the whole
//!    prize to 1st at 2p and only a fraction of it at 3-4p), which is why
//!    [`SCIENCE_CEILING`] is highest at 2p.
//! 8. Balance rule: "produce at least as much science as your lowest
//!    production of food and stone" -- not separately encoded (no profile
//!    knob corresponds to it in the Python source either; `prod_weights`'s
//!    flat `1.6` on science is the whole mechanism here, same as Python).
use super::{Pc, Profile, RuleId, DEFAULT_PROFILE};
use crate::cards::CardType;

/// Rule 5, straight off MasN's scale, by age index (0=A, 1=I, 2=II, 3=III,
/// 4=IV). Above this per-turn rate the bot stops adding NEW lab workers; it
/// keeps upgrading existing ones (a different move kind, untouched).
const SCIENCE_CEILING: [Pc<f64>; 5] = [
    Pc::flat(4.0),
    Pc { p2: 6.0, p3: 5.0, p4: 5.0 },
    Pc { p2: 9.0, p3: 8.0, p4: 8.0 },
    Pc::flat(99.0),
    Pc::flat(99.0),
];

pub(crate) const PROFILE: Profile = Profile {
    prod_weights: super::ProdWeights {
        food: Pc::flat(1.0),
        resources: Pc::flat(1.0),
        science: Pc::flat(1.6),
        culture: Pc::flat(1.0),
        happy: Pc::flat(1.0),
        strength: Pc::flat(1.0),
    },
    tech_bonus: &[
        ("Alchemy", Pc::flat(10.0)),
        ("Scientific Method", Pc::flat(8.0)),
        ("Computers", Pc::flat(8.0)),
        ("Printing Press", Pc::flat(5.0)),
        ("Journalism", Pc::flat(5.0)),
        ("Multimedia", Pc::flat(4.0)),
        ("Code of Laws", Pc::flat(5.0)),
        ("Architecture", Pc::flat(3.0)),
        ("Satellites", Pc::flat(-6.0)),
        ("Oil", Pc::flat(-6.0)),
        ("Masonry", Pc::flat(-3.0)),
    ],
    tech_veto: &[("Oil", Pc::flat(true)), ("Satellites", Pc::flat(true))],
    card_bonus: &[
        ("Universitas Carolina", Pc::flat(6.0)),
        ("Library of Alexandria", Pc::flat(4.0)),
        ("First Space Flight", Pc::flat(3.0)),
        ("Breakthrough (I)", Pc::flat(3.0)),
        ("Breakthrough (II)", Pc::flat(3.0)),
    ],
    leader_bonus: &[
        ("Aristotle", Pc::flat(4.0)),
        ("Leonardo da Vinci", Pc::flat(3.0)),
        ("Isaac Newton", Pc::flat(4.0)),
        ("Moses", Pc::flat(-2.0)),
    ],
    wonder_bonus: &[
        ("Universitas Carolina", Pc::flat(1.5)),
        ("Library of Alexandria", Pc::flat(1.3)),
        ("Pyramids", Pc::flat(0.95)),
        ("First Space Flight", Pc::flat(1.4)),
        ("Great Wall", Pc::flat(0.7)),
    ],
    build_bonus: &[(CardType::Lab, 3.0), (CardType::Library, 2.0)],
    max_take_cost: [2, 2, 2, 3, 3],
    wonder_appetite: 1.0,
    wonder_max: 3,
    pop_appetite: 1.2,
    science_ceiling: Some(SCIENCE_CEILING),
    ..DEFAULT_PROFILE
};

/// Develop is promoted above every discretionary spend: unspent science was
/// the single largest defect measured in the Python-era strength check.
pub(crate) const RULES: &[RuleId] = &[
    RuleId::Round1,
    RuleId::Revolution,
    RuleId::PlayLeader,
    RuleId::Happiness,
    RuleId::MilitaryFloor,
    RuleId::Develop,
    RuleId::WonderStep,
    RuleId::Population,
    RuleId::PlaceWorker,
    RuleId::Upgrade,
    RuleId::ActionCard,
    RuleId::TakeCard,
];

#[cfg(test)]
mod tests {
    use crate::bots::book::Ctx;
    use crate::bots::variants::{Archetype, VariantBot};
    use crate::cards::CardId;
    use crate::game;
    use crate::moves::Move;

    /// Rule 5: once science production clears the ceiling for the current
    /// age, [`super::super::best_build`] must refuse to add a NEW lab
    /// worker, even when it is the only candidate offered. Revert
    /// `PROFILE.science_ceiling` to `None` and this fails -- the shared
    /// default never filters lab builds, so it would take the only move on
    /// offer regardless of `ctx.s.science`.
    #[test]
    fn science_bot_refuses_a_new_lab_once_the_science_ceiling_is_reached() {
        let state = game::new_game(2, 9);
        let mut ctx = Ctx::new(&state, 0, 2, Default::default());
        // Age A's ceiling is a flat 4.0 (see `SCIENCE_CEILING`); push well
        // past it.
        ctx.s.science = 10;
        let philosophy = CardId::by_name("Philosophy").expect("Philosophy is a real Age A lab card");
        let moves = [Move::Build { card: philosophy }];
        let p = &state.players[0];
        let chosen = super::super::best_build(&state, p, &ctx, &super::PROFILE, &moves, false);
        assert!(chosen.is_none(), "a lone lab build offered above the ceiling must be refused");
    }

    /// Rule 1: Alchemy carries the single biggest `tech_bonus` entry in this
    /// profile -- bigger than every other (positive) tech bonus it lists.
    /// Revert the Alchemy entry to 0 (or delete it) and this fails.
    #[test]
    fn science_bot_ranks_alchemy_above_every_other_tech_bonus_entry() {
        let alchemy = super::PROFILE
            .tech_bonus
            .iter()
            .find(|&&(name, _)| name == "Alchemy")
            .map(|&(_, pc)| pc.resolve(2))
            .expect("ScienceBot must bonus Alchemy");
        for &(name, pc) in super::PROFILE.tech_bonus {
            let v = pc.resolve(2);
            if v <= 0.0 {
                continue; // negative entries are penalties, not competing priorities
            }
            assert!(alchemy >= v, "Alchemy ({alchemy}) should lead every positive tech_bonus, but {name} is {v}");
        }
    }

    /// End-to-end sanity: ScienceBot finishes a legal 2p game without
    /// panicking.
    #[test]
    fn science_bot_completes_a_two_player_game_without_panicking() {
        let bot = VariantBot::new(Archetype::Science);
        let mut state = game::new_game(2, 606);
        let mut turns = 0;
        while !state.game_over {
            let mv = bot.pick(&state);
            crate::apply::apply(&mut state, mv);
            turns += 1;
            assert!(turns < 5000);
        }
    }
}
