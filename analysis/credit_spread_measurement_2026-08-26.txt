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

Per credit key k, per decision d (candidate set |C| > 1), DISPLACEMENT c = 1.0 FIXED:
  phi_c = candidate_features(s, legal, allow_resign, freeze=champion)   [shared]
  phi_p = candidate_features(s, legal, allow_resign, freeze=champ[k+w_k]) [per key]
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
DISPLACEMENT (coordinator correction, 2026-08-26): the probe SETS k to
w_k + c, not to c. A set-to-c probe made S_d(c) = h(c) - h(w_k) and the
c = 1.0/2.0 test passed iff the pricer's response h was linear on an
interval containing {w_k, 1.0, 2.0} -- trivially true at w_k = 0 (the pass
set was exactly the zero-champion-weight keys), a second-difference test
otherwise. Displacing by c measures the pricer's response AT THE
CHAMPION'S OWN OPERATING POINT, where |c|-linearity is the correct test
and gated_frac is a per-key property of the pricers.
Linearity test (spec section 4.3): phi_p2 under displacement c = 2.0; if
S_d(2.0) is not ~2*S_d(1.0) the decision counts toward the key's GATED_FRAC
(a gate threshold lies between w_k and w_k + 2.0). gated_frac is the
HEADLINE column: most keys are expected to drop to a small gated_frac and
carry a usable slope; a few keep a high one -- those few are the genuine
finding and stay at CLAMP_BLIND, named with their gated_frac in the
GATED-FRACTION section. Keys gated at more than half their firing decisions
emit NO bound (the RUST TABLE row is 0.000000; clamp_bound's own <= 0.0
fallback keeps them at CLAMP_BLIND).

T_players is re-measured in THIS run (p95 over decisions of the TOTAL spread
max-min dot(champion, phi_c) over C) -- P95_TOTAL_SPREAD (weights.rs) is a
stale sample from featspread's own run and must not be mixed with this one.

bound(k, players) = CLAMP_T * T / p95_slope, capped at CLAMP_BLIND.

[REQUIRED CAVEAT, verbatim]
S_d is a LEVER (max-min over the candidate set of the per-moveDELTA the probe causes, d_m = dot(pw, phi_p(m)) - dot(champion,phi_c(m)) = dot(champion, phi_p(m) - phi_c(m)) -- purely thepricer re-pricing effect, baseline removed), not INFLUENCE. A largeS_d does not imply the key changes the chosen move. Readflip_rate_zero and flip_rate_abs for influence -- those two areCREDIT-CLASS statistics (all 20 credit keys zeroed / set to abstogether, one row), NOT per-key: multcheck's per-key flip ratesperturb ONE key at a time. Read p95_slope for the scale of thebound.

COST NOTE: per decision, 1 shared phi_c + 20 per-key phi_p (displacement c=1.0) +
phi_p2 (displacement c=2.0, the linearity test) + 2 shared flip vectors (all 20 keys -> 0.0, ->abs) =
43 candidate_features calls (vs 1 for featspread). phi_p2 is NOT shared across
keys (each key's c=2.0 freeze is a different vector and the pricers are not
delta-able -- spec section 5's COST NOTE concedes the pricers are the dominant
cost and not linear in a way that supports a delta).

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
key                             champ_w      fire    p95_slope gated_frac          bound       touched      GATED?>
card_rate_credit                 0.2366         0     0.000000     0.0000      60.000000      0.000000           no
unit_strength_credit             0.0310         0     0.000000     0.0000      60.000000      0.000000           no
territory_credit                 0.1883      3221     1.153157     0.0000      60.000000      0.375062           no
bonus_card_credit               -0.2094      4757     0.323400     0.0000      60.000000      0.638333           no
card_board_credit                0.2440      8725   440.293944     0.0638       0.486249      0.949276           no
tech_board_credit                0.0098      7927   141.213174     0.1388       1.516096      0.937358           no
action_board_credit              0.5240      8806    30.951381     0.0945       6.917063      0.999902           no
gov_board_credit                 0.0000      4927   103.608714     0.4581       2.066358      0.583276           no
wonder_board_credit              0.0000      3402     5.008904     0.3918      42.742417      0.405102           no
tactic_board_credit              0.1372      1584     0.711767     0.0000      60.000000      0.168719           no
aggression_board_credit          0.8920       933     0.376610     0.0000      60.000000      0.096917           no
war_board_credit                 2.8044      2267     0.367623     0.0000      60.000000      0.235891           no
pact_board_credit                0.1591         0     0.000000     0.0000      60.000000      0.000000           no
event_board_credit               0.1459      5539     0.117639     0.0000      60.000000      0.638629           no
unit_tech_credit                 0.0416      6365   157.532462     0.2564       1.359038      0.762041           no
build_fresh_credit               0.1197      6299     5.088772     0.3181      42.071577      0.671329           no
restricted_resource_credit       0.0475      3840     1.222799     0.2812      60.000000      0.472176           no
free_action_credit               0.0479      8433     1.358827     0.1418      60.000000      0.996454           no
tactic_reach_credit              0.0756      1233     0.781131     0.0000      60.000000      0.132375           no
card_board_leader               -0.1938      5109   444.221547     0.0986       0.481950      0.562888           no
CREDIT-CLASS flip rates (all 20 keys perturbed together):                                                                  0.462720     0.083128
  flip_zero = credit half zeroed at once; flip_abs = credit half abs-set at once.These are NOT commensurable with multcheck's per-key flip rates (one key ata time). 'touched' above IS per-key (fraction of decisions where that key'sdisplacement c=1.0 probe changed any candidate's score).

-- 3p -- (decisions 16002)
key                             champ_w      fire    p95_slope gated_frac          bound       touched      GATED?>
card_rate_credit                -5.9355         0     0.000000     0.0000      60.000000      0.000000           no
unit_strength_credit             0.0249         0     0.000000     0.0000      60.000000      0.000000           no
territory_credit                 0.0430      5389    13.357448     0.0000      32.029054      0.470816           no
bonus_card_credit                4.2786      5946     0.058798     0.0000      60.000000      0.563242           no
card_board_credit                0.3404      9544   257.969239     0.0719       1.658440      0.725597           no
tech_board_credit                0.0000     11909   145.757114     0.6283       2.935201      0.972753          YES
action_board_credit              0.1666     12673    60.270880     0.1163       7.098394      0.998563           no
gov_board_credit                 0.5677      6975    40.765199     0.1214      10.494894      0.593926           no
wonder_board_credit              0.1415         4     0.075683     0.7500      60.000000      0.000250          YES
tactic_board_credit              0.0219      1784     4.537967     0.0000      60.000000      0.120297           no
aggression_board_credit          0.0000      1950     7.967209     0.0000      53.698410      0.142607           no
war_board_credit                 0.0386      2519     0.401146     0.0000      60.000000      0.184602           no
pact_board_credit                0.0000      1591     1.955373     0.0000      60.000000      0.132608           no
event_board_credit               0.1049      7714     0.071633     0.0000      60.000000      0.660605           no
unit_tech_credit                 1.2215      4532    12.531155     0.1637      34.141021      0.355456           no
build_fresh_credit               0.0793      3066   163.578218     0.3213       2.615424      0.213411           no
restricted_resource_credit       0.0192      3825    12.011099     0.3603      35.619256      0.307587           no
free_action_credit               0.0779     11077     0.006957     0.0009      60.000000      0.991189           no
tactic_reach_credit              0.0747       876     0.211469     0.0000      60.000000      0.061742           no
card_board_leader               -0.0274      7017    75.852318     0.2049       5.640255      0.560930           no
CREDIT-CLASS flip rates (all 20 keys perturbed together):                                                                  0.180852     0.002750
  flip_zero = credit half zeroed at once; flip_abs = credit half abs-set at once.These are NOT commensurable with multcheck's per-key flip rates (one key ata time). 'touched' above IS per-key (fraction of decisions where that key'sdisplacement c=1.0 probe changed any candidate's score).

-- 4p -- (decisions 24433)
key                             champ_w      fire    p95_slope gated_frac          bound       touched      GATED?>
card_rate_credit                -0.5723         0     0.000000     0.0000      60.000000      0.000000           no
unit_strength_credit             0.0000     17230   503.905097     0.2134       0.826560      0.840871           no
territory_credit                 0.0000      7834    63.607113     0.0000       6.548132      0.366799           no
bonus_card_credit               -0.5476      7261     1.135809     0.0000      60.000000      0.347931           no
card_board_credit                0.1949     21652   555.766676     0.0068       0.749429      0.961527           no
tech_board_credit                0.0579     19783   340.149556     0.0050       1.224484      0.914092           no
action_board_credit              0.0595     19338     8.980789     0.0058      46.377636      0.965538           no
gov_board_credit                 0.0000     16518   175.810309     0.3078       2.369075      0.753284           no
wonder_board_credit              0.1674       899    35.990065     0.0000      11.572854      0.041788           no
tactic_board_credit              0.0940     12253   247.104292     0.0000       1.685555      0.545942           no
aggression_board_credit          0.1444      2969     2.273367     0.0000      60.000000      0.135022           no
war_board_credit                 1.3077      3215     3.999759     0.0000      60.000000      0.142676           no
pact_board_credit                0.0096      2187    26.975213     0.0000      15.440389      0.102975           no
event_board_credit               0.0228     14613     0.974300     0.0000      60.000000      0.683870           no
unit_tech_credit                 0.0000     17040   796.642824     0.4665       0.522829      0.839193           no
build_fresh_credit               0.0228     13082    26.668300     0.3382      15.618085      0.568084           no
restricted_resource_credit       0.0000      5812     3.456802     0.1681      60.000000      0.285434           no
free_action_credit               0.0000     18277     0.027570     0.0009      60.000000      0.933696           no
tactic_reach_credit              0.9598     11741    24.407310     0.0000      17.064878      0.531781           no
card_board_leader               -0.0975     11286   639.120835     0.0095       0.651689      0.488561           no
CREDIT-CLASS flip rates (all 20 keys perturbed together):                                                                  0.448492     0.038063
  flip_zero = credit half zeroed at once; flip_abs = credit half abs-set at once.These are NOT commensurable with multcheck's per-key flip rates (one key ata time). 'touched' above IS per-key (fraction of decisions where that key'sdisplacement c=1.0 probe changed any candidate's score).

================================================================================
T RE-MEASURED IN THIS RUN -- p50/p95/max of the TOTAL spread
max-min dot(champion, phi_c) over the candidate set, per decision (spec step 6)
================================================================================
count     n_decisions          p50          p95          max
2p              10153       76.153      214.093      542.885
3p              16002      135.231      427.826     1272.369
4p              24433      174.032      416.508     1334.743

================================================================================
GATED-FRACTION TABLE -- S_d(2.0) not ~2*S_d(1.0) at some firing decisions
(a gate threshold lies between w_k and w_k + 2.0, spec section 4.3,
displacement form). gated_frac is the HEADLINE: most keys are expected to
drop to a small gated_frac and carry a usable slope; a few keep a high one.
Those few are the genuine finding and the right move for them is still
CLAMP_BLIND -- they are named here with their gated_frac, on the record.
Keys gated at more than half their firing decisions (the GATED? column of
the key table) emit NO bound: the RUST TABLE row is 0.000000, so
clamp_bound's own <= 0.0 fallback applies.
Raw p95 readings over FIRING decisions, both probes:
================================================================================
key                             count      fire gated_frac p95 S_d(1.0)   p95 S_d(2.0)          bound (CLAMP_BLIND)
card_board_credit                  2p      8725     0.0638   440.293944     880.587888     0.486249
tech_board_credit                  2p      7927     0.1388   141.213174     282.415141     1.516096
action_board_credit                2p      8806     0.0945    30.951381      61.902652     6.917063
gov_board_credit                   2p      4927     0.4581   103.608714     240.838519     2.066358
wonder_board_credit                2p      3402     0.3918     5.008904      10.017142    42.742417
unit_tech_credit                   2p      6365     0.2564   157.532462     315.064923     1.359038
build_fresh_credit                 2p      6299     0.3181     5.088772      11.267301    42.071577
restricted_resource_credit         2p      3840     0.2812     1.222799       2.445598    60.000000
free_action_credit                 2p      8433     0.1418     1.358827       2.717638    60.000000
card_board_leader                  2p      5109     0.0986   444.221547     888.443093     0.481950
card_board_credit                  3p      9544     0.0719   257.969239     515.938479     1.658440
tech_board_credit                  3p     11909     0.6283   145.757114     173.736799     2.935201
action_board_credit                3p     12673     0.1163    60.270880     120.437806     7.098394
gov_board_credit                   3p      6975     0.1214    40.765199      81.530397    10.494894
wonder_board_credit                3p         4     0.7500     0.075683       0.122443    60.000000
unit_tech_credit                   3p      4532     0.1637    12.531155      25.062006    34.141021
build_fresh_credit                 3p      3066     0.3213   163.578218     327.059772     2.615424
restricted_resource_credit         3p      3825     0.3603    12.011099      24.022199    35.619256
free_action_credit                 3p     11077     0.0009     0.006957       0.013913    60.000000
card_board_leader                  3p      7017     0.2049    75.852318     152.223669     5.640255
unit_strength_credit               4p     17230     0.2134   503.905097    1007.273615     0.826560
card_board_credit                  4p     21652     0.0068   555.766676    1112.251672     0.749429
tech_board_credit                  4p     19783     0.0050   340.149556     680.299112     1.224484
action_board_credit                4p     19338     0.0058     8.980789      17.961578    46.377636
gov_board_credit                   4p     16518     0.3078   175.810309     389.350360     2.369075
unit_tech_credit                   4p     17040     0.4665   796.642824    1592.982408     0.522829
build_fresh_credit                 4p     13082     0.3382    26.668300      53.740912    15.618085
restricted_resource_credit         4p      5812     0.1681     3.456802       3.454225    60.000000
free_action_credit                 4p     18277     0.0009     0.027570       0.055139    60.000000
card_board_leader                  4p     11286     0.0095   639.120835    1278.241670     0.651689


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
    WeightKey::TerritoryCredit => [1.153157, 13.357448, 63.607113],
    WeightKey::BonusCardCredit => [0.323400, 0.058798, 1.135809],
    WeightKey::CardBoardCredit => [440.293944, 257.969239, 555.766676],
    WeightKey::TechBoardCredit => [141.213174, 0.000000, 340.149556],
    WeightKey::ActionBoardCredit => [30.951381, 60.270880, 8.980789],
    WeightKey::GovBoardCredit => [103.608714, 40.765199, 175.810309],
    WeightKey::WonderBoardCredit => [5.008904, 0.000000, 35.990065],
    WeightKey::TacticBoardCredit => [0.711767, 4.537967, 247.104292],
    WeightKey::AggressionBoardCredit => [0.376610, 7.967209, 2.273367],
    WeightKey::WarBoardCredit => [0.367623, 0.401146, 3.999759],
    WeightKey::PactBoardCredit => [0.000000, 1.955373, 26.975213],
    WeightKey::EventBoardCredit => [0.117639, 0.071633, 0.974300],
    WeightKey::UnitTechCredit => [157.532462, 12.531155, 796.642824],
    WeightKey::BuildFreshCredit => [5.088772, 163.578218, 26.668300],
    WeightKey::RestrictedResourceCredit => [1.222799, 12.011099, 3.456802],
    WeightKey::FreeActionCredit => [1.358827, 0.006957, 0.027570],
    WeightKey::TacticReachCredit => [0.781131, 0.211469, 24.407310],
    WeightKey::CardBoardLeader => [444.221547, 75.852318, 639.120835],
}
RUN_EXIT=0
