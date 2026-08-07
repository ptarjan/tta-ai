//! TempoBot -- the wide, 3-Bronze, buy-the-5th-action line.
//!
//! Ports `engine/bots/variants/tempo.py`; read that file's own module doc
//! comment for the full citation trail. This is Camp B of the "Iron vs
//! 3-Bronze" row in `docs/EXPERT_STRATEGY.md`'s disagreements table, and the
//! line the strongest 2-player players describe.
//!
//! The priority list this profile encodes:
//!
//! 1. Do not upgrade the mine track -- [`PROFILE::upgrade_veto`] vetoes
//!    `CardType::Mine` at 2p/3p ("I have won games against high-level
//!    players with only 3 bronze all game"), but NOT at 4p, where the same
//!    corpus flips: "at least not taking Iron creates a big risk where Coal
//!    is hate drafted." [`PROFILE::tech_veto`] mirrors the same split for
//!    Iron/Coal.
//! 2. Buy the 5th civil action -- `tech_bonus`'s Code of Laws entry (12.0)
//!    is the single biggest priority in THIS archetype's own tech_bonus
//!    table, ahead of Alchemy and Knights: "1 extra civil action is as
//!    valuable as 2 upgrades of Iron." Pyramids gets the same treatment in
//!    `card_bonus` (the other source of the 5th action).
//! 3. A 4th Bronze beats an Iron upgrade -- `build_bonus`'s mine entry (a
//!    NEW mine is rewarded; upgrading an existing one is vetoed by rule 1).
//! 4. Take lots of cheap yellow cards -- `card_bonus`'s Engineering Genius/
//!    Rich Land/Urban Growth/Breakthrough/Patriotism entries, and
//!    `hand_penalty: 1.1` (slightly above the shared default, since a plan
//!    built around taking many cards is the one most exposed to hand rot).
//! 5. Stay at the cheap end of the row -- `price_scale: 1.2` (the most
//!    1-CA-biased profile in the roster) and `max_take_cost`'s Age A entry
//!    capped at 1 CA outright (every other archetype allows 2).
//! 6. Military only to the floor (`PROFILE`'s `mil_stance`/`mil_margin` are
//!    left at `DEFAULT_PROFILE`'s plain floor).
//!
//! PLAYER-COUNT PARAMETERIZATION (required: this is a 2p-derived line): the
//! lobby that produced rule 1 is almost exclusively 2p, so the mine veto is
//! pure 3-Bronze at 2p/3p and Coal (not Iron) becomes allowed at 4p.
//!
//! KNOWN WEAKNESS, stated honestly: rock production of 3/turn is below the
//! published Age II benchmark of "+5 or better." If the extra civil actions
//! do not convert into yellow-card resources, this civilisation cannot
//! afford Age II units or wonders -- the actual disagreement with
//! [`super::infrastructure`], not a bug in either side.
use super::{Pc, Profile, RuleId, DEFAULT_PROFILE};
use crate::cards::CardType;

pub(crate) const PROFILE: Profile = Profile {
    // Rule 1 + the player-count split. Oil is universally rejected at every
    // count ("the worst CP in the game").
    tech_veto: &[
        ("Iron", Pc { p2: true, p3: true, p4: false }),
        ("Coal", Pc { p2: true, p3: true, p4: false }),
        ("Oil", Pc::flat(true)),
    ],
    // Never spend a civil action turning a Bronze into anything, at 2p/3p.
    upgrade_veto: &[(CardType::Mine, Pc { p2: true, p3: true, p4: false })],
    tech_bonus: &[
        ("Code of Laws", Pc::flat(12.0)),
        ("Alchemy", Pc::flat(5.0)),
        ("Knights", Pc::flat(4.0)),
        ("Swordsmen", Pc::flat(3.0)),
        ("Irrigation", Pc::flat(1.0)),
        ("Masonry", Pc::flat(-3.0)),
    ],
    card_bonus: &[
        ("Pyramids", Pc::flat(6.0)),
        ("Engineering Genius (A)", Pc::flat(4.0)),
        ("Engineering Genius (I)", Pc::flat(3.0)),
        ("Rich Land (A)", Pc::flat(2.5)),
        ("Rich Land (I)", Pc::flat(2.5)),
        ("Urban Growth (A)", Pc::flat(2.0)),
        ("Urban Growth (I)", Pc::flat(2.0)),
        ("Breakthrough (I)", Pc::flat(2.0)),
        ("Patriotism (I)", Pc::flat(1.5)),
    ],
    leader_bonus: &[("Hammurabi", Pc::flat(3.0)), ("Moses", Pc::flat(-2.0))],
    price_scale: 1.2,
    max_take_cost: [1, 2, 2, 3, 3],
    hand_penalty: 1.1,
    build_bonus: &[(CardType::Mine, 2.0)],
    pop_appetite: 1.5,
    wonder_appetite: 0.9,
    wonder_max: 2,
    ..DEFAULT_PROFILE
};

/// Action cards and card-row picks come before upgrading anything in place,
/// because the whole plan is that a civil action spent on the row beats a
/// civil action spent on a mine -- the exact inverse of
/// [`super::infrastructure::RULES`]'s ordering, which is what makes the pair
/// a real disagreement rather than two flavours of one plan.
pub(crate) const RULES: &[RuleId] = &[
    RuleId::Round1,
    RuleId::Revolution,
    RuleId::PlayLeader,
    RuleId::Happiness,
    RuleId::MilitaryFloor,
    RuleId::ActionCard,
    RuleId::WonderStep,
    RuleId::Population,
    RuleId::PlaceWorker,
    RuleId::Develop,
    RuleId::TakeCard,
    RuleId::Upgrade,
];

#[cfg(test)]
mod tests {
    use crate::bots::variants::{Archetype, VariantBot};
    use crate::cards::CardType;
    use crate::game;

    /// Rule 1: TempoBot refuses to upgrade a mine in place at 2p, unlike
    /// InfraBot (which actively bonuses that exact move). Revert
    /// `upgrade_veto` to empty and this fails.
    #[test]
    fn tempo_bot_vetoes_upgrading_a_mine_in_place_at_two_players() {
        let vetoed = super::PROFILE
            .upgrade_veto
            .iter()
            .any(|&(typ, pc)| typ == CardType::Mine && pc.resolve(2));
        assert!(vetoed, "TempoBot must veto in-place mine upgrades at 2 players");
    }

    /// Rule 1's player-count split: the SAME veto is lifted at 4 players,
    /// per the corpus's explicit 4p counter-argument. Revert
    /// `upgrade_veto`'s `Pc` to a flat `true` and this fails.
    #[test]
    fn tempo_bot_lifts_the_mine_upgrade_veto_at_four_players() {
        let vetoed_at_4p = super::PROFILE
            .upgrade_veto
            .iter()
            .any(|&(typ, pc)| typ == CardType::Mine && pc.resolve(4));
        assert!(!vetoed_at_4p, "TempoBot must allow mine upgrades at 4 players");
    }

    /// Rule 2: "1 extra civil action is as valuable as 2 upgrades of Iron" --
    /// Code of Laws is TempoBot's own single biggest `tech_bonus` entry,
    /// bigger than every OTHER card this archetype bonuses (including its
    /// other top priorities, Alchemy and Knights). Revert it and this fails.
    #[test]
    fn tempo_bot_ranks_code_of_laws_above_every_other_entry_in_its_own_tech_bonus() {
        let code_of_laws = super::PROFILE
            .tech_bonus
            .iter()
            .find(|&&(name, _)| name == "Code of Laws")
            .map(|&(_, pc)| pc.resolve(2))
            .expect("TempoBot must bonus Code of Laws");
        for &(name, pc) in super::PROFILE.tech_bonus {
            if name == "Code of Laws" {
                continue;
            }
            let v = pc.resolve(2);
            assert!(code_of_laws > v, "Code of Laws ({code_of_laws}) should lead TempoBot's own tech_bonus table, but {name} is {v}");
        }
    }

    /// End-to-end sanity: TempoBot finishes a legal 2p game without
    /// panicking.
    #[test]
    fn tempo_bot_completes_a_two_player_game_without_panicking() {
        let bot = VariantBot::new(Archetype::Tempo);
        let mut state = game::new_game(2, 909);
        let mut turns = 0;
        while !state.game_over {
            let mv = bot.pick(&state);
            crate::apply::apply(&mut state, mv);
            turns += 1;
            assert!(turns < 5000);
        }
    }
}
