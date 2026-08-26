================================================================================
THROUGH THE AGES -- CREDIT-HALF SPREAD (creditspread, spec 2026-08-26)
================================================================================
Spec: analysis/credit_spread_measurement_spec_2026-08-26.txt (frozen at af57966).
The 20 credit keys (spec section 2) have NO coordinate in the linear feature
vector: their only readers are the card pricers, which resolve their own credit
weight from the FROZEN vector inside eval::linear_features, so featspread's
spread instrument reads exactly 0.0 for them by construction. The probe in
THIS measurement is applied to the FREEZE (not the dot vector -- c * 0.0 = 0.
0 there); S_d comes entirely from the pricers re-running under the probe.

Per credit key k, per decision d (candidate set |C| > 1), c = 1.0 FIXED:
  phi_c = candidate_features(s, legal, allow_resign, freeze=champion)   [shared]
  phi_p = candidate_features(s, legal, allow_resign, freeze=champ[k=1.0]) [per key]
  S_d(1.0) = max_m dot(pw, phi_p(m)) - min_m dot(pw, phi_p(m))
  s_k(d)  = S_d(1.0)/1.0 -- the normalized slope: a pure function of state and
           legal moves, no weight in it (spec section 4.2); commensurable with
           the featspread spread rows of the other 169 keys.
Linearity test (spec section 4.3): phi_p2 under freeze champ[k=2.0]; if
S_d(2.0) is not ~2*S_d(1.0) the key is GATED (a gate threshold lies between
the probes): raw readings go to a separate section, NO bound is emitted

T_players is re-measured in THIS run (p95 over decisions of the TOTAL spread
max-min dot(champion, phi_c) over C) -- P95_TOTAL_SPREAD (weights.rs) is a
stale sample from featspread's own run and must not be mixed with this one.

bound(k, players) = CLAMP_T * T / p95_slope, capped at CLAMP_BLIND.

[REQUIRED CAVEAT, verbatim]
S_d is a LEVER (max-min of the probe-perturbed score over thecandidate set), not INFLUENCE. A large S_d does not imply the keychanges the chosen move. Read flip_rate_zero and flip_rate_abs forinfluence; read p95_slope for the scale of the bound.

COST NOTE: per decision, 1 shared phi_c + 20 per-key phi_p (c=1.0) +
phi_p2 (c=2.0, the linearity test) + 2 shared flip vectors (k->0.0, k->abs) =
43 candidate_features calls (vs 1 for featspread). phi_p2 is NOT shared across
keys (the spec's 24-count assumed it could be; each key's c=2.0 freeze is a
different vector and the pricers are not delta-able -- spec section 5's COST
NOTE concedes the pricers are the dominant cost and not linear in a way that
supports a delta).

2p: champion rust_champion_2p.json md5 70bba311291371e577419493ed45b82d
3p: champion rust_champion_3p.json md5 6b8dcae87471fa2f4cde9b5b99d0c525
4p: champion rust_champion_4p.json md5 6f9dd8abd394cfea4b3c3c674149ede5
games_per_count=40 seed=0 threads=4

================================================================================
PER-KEY TABLE -- one table carrying p95_slope, bound AND the multcheck
flip rates (spec section 6): flip_zero/flip_abs are counterfactual
argmax-flip rates (the INFLUENCE measure), touched is the fraction of
decisions where the c=1.0 probe changed any candidate's score.
================================================================================
-- 2p -- (decisions 10153)
key                             champ_w     fire    p95_slope          bound      flip_zero      flip_abs       touched       GATED?
card_rate_credit                 0.2366   0.9987   214.092659       1.000000       0.462720      0.083128     0.000000          YES
unit_strength_credit             0.0310   0.9987   214.092659       1.000000       0.462720      0.083128     0.000000          YES
territory_credit                 0.1883   0.9987   213.678516       1.001938       0.462720      0.083128     0.375062          YES
bonus_card_credit               -0.2094   0.9987   213.808550       1.001329       0.462720      0.083128     0.638333          YES
card_board_credit                0.2440   0.9987   361.703315       0.591901       0.462720      0.083128     0.949276          YES
tech_board_credit                0.0098   0.9987   278.666376       0.768276       0.462720      0.083128     0.937358          YES
action_board_credit              0.5240   0.9987   214.355293       0.998775       0.462720      0.083128     0.999902          YES
gov_board_credit                 0.0000   0.9987   229.534414       0.932726       0.462720      0.083128     0.583276          YES
wonder_board_credit              0.0000   0.9987   214.099315       0.999969       0.462720      0.083128     0.405102          YES
tactic_board_credit              0.1372   0.9987   214.092659       1.000000       0.462720      0.083128     0.168719          YES
aggression_board_credit          0.8920   0.9987   214.092659       1.000000       0.462720      0.083128     0.096917          YES
war_board_credit                 2.8044   0.9987   214.092659       1.000000       0.462720      0.083128     0.235891          YES
pact_board_credit                0.1591   0.9987   214.092659       1.000000       0.462720      0.083128     0.000000          YES
event_board_credit               0.1459   0.9987   214.092659       1.000000       0.462720      0.083128     0.638629          YES
unit_tech_credit                 0.0416   0.9987   256.733744       0.833909       0.462720      0.083128     0.762041          YES
build_fresh_credit               0.1197   0.9987   213.609249       1.002263       0.462720      0.083128     0.670442          YES
restricted_resource_credit       0.0475   0.9987   214.063674       1.000135       0.462720      0.083128     0.472176          YES
free_action_credit               0.0479   0.9987   214.091578       1.000005       0.462720      0.083128     0.996454          YES
tactic_reach_credit              0.0756   0.9987   214.092659       1.000000       0.462720      0.083128     0.132375          YES
card_board_leader               -0.1938   0.9987   509.119690       0.420515       0.462720      0.083128     0.562888          YES

-- 3p -- (decisions 16002)
key                             champ_w     fire    p95_slope          bound      flip_zero      flip_abs       touched       GATED?
card_rate_credit                -5.9355   0.9981   428.590229       0.998218       0.180852      0.002750     0.000000          YES
unit_strength_credit             0.0249   0.9981   428.590229       0.998218       0.180852      0.002750     0.000000          YES
territory_credit                 0.0430   0.9981   428.590229       0.998218       0.180852      0.002750     0.470816          YES
bonus_card_credit                4.2786   0.9981   428.590229       0.998218       0.180852      0.002750     0.563242          YES
card_board_credit                0.3404   0.9981   452.664614       0.945129       0.180852      0.002750     0.724159          YES
tech_board_credit                0.0000   0.9981   430.344175       0.994149       0.180852      0.002750     0.972753          YES
action_board_credit              0.1666   0.9980   433.204233       0.987586       0.180852      0.002750     0.998563          YES
gov_board_credit                 0.5677   0.9981   428.788903       0.997755       0.180852      0.002750     0.586239          YES
wonder_board_credit              0.1415   0.9981   428.590229       0.998218       0.180852      0.002750     0.000250          YES
tactic_board_credit              0.0219   0.9981   428.590229       0.998218       0.180852      0.002750     0.120297          YES
aggression_board_credit          0.0000   0.9981   428.590229       0.998218       0.180852      0.002750     0.142607          YES
war_board_credit                 0.0386   0.9981   428.680119       0.998009       0.180852      0.002750     0.184602          YES
pact_board_credit                0.0000   0.9982   428.590229       0.998218       0.180852      0.002750     0.132608          YES
event_board_credit               0.1049   0.9981   428.590229       0.998218       0.180852      0.002750     0.660605          YES
unit_tech_credit                 1.2215   0.9981   428.590229       0.998218       0.180852      0.002750     0.351581          YES
build_fresh_credit               0.0793   0.9981   444.040560       0.963485       0.180852      0.002750     0.213223          YES
restricted_resource_credit       0.0192   0.9981   428.724694       0.997905       0.180852      0.002750     0.307524          YES
free_action_credit               0.0779   0.9981   428.590229       0.998218       0.180852      0.002750     0.991189          YES
tactic_reach_credit              0.0747   0.9981   428.590229       0.998218       0.180852      0.002750     0.061742          YES
card_board_leader               -0.0274   0.9981   428.080460       0.999407       0.180852      0.002750     0.560930          YES

-- 4p -- (decisions 24433)
key                             champ_w     fire    p95_slope          bound      flip_zero      flip_abs       touched       GATED?
card_rate_credit                -0.5723   0.9939   417.306631       0.998086       0.448492      0.038063     0.000000          YES
unit_strength_credit             0.0000   0.9939   689.490925       0.604080       0.448492      0.038063     0.840871          YES
territory_credit                 0.0000   0.9939   419.145642       0.993707       0.448492      0.038063     0.366799          YES
bonus_card_credit               -0.5476   0.9939   417.399033       0.997865       0.448492      0.038063     0.347931          YES
card_board_credit                0.1949   0.9939   597.606555       0.696960       0.448492      0.038063     0.961364          YES
tech_board_credit                0.0579   0.9939   492.363861       0.845935       0.448492      0.038063     0.913887          YES
action_board_credit              0.0595   0.9940   417.327548       0.998036       0.448492      0.038063     0.965088          YES
gov_board_credit                 0.0000   0.9941   433.532660       0.960730       0.448492      0.038063     0.753284          YES
wonder_board_credit              0.1674   0.9939   417.306631       0.998086       0.448492      0.038063     0.041788          YES
tactic_board_credit              0.0940   0.9939   483.781390       0.860942       0.448492      0.038063     0.545942          YES
aggression_board_credit          0.1444   0.9939   417.306631       0.998086       0.448492      0.038063     0.135022          YES
war_board_credit                 1.3077   0.9939   417.306631       0.998086       0.448492      0.038063     0.142676          YES
pact_board_credit                0.0096   0.9939   418.146180       0.996082       0.448492      0.038063     0.102975          YES
event_board_credit               0.0228   0.9939   417.459846       0.997719       0.448492      0.038063     0.683870          YES
unit_tech_credit                 0.0000   0.9939   814.725268       0.511225       0.448492      0.038063     0.839193          YES
build_fresh_credit               0.0228   0.9940   416.874748       0.999120       0.448492      0.038063     0.568043          YES
restricted_resource_credit       0.0000   0.9939   417.115431       0.998543       0.448492      0.038063     0.285434          YES
free_action_credit               0.0000   0.9939   417.306631       0.998086       0.448492      0.038063     0.933696          YES
tactic_reach_credit              0.9598   0.9939   417.309554       0.998079       0.448492      0.038063     0.531781          YES
card_board_leader               -0.0975   0.9939   621.851688       0.669786       0.448492      0.038063     0.488561          YES

================================================================================
T RE-MEASURED IN THIS RUN -- p50/p95/max of the TOTAL spread
max-min dot(champion, phi_c) over the candidate set, per decision (spec step 6)
================================================================================
count     n_decisions          p50          p95          max
2p              10153       76.153      214.093      542.885
3p              16002      135.231      427.826     1272.369
4p              24433      174.032      416.508     1334.743

================================================================================
GATED KEYS -- S_d(2.0) not ~2*S_d(1.0) at firing decisions (a gate threshold
lies between the two probes, spec section 4.3). NO normalized bound is
emitted for these; they stay at CLAMP_BLIND (the RUST TABLE emits 0.000000
for them, so clamp_bound's own <= 0.0 fallback applies).
Raw p95 readings over FIRING decisions, both probes:
================================================================================
key                             count   p95 S_d(1.0)   p95 S_d(2.0) gated_frac bound (CLAMP_BLIND)
card_rate_credit                   2p     214.092659     214.092659     1.0000      60.000000
unit_strength_credit               2p     214.092659     214.092659     1.0000      60.000000
territory_credit                   2p     213.678516     213.574764     1.0000      60.000000
bonus_card_credit                  2p     213.808550     213.678516     0.9874      60.000000
card_board_credit                  2p     361.703315     753.444321     0.9817      60.000000
tech_board_credit                  2p     278.666376     351.420743     0.9968      60.000000
action_board_credit                2p     214.355293     214.972549     0.9999      60.000000
gov_board_credit                   2p     229.534414     287.203780     0.9952      60.000000
wonder_board_credit                2p     214.099315     214.100824     1.0000      60.000000
tactic_board_credit                2p     214.092659     214.092659     1.0000      60.000000
aggression_board_credit            2p     214.092659     214.092659     0.9998      60.000000
war_board_credit                   2p     214.092659     214.092659     0.9961      60.000000
pact_board_credit                  2p     214.092659     214.092659     1.0000      60.000000
event_board_credit                 2p     214.092659     214.092659     1.0000      60.000000
unit_tech_credit                   2p     256.733744     320.992054     0.9983      60.000000
build_fresh_credit                 2p     213.609249     213.707267     1.0000      60.000000
restricted_resource_credit         2p     214.063674     214.034564     1.0000      60.000000
free_action_credit                 2p     214.091578     214.127454     1.0000      60.000000
tactic_reach_credit                2p     214.092659     214.092659     1.0000      60.000000
card_board_leader                  2p     509.119690     877.251054     0.9979      60.000000
card_rate_credit                   3p     428.590229     428.590229     1.0000      60.000000
unit_strength_credit               3p     428.590229     428.590229     1.0000      60.000000
territory_credit                   3p     428.590229     427.913157     0.9997      60.000000
bonus_card_credit                  3p     428.590229     428.590229     0.9994      60.000000
card_board_credit                  3p     452.664614     560.402848     0.9991      60.000000
tech_board_credit                  3p     430.344175     432.735161     0.9999      60.000000
action_board_credit                3p     433.204233     439.751652     1.0000      60.000000
gov_board_credit                   3p     428.788903     428.795664     0.9999      60.000000
wonder_board_credit                3p     428.590229     428.590229     1.0000      60.000000
tactic_board_credit                3p     428.590229     428.590229     1.0000      60.000000
aggression_board_credit            3p     428.590229     428.724694     0.9994      60.000000
war_board_credit                   3p     428.680119     428.724694     0.9996      60.000000
pact_board_credit                  3p     428.590229     428.590229     0.9995      60.000000
event_board_credit                 3p     428.590229     428.590229     1.0000      60.000000
unit_tech_credit                   3p     428.590229     429.824635     1.0000      60.000000
build_fresh_credit                 3p     444.040560     467.018595     1.0000      60.000000
restricted_resource_credit         3p     428.724694     429.151946     1.0000      60.000000
free_action_credit                 3p     428.590229     428.590229     1.0000      60.000000
tactic_reach_credit                3p     428.590229     428.590229     1.0000      60.000000
card_board_leader                  3p     428.080460     428.715480     1.0000      60.000000
card_rate_credit                   4p     417.306631     417.306631     1.0000      60.000000
unit_strength_credit               4p     689.490925    1180.672298     0.9949      60.000000
territory_credit                   4p     419.145642     420.830133     1.0000      60.000000
bonus_card_credit                  4p     417.399033     417.712653     0.9993      60.000000
card_board_credit                  4p     597.606555    1068.988754     0.9904      60.000000
tech_board_credit                  4p     492.363861     733.257937     0.9972      60.000000
action_board_credit                4p     417.327548     417.854012     1.0000      60.000000
gov_board_credit                   4p     433.532660     495.437130     0.9978      60.000000
wonder_board_credit                4p     417.306631     417.592453     1.0000      60.000000
tactic_board_credit                4p     483.781390     608.837152     0.9995      60.000000
aggression_board_credit            4p     417.306631     417.399033     1.0000      60.000000
war_board_credit                   4p     417.306631     417.399033     1.0000      60.000000
pact_board_credit                  4p     418.146180     419.024610     0.9998      60.000000
event_board_credit                 4p     417.459846     417.718049     0.9999      60.000000
unit_tech_credit                   4p     814.725268    1441.010956     0.9986      60.000000
build_fresh_credit                 4p     416.874748     417.399033     1.0000      60.000000
restricted_resource_credit         4p     417.115431     417.115431     1.0000      60.000000
free_action_credit                 4p     417.306631     417.306631     1.0000      60.000000
tactic_reach_credit                4p     417.309554     420.966930     1.0000      60.000000
card_board_leader                  4p     621.851688    1024.844544     0.9992      60.000000


================================================================================
RUST TABLE -- paste as the credit arms of WeightKey::p95_candidate_spread
================================================================================
// 2p: decisions 10153 T (re-measured) 214.092659
// 3p: decisions 16002 T (re-measured) 427.826433
// 4p: decisions 24433 T (re-measured) 416.507771
// NOTE: T is re-measured in THIS run (spec section 5, step 6); it is
// intentionally NOT the P95_TOTAL_SPREAD constant -- mixing the two
// samples is what the spec forbids.
match self {
    WeightKey::CardRateCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::UnitStrengthCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::TerritoryCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::BonusCardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::CardBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::TechBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::ActionBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::GovBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::WonderBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::TacticBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::AggressionBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::WarBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::PactBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::EventBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::UnitTechCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::BuildFreshCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::RestrictedResourceCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::FreeActionCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::TacticReachCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::CardBoardLeader => [0.000000, 0.000000, 0.000000],
}
