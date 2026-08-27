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
  d_m   = dot(pw, phi_p(m)) - dot(champion, phi_c(m))
          = dot(champion, phi_p(m) - phi_c(m))
  S_d(1.0) = max_m d_m - min_m d_m   (the per-move DELTA spread: the constant
            baseline removed, so the 168 non-credit keys cancel out -- the
            spec's 'max-min dot(pw, phi_p(m))' measured the TOTAL score, whose
            spread is T and is identical under every probe; that is why the
            first measurement was flat at T for all keys)
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
S_d is a LEVER (max-min over the candidate set of the per-moveDELTA the probe causes, d_m = dot(pw, phi_p(m)) - dot(champion,phi_c(m)) = dot(champion, phi_p(m) - phi_c(m)) -- purely thepricer re-pricing effect, baseline removed), not INFLUENCE. A largeS_d does not imply the key changes the chosen move. Readflip_rate_zero and flip_rate_abs for influence -- those two areCREDIT-CLASS statistics (all 20 credit keys zeroed / set to abstogether, one row), NOT per-key: multcheck's per-key flip ratesperturb ONE key at a time. Read p95_slope for the scale of thebound.

COST NOTE: per decision, 1 shared phi_c + 20 per-key phi_p (c=1.0) +
phi_p2 (c=2.0, the linearity test) + 2 shared flip vectors (all 20 keys -> 0.0, ->abs) =
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
PER-KEY TABLE -- one table carrying p95_slope, bound, the per-key touched
fraction, and (as a single CREDIT-CLASS row) the flip rates (spec section 6,
as corrected 2026-08-26: the flip vectors move all 20 credit keys at once, so
the flip rates are ONE number for the credit half as a whole, not per-key).
================================================================================
-- 2p -- (decisions 10153)
key                             champ_w     fire    p95_slope          bound       touched       GATED?
card_rate_credit                 0.2366   0.0000     0.000000      60.000000      0.000000           no
unit_strength_credit             0.0310   0.0000     0.000000      60.000000      0.000000           no
territory_credit                 0.1883   0.2963     0.935997      60.000000      0.375062          YES
bonus_card_credit               -0.2094   0.4845     0.391117      60.000000      0.638333           no
card_board_credit                0.2440   0.8533   333.969423       0.641055      0.949276          YES
tech_board_credit                0.0098   0.7818   139.826469       1.531131      0.937358           no
action_board_credit              0.5240   0.8595    14.743366      14.521288      0.999902          YES
gov_board_credit                 0.0000   0.4853   103.608714       2.066358      0.583276           no
wonder_board_credit              0.0000   0.3351     5.008904      42.742417      0.405102           no
tactic_board_credit              0.1372   0.1559     0.614096      60.000000      0.168719          YES
aggression_board_credit          0.8920   0.0925     0.040662      60.000000      0.096917          YES
war_board_credit                 2.8044   0.2231     0.663344      60.000000      0.235891          YES
pact_board_credit                0.1591   0.0000     0.000000      60.000000      0.000000           no
event_board_credit               0.1459   0.5117     0.133961      60.000000      0.638629          YES
unit_tech_credit                 0.0416   0.6242   151.686907       1.411412      0.762041          YES
build_fresh_credit               0.1197   0.6196     4.370563      48.985143      0.670442          YES
restricted_resource_credit       0.0475   0.3817     1.164737      60.000000      0.472176          YES
free_action_credit               0.0479   0.8284     1.293740      60.000000      0.996454          YES
tactic_reach_credit              0.0756   0.1214     0.722099      60.000000      0.132375          YES
card_board_leader               -0.1938   0.5071   530.319169       0.403705      0.562888          YES
CREDIT-CLASS flip rates (all 20 keys perturbed together):                                                      0.462720     0.083128
  flip_zero = credit half zeroed at once; flip_abs = credit half abs-set at once.These are NOT commensurable with multcheck's per-key flip rates (one key ata time). 'touched' above IS per-key (fraction of decisions where that key'sc=1.0 probe changed any candidate's score).

-- 3p -- (decisions 16002)
key                             champ_w     fire    p95_slope          bound       touched       GATED?
card_rate_credit                -5.9355   0.0000     0.000000      60.000000      0.000000           no
unit_strength_credit             0.0249   0.0000     0.000000      60.000000      0.000000           no
territory_credit                 0.0430   0.3244    12.782708      33.469155      0.470816          YES
bonus_card_credit                4.2786   0.3308     0.192774      60.000000      0.563242          YES
card_board_credit                0.3404   0.5862   170.165965       2.514172      0.724159          YES
tech_board_credit                0.0000   0.7442   145.757114       2.935201      0.972753          YES
action_board_credit              0.1666   0.7875    50.253797       8.513316      0.998563          YES
gov_board_credit                 0.5677   0.4201    17.866340      23.945947      0.586239          YES
wonder_board_credit              0.1415   0.0002     0.067218      60.000000      0.000250          YES
tactic_board_credit              0.0219   0.1117     4.438737      60.000000      0.120297          YES
aggression_board_credit          0.0000   0.1219     7.967209      53.698410      0.142607           no
war_board_credit                 0.0386   0.1606     0.373284      60.000000      0.184602          YES
pact_board_credit                0.0000   0.0994     1.955373      60.000000      0.132608           no
event_board_credit               0.1049   0.4773     0.064116      60.000000      0.660605          YES
unit_tech_credit                 1.2215   0.2797     2.790447      60.000000      0.351581          YES
build_fresh_credit               0.0793   0.1910   150.745727       2.838067      0.213223          YES
restricted_resource_credit       0.0192   0.2406    11.780547      36.316347      0.307524          YES
free_action_credit               0.0779   0.6887     0.006417      60.000000      0.991189          YES
tactic_reach_credit              0.0747   0.0555     0.192648      60.000000      0.061742          YES
card_board_leader               -0.0274   0.4373    78.194222       5.471330      0.560930          YES
CREDIT-CLASS flip rates (all 20 keys perturbed together):                                                      0.180852     0.002750
  flip_zero = credit half zeroed at once; flip_abs = credit half abs-set at once.These are NOT commensurable with multcheck's per-key flip rates (one key ata time). 'touched' above IS per-key (fraction of decisions where that key'sc=1.0 probe changed any candidate's score).

-- 4p -- (decisions 24433)
key                             champ_w     fire    p95_slope          bound       touched       GATED?
card_rate_credit                -0.5723   0.0000     0.000000      60.000000      0.000000           no
unit_strength_credit             0.0000   0.7052   503.905097       0.826560      0.840871           no
territory_credit                 0.0000   0.3206    63.607113       6.548132      0.366799           no
bonus_card_credit               -0.5476   0.2890     1.757735      60.000000      0.347931           no
card_board_credit                0.1949   0.8855   447.724763       0.930276      0.961364          YES
tech_board_credit                0.0579   0.8081   320.587077       1.299203      0.913887          YES
action_board_credit              0.0595   0.7937     8.446620      49.310584      0.965088          YES
gov_board_credit                 0.0000   0.6761   175.810309       2.369075      0.753284           no
wonder_board_credit              0.1674   0.0362    29.966359      13.899179      0.041788          YES
tactic_board_credit              0.0940   0.5012   223.880079       1.860406      0.545942          YES
aggression_board_credit          0.1444   0.1177     1.945031      60.000000      0.135022          YES
war_board_credit                 1.3077   0.1310     1.230578      60.000000      0.142676          YES
pact_board_credit                0.0096   0.0932    26.711877      15.592606      0.102975           no
event_board_credit               0.0228   0.5502     0.952076      60.000000      0.683870          YES
unit_tech_credit                 0.0000   0.6974   796.642824       0.522829      0.839193           no
build_fresh_credit               0.0228   0.5355    26.047073      15.990579      0.568043          YES
restricted_resource_credit       0.0000   0.2379     3.456802      60.000000      0.285434           no
free_action_credit               0.0000   0.7480     0.027570      60.000000      0.933696           no
tactic_reach_credit              0.9598   0.4767     0.982539      60.000000      0.531781          YES
card_board_leader               -0.0975   0.4601   701.435940       0.593793      0.488561          YES
CREDIT-CLASS flip rates (all 20 keys perturbed together):                                                      0.448492     0.038063
  flip_zero = credit half zeroed at once; flip_abs = credit half abs-set at once.These are NOT commensurable with multcheck's per-key flip rates (one key ata time). 'touched' above IS per-key (fraction of decisions where that key'sc=1.0 probe changed any candidate's score).

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
territory_credit                   2p       0.935997       2.089154     0.5572      60.000000
bonus_card_credit                  2p       0.391117       0.714517     0.4037      60.000000
card_board_credit                  2p     333.969423     775.715331     0.7589      60.000000
tech_board_credit                  2p     139.826469     281.028420     0.2535      60.000000
action_board_credit                2p      14.743366      45.719843     0.7962      60.000000
gov_board_credit                   2p     103.608714     240.838519     0.4581      60.000000
wonder_board_credit                2p       5.008904      10.017142     0.3918      60.000000
tactic_board_credit                2p       0.614096       1.325863     0.9103      60.000000
aggression_board_credit            2p       0.040662       0.417271     0.8818      60.000000
war_board_credit                   2p       0.663344       0.295721     0.9294      60.000000
event_board_credit                 2p       0.133961       0.290813     0.6576      60.000000
unit_tech_credit                   2p     151.686907     309.962091     0.8305      60.000000
build_fresh_credit                 2p       4.370563      10.239810     0.9668      60.000000
restricted_resource_credit         2p       1.164737       2.387536     0.7905      60.000000
free_action_credit                 2p       1.293740       2.652552     0.6702      60.000000
tactic_reach_credit                2p       0.722099       1.503231     0.9051      60.000000
card_board_leader                  2p     530.319169     974.540716     0.7269      60.000000
territory_credit                   3p      12.782708      26.140156     0.6128      60.000000
bonus_card_credit                  3p       0.192774       0.133976     0.5286      60.000000
card_board_credit                  3p     170.165965     428.141724     0.7942      60.000000
tech_board_credit                  3p     145.757114     173.736799     0.6283      60.000000
action_board_credit                3p      50.253797     110.497067     0.6428      60.000000
gov_board_credit                   3p      17.866340      59.193156     0.7538      60.000000
wonder_board_credit                3p       0.067218       0.116940     0.7500      60.000000
tactic_board_credit                3p       4.438737       8.976704     0.9502      60.000000
war_board_credit                   3p       0.373284       0.761557     0.9245      60.000000
event_board_credit                 3p       0.064116       0.135749     0.6944      60.000000
unit_tech_credit                   3p       2.790447       9.806570     0.8577      60.000000
build_fresh_credit                 3p     150.745727     314.478887     0.9287      60.000000
restricted_resource_credit         3p      11.780547      23.791647     0.7530      60.000000
free_action_credit                 3p       0.006417       0.013376     0.5417      60.000000
tactic_reach_credit                3p       0.192648       0.400841     0.9144      60.000000
card_board_leader                  3p      78.194222     155.451935     0.7851      60.000000
unit_strength_credit               4p     503.905097    1007.273615     0.2134      60.000000
bonus_card_credit                  4p       1.757735       2.893545     0.4494      60.000000
card_board_credit                  4p     447.724763    1003.985101     0.6434      60.000000
tech_board_credit                  4p     320.587077     660.874037     0.7737      60.000000
action_board_credit                4p       8.446620      17.427409     0.7057      60.000000
gov_board_credit                   4p     175.810309     389.350360     0.3078      60.000000
wonder_board_credit                4p      29.966359      65.956424     0.8032      60.000000
tactic_board_credit                4p     223.880079     470.984371     0.8853      60.000000
aggression_board_credit            4p       1.945031       4.218398     0.8713      60.000000
war_board_credit                   4p       1.230578       2.769181     0.8938      60.000000
event_board_credit                 4p       0.952076       1.926376     0.6403      60.000000
unit_tech_credit                   4p     796.642824    1592.982408     0.4665      60.000000
build_fresh_credit                 4p      26.047073      53.128588     0.9273      60.000000
restricted_resource_credit         4p       3.456802       3.454225     0.1681      60.000000
free_action_credit                 4p       0.027570       0.055139     0.0009      60.000000
tactic_reach_credit                4p       0.982539      25.438646     0.9044      60.000000
card_board_leader                  4p     701.435940    1340.556775     0.6944      60.000000


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
    WeightKey::UnitStrengthCredit => [0.000000, 0.000000, 503.905097],
    WeightKey::TerritoryCredit => [0.000000, 0.000000, 63.607113],
    WeightKey::BonusCardCredit => [0.391117, 0.000000, 1.757735],
    WeightKey::CardBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::TechBoardCredit => [139.826469, 0.000000, 0.000000],
    WeightKey::ActionBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::GovBoardCredit => [103.608714, 0.000000, 175.810309],
    WeightKey::WonderBoardCredit => [5.008904, 0.000000, 0.000000],
    WeightKey::TacticBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::AggressionBoardCredit => [0.000000, 7.967209, 0.000000],
    WeightKey::WarBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::PactBoardCredit => [0.000000, 1.955373, 26.711877],
    WeightKey::EventBoardCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::UnitTechCredit => [0.000000, 0.000000, 796.642824],
    WeightKey::BuildFreshCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::RestrictedResourceCredit => [0.000000, 0.000000, 3.456802],
    WeightKey::FreeActionCredit => [0.000000, 0.000000, 0.027570],
    WeightKey::TacticReachCredit => [0.000000, 0.000000, 0.000000],
    WeightKey::CardBoardLeader => [0.000000, 0.000000, 0.000000],
}
