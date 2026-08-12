//! [`PolicyOrder`]: the policy head wired into search as a MOVE-ORDERING
//! PRIOR -- see `docs/NEURAL.md`'s "The policy head (2026-08-12)" section
//! for the measured numbers this is built on (top-1 agreement 0.5236 on
//! decisions with >= 4 legal moves, strongest early/mid, weakest late).
//!
//! ## What this is not
//!
//! Not a value net, not a pruning rule. [`ValueNet::forward`] here scores a
//! `(state, action)` row exactly as [`super::policy_train`] trained it --
//! one logit per legal candidate, softmax-comparable only against the OTHER
//! legal moves at the same decision (`policy_train`'s own top doc comment).
//! [`PolicyOrder::order_moves`] uses that logit only to PERMUTE a caller's
//! move list, never to drop, cap or skip an entry -- every move handed in
//! comes back out, same set, same length, just reordered. A search that
//! wants to also prune on this signal is a separate, unauthorised decision
//! (see the calling task); this module gives it nothing to prune WITH (no
//! threshold, no top-k cut).
//!
//! ## Reused buffers, not a fresh `Vec` per move
//!
//! A search node with N legal moves previously cost this module nothing;
//! with a policy loaded, it costs N forward passes. The state half of each
//! candidate's `(state, action)` row is identical across all N of them, so
//! [`Self::order_moves`] encodes it once per node (not once per move) into
//! [`Self::row`], and re-fills only the action half
//! ([`action::encode_action_into`], allocation-free) between candidates.
//! [`Self::scored`] is cleared and reused across calls rather than
//! reallocated -- house style: no fresh `Vec` per small helper in
//! search-hot code (this crate is already ~12% malloc/free by profile).
//! What remains unavoidable with today's API: one `Vec<f64>` per NODE from
//! [`encode::encode`] (that function has no `_into` counterpart yet) and
//! whatever [`ValueNet::forward`] itself allocates internally for its
//! hidden layer -- both pre-existing costs every other net caller in this
//! crate already pays, not something this module adds on top.

use crate::moves::Move;
use crate::state::GameState;

use super::action;
use super::encode;
use super::net::ValueNet;
use super::policy_train::{self, POLICY_IN_DIM};

/// A loaded policy checkpoint, ready to order legal-move lists at search
/// nodes. One instance is meant to be built ONCE (checkpoint load is real
/// I/O) and threaded `&mut` through a whole search, reusing its scratch
/// buffers across every node and every candidate.
pub struct PolicyOrder {
    net: ValueNet,
    /// `state ++ action`, [`POLICY_IN_DIM`] wide -- the state half is
    /// written once per [`Self::order_moves`] call and read by every
    /// candidate in that call; the action half is overwritten per
    /// candidate. Never resized after construction.
    row: Vec<f64>,
    /// `(logit, move)` pairs for the candidate list currently being
    /// ordered. Cleared, not reallocated, at the top of every
    /// [`Self::order_moves`] call, so its backing storage grows at most
    /// once per process (to the widest legal-move list actually seen).
    scored: Vec<(f64, Move)>,
}

impl PolicyOrder {
    /// Load a `TTAPOL01` checkpoint ([`policy_train::load_policy_checkpoint`])
    /// from `path`. The metadata `policy_train` writes alongside the weights
    /// (epoch, held-out top-1, ...) is discarded here -- this is a play-time
    /// loader, not a training report.
    ///
    /// # Errors
    /// Whatever [`policy_train::load_policy_checkpoint`] returns: a bad
    /// magic/version, a truncated file, or an `in_dim` that does not match
    /// this build's [`POLICY_IN_DIM`] (a checkpoint trained against a stale
    /// encoder).
    pub fn load(path: &std::path::Path) -> Result<PolicyOrder, String> {
        let (net, _meta) = policy_train::load_policy_checkpoint(path)?;
        Ok(PolicyOrder { net, row: vec![0.0; POLICY_IN_DIM], scored: Vec::new() })
    }

    /// Build a `PolicyOrder` from an already-in-memory net, skipping the
    /// checkpoint file entirely -- for a caller that just trained or
    /// otherwise constructed one (`policy_train::PolicyTrainer` holds a
    /// `ValueNet` directly), and for tests that need a hand-built net with
    /// no checkpoint file in the checkout to load.
    ///
    /// # Panics
    /// If `net.in_dim != POLICY_IN_DIM` (a caller bug: this net was not
    /// shaped for this module's `state ++ action` row).
    pub fn from_net(net: ValueNet) -> PolicyOrder {
        assert_eq!(net.in_dim, POLICY_IN_DIM, "PolicyOrder::from_net: net.in_dim does not match POLICY_IN_DIM");
        PolicyOrder { net, row: vec![0.0; POLICY_IN_DIM], scored: Vec::new() }
    }

    /// Reorder `moves` in place, most-preferred-by-the-net first. A pure
    /// permutation: `moves.len()` and the multiset of entries are unchanged
    /// -- see this module's top doc comment for why that is load-bearing,
    /// not incidental.
    ///
    /// Ties (including the whole list, on an untrained or degenerate net)
    /// keep their ORIGINAL relative order: [`slice::sort_by`] is stable, the
    /// same guarantee [`super::super::plan::beam`]'s own terminal-score sort
    /// already relies on ("Stable sort: ties keep the order they were
    /// appended in").
    ///
    /// A list of 0 or 1 moves is left untouched without scoring anything --
    /// there is no order to change.
    pub fn order_moves(&mut self, state: &GameState, actor: u8, moves: &mut [Move]) {
        if moves.len() <= 1 {
            return;
        }
        let state_enc = encode::encode(state, actor);
        self.row[..encode::ENCODING_DIM].copy_from_slice(&state_enc);

        self.scored.clear();
        for &mv in moves.iter() {
            action::encode_action_into(actor, mv, &mut self.row[encode::ENCODING_DIM..]);
            let logit = self.net.forward(&self.row);
            self.scored.push((logit, mv));
        }
        self.scored.sort_by(|a, b| b.0.partial_cmp(&a.0).expect("policy logit is never NaN"));

        for (slot, &(_, mv)) in moves.iter_mut().zip(self.scored.iter()) {
            *slot = mv;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::neural::policy_train::random_policy_net;
    use crate::game as G;
    use crate::legal;

    /// [`PolicyOrder::order_moves`] never changes the SET of moves it was
    /// handed, on a real opening position -- the one property that must
    /// hold even for a freshly random-initialised (untrained, effectively
    /// noise) net: order_moves is a permutation generator, not a filter.
    #[test]
    fn order_moves_is_a_permutation_never_drops_or_adds_a_move() {
        let net = random_policy_net(8, 0);
        let mut policy = PolicyOrder { net, row: vec![0.0; POLICY_IN_DIM], scored: Vec::new() };
        for players in [2, 3, 4] {
            let state = G::new_game(players, 42);
            let moves = legal::legal_moves(&state);
            let mut got = moves.as_slice().to_vec();
            let actor = state.decider();
            policy.order_moves(&state, actor, &mut got);

            let mut want_sorted = moves.as_slice().to_vec();
            let mut got_sorted = got.clone();
            want_sorted.sort_by_key(|m| format!("{m:?}"));
            got_sorted.sort_by_key(|m| format!("{m:?}"));
            assert_eq!(got_sorted, want_sorted, "{players}p: order_moves changed the move SET");
            assert_eq!(got.len(), moves.len(), "{players}p: order_moves changed the move COUNT");
        }
    }

    /// A single-candidate list is returned untouched, and scores nothing --
    /// there is only one possible order, so nothing should even call
    /// [`ValueNet::forward`] for it (this test does not observe the call
    /// count directly, but a real checkpoint being unavailable in this
    /// checkout is not a valid excuse for this test to fail, which a
    /// scoring attempt on a zero-initialised buffer would risk if the
    /// length-1 short circuit were ever removed).
    #[test]
    fn order_moves_leaves_a_single_candidate_untouched() {
        let net = random_policy_net(4, 0);
        let mut policy = PolicyOrder { net, row: vec![0.0; POLICY_IN_DIM], scored: Vec::new() };
        let state = G::new_game(2, 3);
        let mut moves = [Move::EndTurn];
        policy.order_moves(&state, 0, &mut moves);
        assert_eq!(moves, [Move::EndTurn]);
    }

    /// The ordering genuinely reflects the net's own per-candidate score,
    /// not just "some permutation": independently recompute every
    /// candidate's logit with a SEPARATE forward pass (`net.forward` on a
    /// freshly built `state ++ action` row, not `order_moves`'s internal
    /// scratch buffers), then check `order_moves`'s output is sorted
    /// descending by those independently-computed scores. Mirrors
    /// `bots::human::tests::root_shortlist_keeps_exactly_the_top_ranked_
    /// candidates_by_score`'s identical "recompute independently, then
    /// compare" shape. This is the one test here that would fail if
    /// `order_moves` silently ignored the net's output (e.g. left `moves`
    /// unsorted) -- every other test in this module tolerates any
    /// permutation.
    #[test]
    fn order_moves_matches_the_nets_own_independently_recomputed_score() {
        let net = random_policy_net(6, 11);
        let reference = net.clone();
        let mut policy = PolicyOrder { net, row: vec![0.0; POLICY_IN_DIM], scored: Vec::new() };

        let state = G::new_game(3, 5);
        let actor = state.decider();
        let moves = legal::legal_moves(&state);
        assert!(moves.len() > 2, "test needs a decision with more than 2 legal moves to be meaningful");
        let mut got = moves.as_slice().to_vec();
        policy.order_moves(&state, actor, &mut got);

        let state_enc = encode::encode(&state, actor);
        let mut row = vec![0.0; POLICY_IN_DIM];
        row[..encode::ENCODING_DIM].copy_from_slice(&state_enc);
        let mut score = |mv: Move| -> f64 {
            action::encode_action_into(actor, mv, &mut row[encode::ENCODING_DIM..]);
            reference.forward(&row)
        };

        let scores: Vec<f64> = got.iter().map(|&mv| score(mv)).collect();
        for pair in scores.windows(2) {
            assert!(
                pair[0] >= pair[1],
                "order_moves output not sorted by the net's own score: {scores:?} for order {got:?}"
            );
        }
    }
}
