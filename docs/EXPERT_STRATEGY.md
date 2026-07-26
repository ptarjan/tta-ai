# Expert Strategy: Through the Ages (2015 "A New Story of Civilization"), BASE GAME

> **This document is EXPERT HUMAN CONSENSUS, gathered independently of our AI. It is a check ON our AI, not a product of it.** Nothing here was derived from our engine, our search, our training runs, or our self-play. It is what strong human players say about how to play well. Where it disagrees with our bots, the burden of proof is on the bots.

Compiled 2026-07-26 from BoardGameGeek, Steam, Board Game Arena, GameFAQs, Chinese/Taiwanese and Korean strategy wikis, and the local `sources/` corpus.

---

## Source-quality note (read first)

The highest-value sources, roughly in order:

1. **BGG "empirical tournament tier list"** — 39 games across 3 International Championships + 3 Intermezzo seasons, scored by *average civil actions spent per card per game*, i.e. revealed preference rather than opinion. https://boardgamegeek.com/thread/2494200
2. **BGG "what are the must get and never get cards?"** — contains posts from the **#1-ranked BGA player** and several other top-10/top-5 players, plus a 250-game BGA player. https://boardgamegeek.com/thread/2393942
3. **BGG ~100-game strategy guide** (3–4p focus). https://boardgamegeek.com/thread/2801950
4. **MasN's card-by-card ratings** on a calibrated 1–7 CA-value scale. https://boardgamegeek.com/thread/2258467
5. **30,000-game statistical analysis** of the digital edition. https://boardgamegeek.com/thread/1933554
6. **Translated Chinese guide** compiling the *hcy1* ("Pig's Life Log") and *akong740429* ("Empty Farm") blogs — the most detailed new-edition card-by-card analysis found. https://steamcommunity.com/sharedfiles/filedetails/?id=1367549747
7. Champion interviews: frotes (2× International Champion) and Palino (Intermezzo winner).

**Access caveats, because provenance matters here:**

- **boardgamegeek.com blocks WebFetch and plain curl (403).** Everything BGG-sourced in this document was retrieved via the `https://r.jina.ai/<url>` text proxy, which returns full thread text. The BGG XML API2 returned 401.
- **gamefaqs.gamespot.com is behind Cloudflare** and could not be fetched by any method tried (direct, jina proxy, allorigins proxy). GameFAQs (killswitch19) content here comes from **search-result snippets only** and is correspondingly lower-confidence. The local file `sources/gamefaqs_75690.txt` is just a Cloudflare interstitial, not content.
- **reddit.com is blocked to this crawler.** Domain-restricted WebSearch against reddit.com returns a hard API error. A background research agent *did* reach **old.reddit.com** HTML via curl; all Reddit-sourced material is confined to **Appendix B** and marked as such. Nothing in the main body is Reddit-sourced.
- The **throughtheages.fandom.com MediaWiki API** works over plain curl and its category listings are **base-game-only**, which is what let a second agent independently confirm the base card sets.

**Edition caveat — the single biggest hazard in this literature.** Almost every published tier list is expansion-era (New Leaders & Wonders) or digital-edition. The expansion adds **6 leaders and 4 wonders per age**. Rankings must be filtered before use. See the next section.

---

## Base-game filter (load-bearing — we are locked to the 2015 base game)

Authoritative source: `/Users/pt/tta-ai/sources/bga_card_counts.tsv` (extracted from the Board Game Arena implementation). Independently confirmed via the Fandom MediaWiki category API (`Category:Age_A_Wonders`, `Category:Age_I_Wonders`, `Category:Age_II_Wonders`, `Category:Age_III_Wonders`, and the Leaders categories) and by two pre-expansion sources that each enumerate exactly four wonders per age.

**Base game = 6 leaders per age, 4 wonders per age.**

| Age | Leaders (base) | Wonders (base) |
|---|---|---|
| A | Alexander the Great, Aristotle, Hammurabi, Homer, Julius Caesar, Moses | Colossus, Hanging Gardens, Library of Alexandria, Pyramids |
| I | Christopher Columbus, Frederick Barbarossa, Genghis Khan, Joan of Arc, Leonardo da Vinci, Michelangelo | Great Wall, St. Peter's Basilica, Taj Mahal, Universitas Carolina |
| II | J.S. Bach, Isaac Newton, James Cook, Maximilien Robespierre, Napoleon Bonaparte, William Shakespeare | Eiffel Tower, Kremlin, Ocean Liner Service, Transcontinental Railroad |
| III | Albert Einstein, Bill Gates, Charlie Chaplin, Mahatma Gandhi, Sid Meier, Winston Churchill | Fast Food Chains, First Space Flight, Hollywood, Internet |

### Explicit expansion-exclusion list — DO NOT CODE THESE

**Leaders:** Cleopatra, Boudica, Hippocrates, Confucius, Ashoka, Sun Tzu, Jan Žižka, Isabella of Castile, Nostradamus, Johannes Gutenberg, Saladin, Eleanor of Aquitaine, Alfred Nobel, Catherine the Great, James Watt, Maria Theresa, Charles Darwin, Antoni Gaudí, Marlene Dietrich, Marie Curie Skłodowska, Steve Jobs, Pierre de Coubertin, Ian Fleming, Nelson Mandela.

**Wonders:** Roman Roads, Stonehenge, Colosseum, Acropolis, Machu Picchu, Forbidden City, Himeji Castle, Silk Road, Suez Canal, Statue of Liberty, Harvard College, Louvre Museum, United Nations, International Red Cross, Manhattan Project, Empire State Building.

**Two specific traps this list defuses:**

1. A GameFAQs-derived claim that Age A has **11** leaders (Hammurabi, Hippocrates, Confucius, Aristotle, Cleopatra, Ashoka, Homer, Alexander, Julius Caesar, Moses, Sun Tzu) is **wrong for our purposes** — that is the digital edition *with* expansion. `bga_card_counts.tsv` and the Fandom categories both give **6**. Resolved in favour of 6.
2. Colossus defences that cite "3 free military card draws at the start of Ages II and III" are quoting the **expansion/digital buff**. Pure base-game Colossus is `+2 strength, +1 colonization bonus` only, and is therefore weaker than those defences assume.

---

## 1. Opening (Age A / Age I, turns 1–4)

**Target board state by end of turn 3: 2 Agriculture / 3 Bronze / 2 Philosophy (+ starting Warriors) = 8 workers.** This is the strongest consensus in the whole dataset — independent sources give the identical split:

- "On turn 3 you then send a worker to philosophy or bronze to end up with **2 workers at agriculture, 3 workers at bronze and 2 workers at philosophy**. If you send more workers to either bronze or agriculture you will have too few workers for other things like the army and you also risk getting corruption. **Bronze needs more workers because stone is much more valuable than food in the first age.**" — https://boardgamegeek.com/thread/2801950
- "**Two farms, three mines, two labs** (unless going for a military win)" — https://steamcommunity.com/app/758370/discussions/0/1696043263506122183/

**Turn 1 (P1 has only 1 CA): build the third Bronze mine, or take a leader.** Near-universal.

- "Build a bronze mine as your first action in the game. Understand why everyone does this… on turn 1, each player can build exactly one infrastructure improvement" — https://boardgamegeek.com/thread/2695320
- "in base game it's really hard to justify not having 3 mines very early" — https://boardgamegeek.com/thread/2569870
- 30k-game data: "Go for **Mine or Lab in Round 1**… They are both better than all other choices, such as the 3rd Farm, or directly working on a Wonder." — https://boardgamegeek.com/thread/1933554
- Exception: if you drew **Urban Growth A**, mine→lab vs lab→mine is roughly a wash.
- Contrarian: "first turn should typically involve **+1 mine and +1 population**" — https://steamcommunity.com/app/758370/discussions/0/1696043263498632583/

**Bronze vs Philosophy: Philosophy first if affordable, else Bronze.** "1) send a worker to **philosophy if you can, otherwise to bronze**" (BGG 2801950). Note Philosophy costs 3 res vs Bronze 2 res (`bga_card_counts.tsv`), so on turn 2 with 2 stored + 2/turn you often can't. The full sequencing consensus is **mine (T1) → 2nd lab (T2/T3) → finish wonder (T3–T5)**: "turn 2 mine, turn 3 philosophy, turn 5 wonder is preferable to either turn 2 mine turn 4 wonder or no 3rd mine and turn 3 wonder" — https://boardgamegeek.com/thread/2258467

**Canonical turn-2 script (4 CA):**

1. Worker → Philosophy (else Bronze)
2. Increase population
3. Play out your leader
4. Take a yellow card — *"don't take a yellow card and take a leader/wonder instead if you haven't got one of them"*

— https://boardgamegeek.com/thread/2801950. Corroborated: "On my first real turn I build a mine and increase population **about 90% of the time**. The other two actions usually go towards taking a good card that's on 2 CA, or taking a 1 CA card and electing a leader." — https://boardgamegeek.com/thread/2569870

**Leader on turn 1: YES, but don't overpay for tempo you can get next turn.**

- "I suggest taking a **leader and a wonder** when the game starts, leaders bring in a lot of value despite being cost free (they only cost civil actions), so **try to always have a leader present**." — https://boardgamegeek.com/thread/2801950
- "each round without a leader is significant negative utility. Only in **one game out of 39** did I skip an age A leader" (tournament data) — https://boardgamegeek.com/thread/2494200
- "**Use Leaders. This is the single most important thing.** You should never be too busy doing 'other things' and forget to get a leader of the current age." — https://boardgamegeek.com/thread/1933554
- "I often see players take Hammurabi for 2 CA when they could also get him next turn for 1, **don't do that**." — https://boardgamegeek.com/thread/2166558
- "It rarely makes sense to skip the Age A leader, but sometimes it makes sense to leave the leader for the second turn if it looks like one will be available on the card row for the second turn." — https://boardgamegeek.com/thread/2569870
- BGA tips agree: "Secure **one Leader per Age** when possible" — https://en.doc.boardgamearena.com/Tips_throughtheagesnewstory
- *Dissent:* the *hcy1* blog says with a bad seat you may take a yellow card first and react to opponents: "if the early players are afraid that the choice of the lower hand will give pressure…the first round can also take the yellow card first, and then wait and see the opponent's choice" — via https://steamcommunity.com/sharedfiles/filedetails/?id=1367549747

**Keep 1 idle worker through Age A — contested.** Age A events `Development of Religion` (free temple) and `Development of Warfare` (free warrior) only fire for a player with an unused worker (`bga_card_counts.tsv` rows 13, 17).

- Pro: "while Age A events are waiting to come out, it is **ideal to have an idle worker at all times**" — https://steamcommunity.com/app/758370/discussions/0/1696043263498632583/; "Maintain a spare worker that is unassigned… potentially gaining **2-3 free iron AND a free CA**" — https://steamcommunity.com/app/758370/discussions/0/1696043263506122183/
- Pro, from CGE's own David Jablonovsky: "in Age A, you usually want to have a free worker ready at the end of your turn" — https://boardgamegeek.com/thread/2695320
- Con: "Generally I'd rather take a guaranteed 1 food than ~1/10th chance of a free temple." That analysis ranks three options — (A) 2nd lab, no float; (B) build wonder stage, float; (C) 2nd lab + float via early consumption — and calls **C the worst**, with A and B close. — https://boardgamegeek.com/thread/2166558

**Second population increase timing: when you hold 4 or 6 food, i.e. turn 4 or 5.** "Once you increase population for the second time you start producing 1 food instead of 2, usually I do that when I have **4 or 6 food in total (in the 4th or 5th turn)**." — https://boardgamegeek.com/thread/2801950

**Rich Land A vs Urban Growth A** — argued for Rich Land, contrary to the popular Urban Growth pick: "Early on rocks are more often a limiting factor… almost all the things you want to make early come in increments of **3 rocks**: Iron/Alchemy upgrade, wonder stage, military unit, printing press. Most other rock sources come in size **2**." Also: "taking both Rich Land A and Urban Growth A is usually too much." — https://boardgamegeek.com/thread/2166558

**Turn-3 two-turn wonder trick:** take an Age A wonder + Engineering Genius on turns 1–2, spend nothing, then on turn 2/3 you have 4 resources banked + EG → finish a 3-stage wonder immediately. "if you complete [Pyramids] in 2 turns, you only get about **15 citizen tokens** throughout the game. This means you gain about **three turns**!" — `sources/namu_wonders.txt` line 30.
**Counter-rule from the #1 BGA player:** "**don't skip your 3rd mine for the turn 3 wonder.** Try to roll the event deck for the missing rock. If you miss, just build a lab instead." — https://boardgamegeek.com/thread/2393942
**Counter-rule from the 30k-game data:** "maybe we should stop rushing to complete Pyramids… it's not worth delaying the extra Science production" — https://boardgamegeek.com/thread/1933554. But another strong player: "I do tend to rush LoA… Colossus doesn't need to be rushed, nor does HG" — https://boardgamegeek.com/thread/2569870.

**Seat preference: 2nd or 3rd.** "I recommend being **second or third**… The first player…doesn't have nearly as many options as other players in the first turn due to only having 1 civil action… As for the 4th player, he has a lower chance of taking good cards." — https://boardgamegeek.com/thread/2801950
*Counter-view:* the **last** player is strongest because they control when the game ends (cited as ~5% higher win rate in 2p) — https://boardgamegeek.com/thread/2732988

---

## 2. Leader rankings (base game only)

### Age A

| Tier | Leader | Why (cited) |
|---|---|---|
| **A** | **Hammurabi** | "Hammurabi's extra action is **excellent**, comparable to having the Pyramids without having the Pyramids, though you sacrifice a Military Action" (killswitch19 ★★★, via search of https://gamefaqs.gamespot.com/pc/234527-through-the-ages/faqs/75690/leaders). "Hammurabi…is a **fast leader**, he allows you to spend as many resources as you want and never have corruption" (BGG 2801950). "for 2p I would pick Hammurabi, **an extra CA worths so much than any others**" — https://boardgamegeek.com/thread/1761996. MasN: **6\*** "best… and it's not even close" (BGG 2258467). Tournament: **highest of the age at 0.36** (BGG 2494200). |
| **A** | **Aristotle** | "**Aristotle wins if 2 or more are available at the same CA cost**" (BGG 1761996 — two separate posters name him best Age A). Expected value **+4 science** (killswitch19); "2~5 brains are probably reasonable values" (*hcy1* via Steam guide); "approximately 5 brain equivalent" (https://hcy1.blogspot.com/2009/06/through-ages.html). Caveat: "Aristotle is special in the sense that you have to take **technology cards**, but it will sometimes cost you 2-3 civil actions, so you can't afford taking yellow cards as well" (BGG 2801950). MasN: 4\* "second best, gap to first very noticeable." |
| **B** | **Homer** | The biggest disagreement in the data. BGG guide: B tier / slow leader. killswitch19: **★★★**. Tournament: **Tier 1 (0.28)**. BGG 1761996: "**A smiley face all game is worth more than any short-term resource.** I am finding on BGO among the **stronger players**, I am finding more and more players who believe this." Rebuttal in the same thread: "Moses/Aristotle/Hammurabi/Caesar each give **3-5+** of a resource… Homer gives typically **1-2 rock**, plus saves you **3 rock and a yellow token** for the temple. So he is basically a yellow token better." Unique upside: "The advantage of Homer is that it **does not need to be played immediately**" (*hcy1*). Needs a completed wonder, so bad with Hanging Gardens, mediocre with Colossus. |
| **B** | **Alexander** | "+1 strength per unit"; "**Yellow token > Happiness**" (BGG 1761996). Tournament Tier 1 (0.23). Value is *cash-in* value; both he and Homer "cost an **extra CA overall** because you don't get the CA back when replacing them" (BGG 2258467, 2494200). Downside: "no way to trigger the exchange of leaders and white spots" (*hcy1*). |
| **C** | **Julius Caesar** | Strongest disagreement after Homer. BGG guide C tier; killswitch19 **★**; MasN 2\*; tournament Tier 3 (0.03). "**In the old TTA, sure, but in the new TTA he is terrible. MA in Age I are worth little and there are better ways to get them.**" — https://boardgamegeek.com/thread/1761996. 30k-game data with skill-filtering: "**Caesar becomes the worst Age A leader**" among good players — https://boardgamegeek.com/thread/1933554. *But* *hcy1* rates him well: "3 red points is very defensive… Caesar and Colossus are a very good match… even if no aggression, in event control or colonial bidding can also get an advantage." Code as **situational-military**, not a default. |
| **C** | **Moses** | Weakest by consensus (tournament Tier 3, 0.03; MasN 2\*; killswitch19 ★). "His effect is not that helpful because producing too much food can be problematic, with the problem of **idle workers and corruption** being frequent." Backloaded, and forces CA into growth actions to dodge corruption — "negative tempo" (BGG 2494200). |

**Calibration point (important for the engine):** Hammurabi, the best Age A leader at 0.36 tournament CA-spend, scores **below the bottom third of Age I leaders**. Age A leaders are a bridge, not a win condition. — https://boardgamegeek.com/thread/2494200

**Head-to-head rule of thumb:** "Hammurabi is a slight bit more than **1 CA better than Aristotle** generally" — https://boardgamegeek.com/thread/2166558

**Universal caveat (code this):** "all **24 leaders remain viable; no 'bad' leaders exist**. **No leader warrants spending 3 white points (except Age 3)**; 2 white points: occasionally necessary." — https://hcy1.blogspot.com/2017/03/tta_31.html. And: "Leaders in the A period were all available… **the gap between strengths and weaknesses was small**" (*hcy1*).

### Age I (base six)

Tournament CA-spend ordering (base cards only): **Joan (0.33) > Barbarossa (0.15) > Genghis (0.08) > Leonardo (0.05) > Columbus (0.03) > Michelangelo (0.00)** — https://boardgamegeek.com/thread/2494200. MasN's opinion ordering differs: **Leonardo 6\* > Columbus 5\* > Joan / Genghis / Barbarossa 4\* > Michelangelo 2\*** — https://boardgamegeek.com/thread/2258467.

1. **Joan of Arc** — +1 MA, +1 culture, +1 strength per Temple/government happy face, peeks at the top of the event deck. "**Provides the third MA, which combined with a source for the 5th CA, allows comfortably skipping an age 1 government.**" Caveat: "I find temples to be the weakest urban buildings, so often provides only a single strength." Expect a **~5 strength cliff** when she leaves. Combo: Joan + 2× Theology = 4 happy faces + 2 culture + 4 strength (*hcy1*). "Joan, quite surprisingly. She is a versatile leader and the MA provided is always useful" — https://boardgamegeek.com/thread/1761996
2. **Christopher Columbus** — **only take with a colony already in hand.** "Columbus can be **gamebreaking with a good colony**" (BGG 2801950). "**Do not take speculatively, and not without at least 3 MA**" (BGG 2494200). Valuation model: `ColumbusValue = TerritoryI + (TerritoryII − TerritoryI) × P(TerritoryII)`. Best with **Vast Territory I** (then he's the best Age I leader); Inhabited I "noticeably worse but still good"; Historic I and Wealthy I are "garbage." Optimal timing: discover a colony right at the end of Age I. — https://boardgamegeek.com/thread/2258467, https://boardgamegeek.com/thread/3454327
3. **Frederick Barbarossa** — each activation converts an MA into a CA + 1 food + 1 rock, unlimited per turn. Needs (1) Irrigation, (2) a 3rd MA, (3) a happiness solution. "Personal experience is almost equal to the ability to use Baba **3 times. 3 White should be a very good leader**" (*hcy1*). Great for dodging corruption during a revolution and for colonising. Anti-synergy with Frugality. — https://boardgamegeek.com/thread/2258467, https://boardgamegeek.com/thread/2494200
4. **Genghis Khan** — value is **tempo**, not raw strength: "**Does not actually add strength, only lowers the requirements of tactics.**" "the advantage of sweating lies in **delaying the research and development of the cavalry**… the R&D Cavalry's **5 brains** can be used to do other things (iron, alchemy, government, irrigation)" (*hcy1*). Only a confident pick **with Heavy Cavalry**; Phalanx is too easily copied. Hard to transition out of. — https://boardgamegeek.com/thread/2494200
5. **Leonardo da Vinci** — hard prerequisite: **Alchemy or Printing Press**. "Don't take him without either Alchemy or Printing Press" (BGG 3454327). "One of the most powerful age 1 economy leaders, and enters age 2 with an abundance of science" (BGG 2494200). "I certainly **don't want to pay 2 or 3 CA for him** very often" (BGG 2393942). **Rule: take at 1 CA, sometimes 2; require an Age I lab/library in play or in hand.**
6. **Michelangelo** — **last in every list found.** "I **don't like Michelangelo**" (BGG 1761996). "**Michelangelo is bad in BGA meta. I'd be surprised if it was picked in even 5% of games**" (#1 BGA player, BGG 2393942). "in four player, you are **either going to win with him or finish fourth**… **without HG or St. Pete's I usually pass**" (BGG 2393942). Called a "**noob trap**": "invites novice players to over-invest… encourages wasting resources on bad wonders and **inflating the price in actions of Age 3 wonders**." The mechanical statement: "CA provided is **geometric** with respect to the number of wonders picked; **amazing value if many (4+) wonders are picked**, otherwise unimpressive" (BGG 2494200). **Rule: only take Michelangelo if (Hanging Gardens or St. Peter's is yours) AND (Iron, ideally + Masonry).**

**Best base-game early culture engines: Genghis + Great Wall, or Joan + St. Peter's Basilica** — "These make decent culture per turn while also presenting a strong military threat" — https://boardgamegeek.com/thread/2425822

### Age II (base six)

Tournament tiers: **Tier 1: Newton, Napoleon. Tier 2: Cook, Bach, Shakespeare. Tier 3: Robespierre.** — https://boardgamegeek.com/thread/2494200. MasN: **Napoleon 9\*** (off-scale on a 1–7 scale), Newton 5\*, Robespierre 5\*, Bach 4\*, Cook 4\*, **Shakespeare 2\*** — https://boardgamegeek.com/thread/2258467.

1. **Napoleon** — "**Most important card in the game**" (#1 BGA player, BGG 2393942). "This almost always goes to the person lucky enough to find it for 3 CA… **the most powerful card in this version of the game**" (MasN). "Napoleon has the **biggest potential because he can increase your strength by 6 or even more**" (BGG 2801950). Statistically the best Age II leader in 30k games. *Dissent:* "All that Napoleon love. He's **4–6 strength and some MA**. Autopick? I think not." — https://boardgamegeek.com/thread/1761996. Post-nerf note: Napoleon dropped from 2 MA to 1 MA in this edition. **Rule: worth 2–3 CA; also worth hate-drafting.**
2. **Isaac Newton** — "Probably Newton" as best Age II (BGG 1761996). "**Newton can be used to go through a military revolution and still leave yourself with one civil action**" (Steam guide) — the key revolution combo.
3. **William Shakespeare** — high ceiling, needs setup. "useless if you don't have either **Opera or Journalism**… in all the other cases he can save you up to **14 stones, 6 science** and increase the culture production by up to **8 culture per round**" (BGG 2801950). "will very often give you **4-6 culture**" (BGG 1761996). MasN 2\*: "the stars will not line up perfectly"; needs Printing Press + 2 workers into Theaters + no military pressure.
4. **James Cook** — "as long as **2 colonies**, I will consider using Cook to earn **3 points per round**. A score of 3 equals an opera" (*hcy1*).
5. **J.S. Bach** — "**something of a consensus that he was the weakest of the Age II leaders**" (BGG 2393942). "Bach and Shakespeare the worst" (BGG 1761996). Also flagged as a noob trap. *Dissent (2p):* "**Bach is Tier 1; Bach and Shakespeare are even worth 3 civil actions**."
6. **Maximilien Robespierre** — last in most lists. Narrow use: "The most common with the **republican system**… only **3 brains change the regime, 7 white 3 red**" (*hcy1*). Value ≈ "**10 brains**" saved.

### Age III (base six)

**Bill Gates** (best; "**computer + Gates completed earlier is the strongest combination of the 3 phases**… Gates is usually worth **3CA**" — *hcy1*; Tier 1 in tournament data, needs Computers) ≈ **Sid Meier** ("**Easy 8-9 [culture] per turn**" — BGG 1761996; "+**12 culture/turn** with 4 Computers" — http://blog.lightningshroud.com/2018/01/through-ages.html; ~9 culture with Computers, ~6 with Scientific Method only) ≈ **Churchill** ("**Churchill is the versatile choice**"; "**3 brains and 3 mines are worth a lot more than 3 points**" — *hcy1*) > **Einstein** ("the most general one… all-round") > **Gandhi** (specialist anti-war; "I **almost never pick Gandhi**" ×2 in BGG 1761996; "**Don't count on Gandhi**" — 30k data) > **Chaplin** ("the one I think is **very rarely worth picking**" — BGG 2393942).

---

## 3. Farms vs mines, food/resources, worker counts

**Ratio: mines > farms, 3:2 in Age I.** "Bronze needs more workers because **stone is much more valuable than food** in the first age" (BGG 2801950). "**Value resources over food. You can build a new farm with resources, but you can't build a new mine with food**" — https://en.doc.boardgamearena.com/Tips_throughtheagesnewstory. "In the first couple of turns of the game, 1 extra rock is much more likely to be relevant than 1 extra science."

**Food is a low priority.** "Food is a **low priority** for the most part, compared to basically everything else in the game. **Only invest in food if you have an excess of happiness**" (Steam "Any tips?"). "**Additional food is not needed in the first age and leads to corruption**" (BGG 2801950). "**Don't take Frugality**" (gain 1 food after increasing pop) — BGG 2801950.
*Contrarian, worth noting:* "It is still not profitable to build a third farm on the first turn, but it is already profitable in Age 1… I now play 3 farms much more often if there is no early irrigation." — https://boardgamegeek.com/thread/2957812

**Rock production benchmarks (per turn)** — https://boardgamegeek.com/thread/2097526:

| Point in game | Target |
|---|---|
| End of Age I | 3 is acceptable **if** you hold Iron or Leonardo |
| End of Age II | "at least +5 (Iron) or better" |
| Age III | "+6 or better is optimal" |

Key breakpoints: "**+3/turn and +5/turn** are important production numbers, because they represent being able to build an Urban Building/Knight/Swordsman every turn, or an Age II Military every turn." — same thread

**Farm targets:** 2 farms upgraded to Irrigation = **4 food/turn**, "should be enough for your civilization **through Age II**" (killswitch19 via search). "Begin with 2 agriculture workers. Target: upgrade to **2 irrigation farms (4 food/turn)**. Build additional farms rather than upgrading if actions are limited" — https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/. "the farms are practically unskippable. **95% of the time** you'll want either Irrigation or the Age II counterpart (Selective Breeding)" (BGG 2258467). **Selective Breeding is "always worth 3 civil actions"** (BGG 2801950) — but only if you skipped Irrigation. Irrigation is much more valuable in 2p because there's only **one** Selective Breeding. "Irrigation is a tech predestined to be developed **at the end of Age I**, not significantly earlier or later" (rushing it causes corruption + happiness trouble) — https://boardgamegeek.com/thread/2724657

**Mine targets — genuine disagreement, player-count dependent:**

- 3–4p / BGG 2801950: goal #2 of Age I is "**Upgrading 3 bronze mines to iron mines**"; "**Iron is often worth 3 civil actions**"; "**You can't win by producing 3 stones per round**, so get Coal as soon as you can if you haven't got Iron."
- 2p / #1 BGA player: "**there is no need to get a mine tech. In most cases if you miss Iron you complete the game on bronze.** I definitely **prioritize Alchemy over Iron**." Another top player: "~**2/3 of the games I played, I did not upgrade from 3 Bronze**… the **5th CA is critical** to allow you to grab yellows, and as such **Code of Laws is an MVP tech**." — https://boardgamegeek.com/thread/2393942
- "a **top 5 player**… said very often in **top 10 players matches they don't improve either mines or farms**. They take advantage of the high number of yellows that provide resources. Also focus primarily on military and tech." — https://boardgamegeek.com/thread/2393942
- "I have won games against high-level players with **only 3 bronze all game**. Lots of CAs to snag lots of yellow cards is the key." — https://boardgamegeek.com/thread/2097526
- 4p counter: "in four player at least **not taking Iron creates a big risk** where Coal is hate drafted or comes out very late." — https://boardgamegeek.com/thread/2393942
- Two strong players, both >60% winrate, taking opposite sides: "Iron is one of the best techs in the game" vs "I'm happy to end the game with only Bronze" — https://boardgamegeek.com/thread/2233796
- **Iron total cost: 5 CA, 5 science, 9 rocks; payback on a Bronze→Iron upgrade is 3 turns.** — https://boardgamegeek.com/thread/2258467, https://boardgamegeek.com/thread/2097526
- Codable "take Iron" triggers: you did NOT get Code of Laws / Hammurabi / Monarchy / Pyramids; you have Aristotle or early Leonardo+Alchemy; 2p and opponent took Iron for 2 CA; early Michelangelo + wonder plan; Iron is at 1 CA early in Age I; your Bronze is stacking blue cubes you can't spend. — https://boardgamegeek.com/thread/2233796
- "Iron's **5 cost** is juuuuust high enough that I don't like to sit on it. So I'll take Iron if I see a plan to get the upgrades done in the next couple of turns, but **don't take it speculatively**."
- **Oil is universally rejected:** "the worst CP in the game. Not recommended" (akong via Steam guide); "**Comes too late**… you pay 3 rocks to get +2 rocks/turn… **you want to convert your rocks to culture, not more rock production**" (BGG 2393942).
- **Don't take both Iron and Coal**, or both Age I and Age II of the same track: "I seldom if ever do Age I and Age II of the same tech, nor Age II and Age III of the same tech." — https://boardgamegeek.com/thread/2892591

**Codable mine rule:** upgrade to Iron only if it lands in Age I early **and** you can finish the upgrades within ~2 turns using Rich Land / resource events; otherwise stay on 3–4 Bronze and buy the 5th CA (Code of Laws / Pyramids / Hammurabi) plus yellow resource cards instead.

**Hard rule with a direct counter-claim:** "**never upgrade a production track at the very end of that track's age**" ("If Iron comes out at the end of Age I, I'm not going to upgrade into it. Same for Irrigation and Alchemy") — https://boardgamegeek.com/thread/2097526. Counter: "**Taking Iron at the end of Age I is often great**" — https://boardgamegeek.com/thread/2724657

**Corruption hard threshold (from rules, directly codable):** you have **16 blue tokens**; "**You face no corruption if you have at least 11 blue tokens in your blue bank**" — `sources/ubg_subsequent-rounds.txt:197`. So: **end-of-turn (stored food + stored resources + blue cubes locked on unfinished wonder stages) must be ≤ 5.** Corruption bands are **−2 / −4 / −6** (https://statelyplay.com/2017/09/26/strategy-101-through-the-ages-corruption-edition/). Target: "**Avoid more than 1-2 corruption events per game** — excess production indicates poor planning" (Steam "Any tips?").

**Age I → II food transition rule:** "If you will produce **0 food** after the first age you need to have **exactly 0 food** at the start of the second age to avoid future corruption… Do your best to avoid a situation where age II begins, you produce 2 or 3 food and can't increase population because it **costs 4 food**, but you also have no happy faces, so you have to destroy a building." — https://boardgamegeek.com/thread/2801950

**Science production scale (MasN, per turn at end of Age I)** — https://boardgamegeek.com/thread/2258467:

| Science/turn | Verdict |
|---|---|
| 1 | "a way to throw the game" |
| 2 | struggle but survivable |
| 3 | not ideal, no panic |
| **4** | **"a good amount"** |
| 5 | a lot — buy Code of Laws |
| 6 | diminishing returns; "unless your 6 science is from LoA + 2 Alchemy + Da Vinci, you probably made a mistake" |
| 7 | over-invested |

Tournament player: "I generally am happy going into age 2 with **3** science income" — https://boardgamegeek.com/thread/2494200. Age I budget: "you usually have enough science for **3 things**: (1) Swords or Horses, (2) Alchemy or Printing Press, (3) Code of Laws or Iron" — https://boardgamegeek.com/thread/2233796

**Worker counts:**

- Start: **7 workers** (6 placed: 2 Agriculture / 2 Bronze / 1 Philosophy / 1 Warriors; 1 in pool), **18** in the yellow bank, **16** blue tokens — `sources/ubg_player-areas.txt:40-45`.
- End of Age A / turn 3–4: **8** (2/3/2 + warrior), keep **1 idle**.
- No source publishes an end-of-Age-II/III worker benchmark. The nearest hard number is the Age III `Impact of Population` event: **2 culture per content worker beyond 10** (`sources/namu_events.txt`), implying **>10 workers is "large"** by end of game.
- Ocean Liner Service short-circuits this entirely: "free worker for no Actions or food," "**Agriculture almost does not need development**" (*hcy1*; statelyplay).

**Yellow-cube economics (the closest thing to a worker-count model found):** every 4 yellow cubes reduces growth cost and consumption by 1; every 2 reduces required happiness by 1. A civ with 6 extra yellow cubes "requires 3 less happiness and potentially 4 less food income per turn to sustain 1 growth/turn." There is an explicit expert disagreement in that thread on whether yellow cubes have *increasing* or *diminishing* returns. Both sides agree: "**the first yellow cube in a bin is worth the most; the second is worth nothing**." — https://boardgamegeek.com/thread/2494200

---

## 4. Happiness

**Mechanic (codable):** discontent workers = number of emptied yellow-bank subsections to the left of your happiness marker. **If discontent workers > unused workers at end of turn → uprising → skip your entire Production Phase.** A single discontent worker is not itself a penalty. First trigger: "**After you increase your population for the second time, subsection 1 of your yellow bank is empty. You need at least 1 happy face**" — `sources/ubg_subsequent-rounds.txt:164`. **Happiness track caps at 8** — https://boardgamegeek.com/thread/1064481. Unpaid food costs **4 culture per unit** (`sources/hypercheat.txt`).

**Buffer rule: 0 in Age I, then 2 happy faces for the Age I→II transition.** Each age change costs you **2 yellow tokens** — that is the trap.

- "**You don't need happy faces and culture in the first age**" (BGG 2801950). "In age I I **rarely have problems with happiness**" (BGG 2393942).
- "Always watch out when reaching a new Age… most importantly you lose **2 yellow tokens**, which means you need to plan ahead to avoid an uprising. **Ignoring your total happiness is very common and sometimes instant game losing.**" — https://boardgamegeek.com/thread/2695320
- Age I checklist item: "Get **at least two happiness** for the age 2 transition." "The transition to age 2 can be jarring, usually requiring 2 happiness." — https://boardgamegeek.com/thread/2494200
- Then: "Avoid having **too many happy faces** and spending so many yellow cubes for urban buildings that **you don't have enough for the army**" (BGG 2801950). "Most of the time I have exactly as much happiness as I need right now."

**Practical buffer = +1 happy face ahead of your next planned population increase; never bank more than that.** Ceiling: 6 happiness is usually enough, max useful is 8 (https://boardgamegeek.com/thread/1039749).

**Strongly-held consensus: buy happiness from leaders/wonders, not from tech.**

- "Cards providing happiness are especially highly valued, as they enable **skipping Theology and Bread and Circuses**; the age-1 buildings free up a **single** worker while their age-2 equivalents free up **two**." And the headline stat: "**I found that in 39 games I have selected Theology exactly 0 times**" — https://boardgamegeek.com/thread/2494200
- "**Theology 3\* / Bread & Circuses 2\***: These two things are not something you get so much because you want to, but because Age I just ended and you'll face rebellion if you don't destroy something." — https://boardgamegeek.com/thread/2258467
- "Generally you want to **avoid building Happiness buildings until later in Age I**." — https://boardgamegeek.com/thread/1039749
- Ordering: Hanging Gardens / St. Peter's / Great Wall / Homer / Joan first → skip Age I Religion tech → take Age II happiness (Organized Religion / Team Sports / Opera) only as needed.

**Which happiness building (base costs from `bga_card_counts.tsv`):**

- **Bread & Circuses (Arena, Age I): 3 science / 3 resources → +2 happy, +1 strength.** Best resources-per-happy in Age I. "**Bread and Circuses is a preference over building Religion/Theology in Age I**, taking care of Happiness problems likely **until Age III**" (killswitch19 via search). *Dissent:* one of BGG 2393942's four worst techs — "+1 str is just not that impactful."
- **Theology (Temple, Age I): 2 science / 5 resources → +2 happy, +1 culture.** Cheapest *science* route. "Joan and the first phase of theology (**only 2 brains**) match… **cover two theology, four smiling faces can be used for a long period**" (*hcy1*).
- **Temple vs Arena is roughly a wash.** Temples marginally better because you may already own the free Age A Temple to upgrade; Arenas are cheaper, give strength, give more happiness per building, and score Impact of Competition. "You can achieve **6 happiness with Despotism and 8 happiness with other governments with just Bread & Circuses**." Explicit: **never build both temples and arenas.** — https://boardgamegeek.com/thread/1039749
- **Theaters are for culture, not happiness.** "Theaters are built for culture, not Happiness, as there are better options" (killswitch19). "It is more difficult to get libraries and theatres into play, as there's rarely the population to spare." Libraries are built *instead of* labs. Drama 1\*, Printing Press 2\*, Opera 3\* (MasN).
- **Skip happiness buildings entirely with Hanging Gardens or St. Peter's.** "**As long as you have St. Peter's Basilica, you can live the entire game without worrying about your happiness**" — `sources/namu_wonders.txt:193`. "Two temples in combination with Basilica produce **5 happy faces**" (BGG 2494200).

**Cost of unhappiness:**

- Uprising = lose your whole Production Phase (rules).
- Rebellion event "removes **two civil actions for each unhappy worker**" (BGG 2801950).
- Age II event `Civil Unrest`: **−4 culture per discontent worker**, plus the worst offender loses a blue token — `sources/namu_events.txt`.
- Age III `Impact of Happiness`: **+2 culture per happy face (cap 16) and −2 per discontent worker** — same file; https://boardgamegeek.com/thread/1064481.
- Tournament advice: "be very wary of seeding with **2 unhappy workers**" (BGG 2494200).
- **Kremlin's −1 happiness** and **Communism's −1** are the classic traps.
- **Ravages of Time interaction:** keep a happiness *backup plan* if your happiness sits on a wonder. Pain-of-loss ranking (descending): Hanging Gardens (−2 happy) > Colosseum > Stonehenge > Roman Roads > Library of Alexandria > Pyramids > Acropolis > Colossus — https://boardgamegeek.com/thread/2494200 (expansion wonders listed for completeness; base-relevant ordering is HG > LoA > Pyramids > Colossus).
- Emergency fix = decommission a Philosophy or a Warrior (BGG 2892591).

---

## 5. Military

**The mantra, repeated near-verbatim across sources:** "You don't necessarily win with good military, but **you will almost certainly lose if you ignore it**." — https://boardgamegeek.com/thread/2424523. "Having Military won't always win you the game, but not having it will always lose it." — https://boardgamegeek.com/thread/2597183. From 2× International Champion frotes: "military strength might not win you the game but it will definitely lose you the game if you neglect it."

**Floor rule: never be last; ideally top-2.**

- "The most basic goal with Military is to **stay within a hair's breadth of your opponents. You don't have to be in the lead, but you don't want to be in last place**" — https://statelyplay.com/2017/09/25/strategy-101-through-the-ages-resource-edition/
- "**Top 2 military position** guarantees most military events benefit you (4-player) or nearly all (3-player)" — http://blog.lightningshroud.com/2018/01/through-ages.html; corroborated: "Just being one of the top 2 players on military strength means that all (in a 4p) or almost all (in a 3p) military events will benefit you, or harm an opponent" — https://boardgamegeek.com/thread/1866366
- "**try to avoid being the weakest** because some events punish the weakest player. Ideally you stay the strongest player to gain bonuses from the events, but that's not necessary" (BGG 2801950).
- "Through about **turn 11**, you can ride out being attacked, and you just need to avoid having the weakest military." — https://boardgamegeek.com/thread/1866366
- Target band: "**hang about 2–5 strength back from the leader**… stay within defense distance of the leader." — https://boardgamegeek.com/thread/2298716
- **Numeric floor for Age A/I: within 3–5 strength of the leader.** "Early period: **difference of 3~5** [is] often defensible" — https://hcy1.blogspot.com/2017/03/tta_31.html (because the new-edition defender may discard military cards for **+1 defense each**, up to their MA count — `sources/namu_military.txt`).

**Absolute strength floors by age** (3p, "minimum investment even if no one threatens you") — https://boardgamegeek.com/thread/1866366:

| Age end | Units | Strength |
|---|---|---|
| Age I | 3–4 units + Knights *or* Swordsmen + a matching tactic | **~10** |
| Age II | 4–6 units, Age II tactic, Cannon | **15–25** |
| Age III | Age II tactic + Air Force, or 1 upgrade to a non-antiquated Age III army | **~30** |

Age II/III scale from another source: "mid-to-late period 2: competitive warfare involves **40+ military strength, sometimes exceeding 60**" (*hcy1*). Standing strength caps at **60** (sacrifices can exceed it). Real endgame armies observed: 74/81/86/88/90/92 — https://boardgamegeek.com/thread/2597183.

**Aggression thresholds (the key numeric):** "Even with just your starting **two MAs, it takes a strength lead of 5 to guarantee a successful Age I aggression**. Aggressive players may attempt aggressions with only a **3 or 4** strength lead, but **once you add a third MA**… even the aggressive players will look elsewhere for their Age I prey." The author's own bar: "I'm loath to play an aggression in Age I with anything less than a **4-strength lead**." — https://boardgamegeek.com/thread/2424523. Steam version: "Attack only when **strength advantage exceeds opponent's available cards/military actions combined**."

**Age I aggression needs four things simultaneously:** the right tactic, Knights, a lot of food, and aggression cards in hand. "**Take one of the 4 things away and you won't be strong enough** to win aggressions." — https://boardgamegeek.com/thread/2801950

**Military actions (MA):** the **3rd MA is the key Age I breakpoint** (deters aggressions, enables 3 military draws). "The difference between **2 and 3** military action tokens is **greater than any other number difference**!" (namu, on Open Borders Agreement). "the importance of the **third red dot is still much greater than the fifth white dot**" (akong via Steam guide). Tiering: **3 MA = draw 3 cards/turn + options; 4+ MA = targeted aggression; 5+ MA = full warfare** (lightningshroud). "I am generally aiming for at least **4 MA**" — https://boardgamegeek.com/thread/2453050. Age III: "you should have **4–5 MAs at least** in Age III to keep building up military while fully drawing cards" — https://boardgamegeek.com/thread/2425822. Reserve "**three left over military actions each turn**" in Age III for seeding events — https://steamcommunity.com/app/758370/discussions/0/1639801448911030439/

**Unit economics** — https://boardgamegeek.com/thread/2424523:

| Age | Strength | Rocks |
|---|---|---|
| I | 2 | 3 |
| II | 3 | 5 |
| III | 5 | 7 |

All Age II unit techs cost 6 science; Knights 5, Swordsmen 4. Each unit costs a yellow cube — "Three Warriors technically provides the same amount of strength as a single Riflemen, but the former requires **three yellow cubes**." Age I tactic bonuses: Fighting Band +1, Legion +2, Medieval Army +2, Phalanx +3, Heavy Cavalry +4 per army. Age II: Conquistadors +5/+3, Classic Army +8/+4, Fortifications +5/+3, Defensive Army +6/+3, Mobile Artillery +5/+3, Napoleonic Army +7/+4. Age III: Entrenchments +9/+5, Mechanized +10/+5, Modern Army +13/+7. **Each Air Force doubles an army's tactic bonus.**

Endgame math — https://boardgamegeek.com/thread/2352577: best achievable rate is **1.33–1.6 strength per rock**; Age II tactic + Air Forces ≈ **6 strength per population**; Age III tactic ≈ **9 strength/pop**. Without Air Forces the ceiling drops to ~1.25 str/rock and ~5 str/pop. "If you want about **65 strength** you will probably need to spend at least **40 rocks** on units." Concrete: Classic Army + Air Forces = 29 str/army at 19 rocks (1.53).

**Which red techs, coded:**

- **Age I: Knights** (6 of 10 Age I tactics need cavalry) — "taking knights is always worth **2 civil actions, sometimes even 3**" (BGG 2801950). Payoff: **2 Knights = strength 5; 4 = strength 10** with Medieval Army / Phalanx (killswitch19 via search). Swordsmen is the acceptable substitute. "**Going into Age 2 with none of the military techs is very dangerous**" (BGG 2393942).
- **Age II: Cannon** ("4 of 6 Age II tactics require Artillery") — "worth **3 civil actions** if you have a tactic with cannons" (BGG 2801950).
- **Age III: Air Forces is the one universal auto-take.** "Air Forces is the only card I think is worth **3 CA in every circumstance**" — https://boardgamegeek.com/thread/2766920. "**Air Force is the single Military Tech that actually makes a difference statistically**… It's worthwhile even just to deny your opponents" — https://boardgamegeek.com/thread/1933554. "**ALWAYS PICK THIS! If you can't use it yourself, take it so the others can't**" (BGG 2393942). "You need to place more value on Air Forces… if you do not, **you are open season**" (BGG 2597183).

**Tactics discipline:** Age I tactics have **2 copies each** and never become antiquated — activate freely. **Age II tactics have exactly 1 copy** — hold them: "**delay activating an Age II tactic until necessary**… hang on to it until late into Age III and then drop it right as you're waging a game-closing War over Culture." — https://boardgamegeek.com/thread/2424523. Corollary: "**threatening** to increase your military can be as good as actually increasing it. Hoard yellow cards and make sure you always have **12 science** available for Air Forces." — https://boardgamegeek.com/thread/2597183. Also: "Tactics from Age II and after have **2 numbers**. The lower number is if you have any outdated units. **Look for any outdated units before you go all in on a tactic**" (BGG 2695320).

**When to actually attack:**

- Age I: essentially never unless a specific combo lands.
- **End of Age II / start of Age III is the window** — "If you wanna win a game by pure military strategy, you should take aggressions and declare wars at the **end of Age II or the beginning of Age III**. Building an Age III army is too late."
- **Age III targeting:** "**Attack the player who is winning, otherwise attack the weakest one. Don't switch targets, attack the same player over and over** because it's hard for him to build up military after losing 7 stones… **Save the attacks that steal culture for later**, start with the ones that steal resources, science and yellow cubes." — https://boardgamegeek.com/thread/2801950
- Risk framing: declaring war "you win unless they have a specific age 2 tactic (one copy) or Economic Progress is revealed; this choice should be made considering your position — **if already a heavy favorite, minimize variance**" — https://boardgamegeek.com/thread/2494200

**Aggression/war payoffs (codable, from `sources/namu_military.txt`):** Pillage 1 MA → 3/5/7 res+food. Raid 1/2/3 MA → destroy buildings. Age II Spy 1 MA → up to 5 science. Age II Infiltration 2 MA → remove leader/wonder, +3 culture per level. Age III Military Intervention 2 MA → up to 7 culture. **War over Territory** 2 MA → 1 yellow token + 1 per **5** strength difference. **War over Culture** 3 MA → **(5 + strength difference)** culture — "it is **not uncommon to lose 30 to 40 points**"; hcy1 (2009) documents **30–50 point** swings. Restated as a swing: "**X disadvantage in strength makes you lose 2X + 10 culture advantage** to the winner (because culture is taken away from you)."

**Historical success rates (1st ed., 100 games):** Age I aggressions **<50%**, Age II–III **~55–60%**, wars **~80%** — http://gamesstrategyandtactics.blogspot.com/2012/03/through-ages-part-xviii-military.html

**Don't over-build military either.** "What usually happens when you over-invest in military is your opponents will do whatever they can to stay close… creating a **Mutually Assured Destruction** situation where no one can attack, but everyone has a ton of resources invested" (Steam "Any tips?"). "**Over-Investment in Military**" is *hcy1*'s #2 beginner mistake, with the observation that most Age I aggressions cost the victim only **~2–3 resources**. Balancing statement: "My guess is that you might be **over-investing in military strength**. You don't need that much of a lead to win Aggressions, Events or Colonies, so doing that is essentially **gambling on Wars that may never come**" — https://boardgamegeek.com/thread/2425822

**Statistical clincher:** "If I have to ask **one question** to determine the winner, it will be '**who uses the most MA in the last 4 rounds**'." — https://boardgamegeek.com/thread/1933554

**Pacts:** "try to have **as many pacts as you can**… **Don't offer pacts to the winning player**" (BGG 2801950). 3–4p only.

---

## 6. Wonders (base game)

### Age A — real disagreement; ranked by weight of evidence

| Wonder | Cost | MasN | Tournament (avg CA) | 30k-game data |
|---|---|---|---|---|
| **Library of Alexandria** | 1/4/1 (3 stages, new ed.) | 5\* | **Tier 1, 0.31** | **wins more than Pyramids** |
| **Pyramids** (+1 CA) | 3/2/1 | 5\* | Tier 1, 0.23 | taken 20% more often, wins slightly less |
| **Colossus** (+2 str, +1 colonization) | 3/3 | **1\*** "the weakest wonder in the game" | Tier 2, 0.13 | **anti-correlated with winning** |
| **Hanging Gardens** (+1 cult, +2 happy) | 2/2/2 | 2\* | **Tier 3, 0.03** | — |

- **Pyramids:** killswitch19 **★★★★** "**1 Extra Civil Action × ~17 turns is incredibly powerful**"; namu: "like having Code of Laws, a science-6 tech — **very good wonder**… fits well with any civilization" (`sources/namu_wonders.txt`).
- **LoA vs Pyramids is genuinely close, and the data disagrees with the forums.** "These two are incredibly close… Given equal cost I lean *very slightly* towards Pyramids [because] the 3-2-1 cost is more convenient than the 1-4-1 of Library" — https://boardgamegeek.com/thread/2166558. Against: "Stronger players slightly prefer Pyramids over Library… however **Library performs significantly better for winning the game. I must conclude that this is a mistake made by strong players.**" — https://boardgamegeek.com/thread/1933554. The **#1 BGA player**: "**Pyramids: LoA is slightly better.**" — https://boardgamegeek.com/thread/2393942. Base-game summary line: "**LoA > Pyramids >> Colossus/Gardens**" — https://boardgamegeek.com/thread/2569870. namu: "**upgraded to level 3 in the new edition… Some players treat it as a wonder even better than the Hanging Gardens and Pyramids.**" Quantified: **Library ≈ 14–16 science+culture over the game; Pyramids ≈ 14–16 extra civil actions.**
- **Hanging Gardens is the widest split in the corpus.** **statelyplay ranks it #1** ("allowing players to skip religion buildings" — https://statelyplay.com/2017/09/29/strategy-101-through-the-ages-wonder-edition/) and namu praises it strongly ("Happiness 2 is an excellent ability that reduces worries about happiness **until almost Age 2**"). Against: BGG 2801950 **C tier**; killswitch19 ★★; *hcy1* "most disliked" ("**usually arrives late**"); "**I would honestly rather have no Age A wonder over this, every single time**" (https://boardgamegeek.com/thread/3584465); its 3-food completion bonus tips you into corruption; it is the **#1 Ravages of Time target** (−2 happiness). **Note that statelyplay is the oldest and the only base-game-only source, so a base-game card pool with scarcer happiness may partly justify it.** Code it as: **high value only if you plan an early 3rd/4th population increase or a St. Peter's / Michelangelo line.**
- **Colossus is contested.** Dead last for MasN and the 30k data. Defenders in 2p/4p militaristic metas cite the free Age II/III military card draws — **but that is the expansion/digital buff, not base game.** Base Colossus is +2 strength, +1 colonization only. *hcy1* still likes it in multiplayer: "the more people there are, the greater the pressure… it is the wonder of the A period **least afraid of the Ravages of Time**."
- **Skipping the Age A wonder is legitimate** if only Colossus/Hanging Gardens are available, especially to preserve CA for St. Peter's. "When you cannot get Pyramids or Library, maybe it is a very good play to **skip Age A Wonder** such that you can grab Basilica cheap" — https://boardgamegeek.com/thread/1933554, https://boardgamegeek.com/thread/2166558. If you skip, also **refrain from seeding early events** (the free resources are only useful if you have a wonder to sink them into).

### Age I

| Wonder | MasN | Tournament | 30k data |
|---|---|---|---|
| **St. Peter's Basilica** | **6\*** "the best wonder in the game" | Tier 2, 0.49 | best early wonder |
| **Great Wall** | 4\* | **Tier 1, 0.59** | the one Age I wonder that *doesn't* over-perform |
| **Universitas Carolina** | 4\* | Tier 2, 0.28 | undervalued |
| **Taj Mahal** | 3\* | **Tier 3, 0.00** | undervalued (!) |

- **St. Peter's** is the consensus #1. "**the age I wonder both players compete for is Basilica**" (#1 BGA player). "the best happiness solution in the game… generally goes for **3 CA** (2 from the row + 1 from the tax). People generally can't spare 4, but it's valuable enough to be worth 3" (MasN). "**only takes two stages to complete**… you don't have to worry about happiness throughout the game even if you have just one happy building" (namu). killswitch19 ★★★.
- **Great Wall** has the highest tournament CA-spend of any Age I wonder but the worst opportunity-cost analysis. "+1 strength for each infantry and artillery unit"; "worth **4 strength in age 1, up to 6 in age 2** with Defensive Army, upwards of **8** at the end" (BGG 2494200). Against: "**arguably the weakest of the Age I wonders**… you lose **10 culture** vs St. Pete's and **20** vs the Taj [over 10 turns], and the strength bonus **in practice works out usually to about 4 strength**" (BGG 2393942). Costs **4 stages / 9 resources**; needs Masonry. A 250-game BGA player strongly dissents: "most of the games that my opponents conceded on Age II/early Age III involved **Great Wall with a lot of cannons or infantry** and the right tactics with a lot of military action (GW+Strategy+ConMon or Napoleon)." **Rule: take only with an infantry/artillery tactic plan (Barbarossa / Genghis) and only for 1 CA.**
- **Universitas Carolina** — "an XOR with Alchemy and a fantastic first wonder, generally preferable to Pyramids or Library, but not always." "Along with the initial two philosophies, provides **enough science income for the rest of the game**." "the age 1 wonder I most want to see early, especially if an age A wonder was skipped." Will produce "**20 science (three more techs)**" over ten turns. — BGG 2258467, 2494200, 2393942
- **Taj Mahal** (+3 culture, +1 blue token; **−2 CA cost if you swapped leader this turn**) is the most contested card in the game. Consensus worst: "I really only consider taking Taj Mahal for **0 CA**"; "**Utterly useless wonder. If someone takes Taj, I automatically assume he may be an easy target later**"; killswitch19 ★; tournament 0.00. Against that: namu says it is "**often the source of the most scores for the entire game**", and BGG 2393942 computes it as **+20 culture over Great Wall across 10 turns**, and the 30k dataset says strong players *undervalue* it. **Rule: take only at 0–1 CA via the leader-swap discount.**
- **Age I wonders > Age A wonders in raw power.** "Age I wonders are much more powerful than Age A wonders." Many produce 2 culture/turn = "as much as **30 culture** through the game (if completed on turn 5 in a 19-round game), and usually at least 20." Age A culture wonders yield ~14–17. — https://boardgamegeek.com/thread/2569870, https://boardgamegeek.com/thread/2494200

### Age II

1. **Ocean Liner Service** — near-unanimous S-tier. "**Most gamechanging wonder in a game. And at the same time the cheapest Age II wonder**"; "competes with Great Wall for best in game"; "**Kiss your population problems goodbye**" (BGG 3584465). BGG 2801950 A tier; *hcy1*'s "**personal favorite of the second stage**"; statelyplay #1 Age II. Free worker per turn → skip food tech entirely.
2. **Eiffel Tower** (+4 culture, 13 resources) — "**it's actually the best age II wonder in base game**" (#1 BGA player). "Probably the **best pre-expansion Age II wonder**, by a narrow margin over Kremlin. The key is that it can be **built fairly late and still produce a decent payoff**… **built with just three turns to go still gets you twelve culture**" (BGG 2393942). Requires Iron: "If you don't [have Iron] you likely can't build any age II wonder." *Dissent:* "sometimes I take it in late age II… but I consider this wonder too weak" (BGG 3584465).
3. **Kremlin** (+2 cult, +1 CA, +1 MA, −1 happy) — BGG 2801950 C tier, but namu and BGG 2393942 both rate it near Eiffel. "Suitable for low-tech, high-mining production." Only with St. Peter's or another happiness solution.
4. **Transcontinental Railroad** (+5 strength, best mine produces twice) — payback **4 turns with Coal, 6 with Iron, "50 games with Bronze"** (BGG 3584465). "**In general, the new edition has been weakened**… very little opportunity" (*hcy1*). *Dissent:* "Railroad is by far the weakest wonder in the game and is **useless in 2 and 3 player games**" (BGG 2233796).

### Age III — roughly equal; pick by what you already built

Approximate yields, cross-source:

- **Hollywood** — 3 stages, biggest ceiling: "**Hollywood can be a 40-point spectacle**" with movies/Chaplin (*hcy1*); "~30 with a theater build"; "best with theater/library focus" (statelyplay).
- **First Space Flight** — "**around 20 to 30**" (*hcy1*); "up to 30+ with science focus"; best for science builds.
- **Internet** — 5 stages, cheapest resources: "**10 points** if you went military, **25 to 35** if the city developed" (*hcy1*); "20+ but needs Sid Meier + Computers or lots of multimedia."
- **Fast Food Chains** — safest and most build-agnostic: "**about 20-25 points**" (*hcy1*); "**17 or 18 culture without having to make any additional investments**, you just build the wonder" (BGG 2393942); "18–23, always somewhere between; 16 resources." "**the best of the 'vanilla' Age III wonders, only because it is good no matter what strategy you are going. Everyone has workers.**" (BGG 3584465)

**Threshold rule:** "**Late game wonders on average produce about 14 culture each** but the payoff can be over 30" (BGG 2393942); "**If you have 20 points, you can think about it**… sometimes a three-phase wonder of about 30 points appears in a very busy key round, and usually it will all be let go" (*hcy1*). Purchase rule: **buy iff production × turns remaining > cost** — "Picking up Fast Food Chains makes more sense if my production times the amount of turns left is higher than its cost of **16 resources**." **Code: build an Age III wonder only if projected culture ≥ ~20 and it doesn't consume a whole Age III turn.** Requirement: "You need **Architecture or early Engineering** to build at least one Age III wonder efficiently" (BGG 2425822). Also note: "there are enough ways to score Age III culture that **you can get by without an Age III wonder**" (BGG 2393942).

**General wonder rule:** "wonders… **don't require workers** and can be handy when there is no other good way to spend civil actions and stones. **Without a wonder it's possible that you get a lot of resources through events and won't be able to spend them, which leads to corruption**" (BGG 2801950). It costs **+1 CA per already-completed wonder** to take a new one, so wonder count is self-limiting.

---

## 7. Government

**Base-game costs** (from `sources/bga_card_counts.tsv`; tech cost = peaceful change, revolution cost in parens from `sources/namu_gov.txt`):

| Govt | Age | Peaceful | Revolution | CA | MA | Urban cap | Extra |
|---|---|---|---|---|---|---|---|
| Despotism | A | — | — | 4 | 2 | 2 | — |
| Theocracy | I | 6 | **1** | 4 | 3 | 3 | +1 cult, +1 happy, +1 str |
| Monarchy | I | 8 | **2** | 5 | 3 | 3 | — |
| Constitutional Monarchy | II | 12 | **6** | 6 | 4 | 3 | — |
| Republic | II | 13 | **3** | 7 | 2 | 3 | — |
| Communism | III | 19 | **5** | 7 | 5 | 4 | −1 happy |
| Fundamentalism | III | 18 | **7** | 6 | 5 | 4 | +5 str, −2 sci |
| Democracy | III | 17 | **9** | 7 | 3 | 4 | +3 culture |

**Revolution arithmetic (a genuinely settled disagreement).** Peaceful Monarchy = 1 CA + **8 science**. Revolution = **all** your CAs (4 under Despotism) + 2 science + likely 2 rocks of corruption. So a revolution costs **4 more CA to save 6 science**. (One thread originally said 3 CA; it was corrected to 4 — the revolution consumes the CA you'd otherwise have had.) — https://boardgamegeek.com/thread/2732988. As a rate: revolting gives **1.5 science per CA** (Monarchy) or **1.66 science per CA** (Theocracy) — "that's quite a solid deal and that's why revolting into these is often great" — https://boardgamegeek.com/thread/2166558. Sanity check offered there: "a good thought experiment is whether you'd spend a total of 4 CAs in Age I grabbing **3 copies of Breakthrough I**. It's a reasonable but not amazing play."

**Consensus target: Constitutional Monarchy, ideally skipping Age I governments.**

- "**Constitutional monarchy is still the best form of government in this version**" (akong via Steam guide, ★★★★☆ — the highest-rated government).
- "**Constitutional Monarchy is the best all round form of government**… comes just early enough that **you might be able to skip Monarchy, particularly if you have Code of Laws**" (BGG 2393942). MasN 6\*: "generally the best peaceful development in the game."
- "**I recommend ignoring the age I governments**, this leaves you with 4 civil actions until you get the age II government… if you develop the Law of codes you will be able to do **25% more things per round** thanks to having **5 civil actions instead of 4**. This is huge." — https://boardgamegeek.com/thread/2801950
- "**The early to mid-2nd era is when tokens are scarce**… the Republic was not loved because it had **7 civil action tokens but only 2 military action tokens**, while [ConMon] has 4 military tokens, allowing you to **draw 3 military cards every turn**" — `sources/namu_gov.txt`.
- **Statistical dissent:** **Republic outperforms ConMon** in the 30k dataset (cheap revolution frees science for Strategy) — https://boardgamegeek.com/thread/1933554.
- **Decision rule when both are live:** if science income is low (≈1–2/turn), **revolt to Republic now** rather than pay 4 more science and 2 turns for ConMon — "when you're at 1 science per turn, you simply can't recover ever if you take two turns to revolt into const monarchy." If you have alternate MA sources (leader / Kremlin / Strategy), Republic is fine.
- **Fallback if you can't get ConMon:** "at least develop **Strategy** for the two additional military actions and 3 strength" (BGG 2801950).

**Age I government timing: value is entirely about how early you get it.**

- "The value of Age I governments is **all about how early you get them** (each turn means an extra CA and MA)… If it comes early, [Monarchy is] really good, **5\***, even **6\*** if it's the first card from Age I. **If it comes late, don't bother.**" — https://boardgamegeek.com/thread/2258467
- "**Very early Monarchy: just revolt and take the corruption.** You lose 2 rocks but if you get your government a turn or two earlier it can be well worth it." — https://boardgamegeek.com/thread/2166558
- Hammurabi enables a **turn 3 or turn 4** revolt: "mine T2, lab T3, quick-revolt T4, wonder T5." Earliest legal revolution is **turn 4** (turn after taking the card) — https://forum.boardgamearena.com/viewtopic.php?t=5185.
- **Skip-government-entirely conditions:** 5th CA from Pyramids/Code of Laws **plus** 3rd MA from Joan (or Colosseum, expansion) → "allows comfortably skipping an age 1 government" (BGG 2494200).
- **Governments are exclusive with each other:** "If you have an Age I government, you generally won't be getting an Age II government" (BGG 2258467).
- **Revolution is a better deal the further behind on CA you are** — so with Pyramids + Code of Laws it's often correct to sit on Despotism and wait until Age III (BGG 2258467).
- Timing decay: "Transitioning to Constitutional Monarchy is much less exciting when Age II is almost done" — https://boardgamegeek.com/blog/9362/blogpost/97753/
- Regional guidance from akong: "**If you want to change the government [peacefully], it is best to change [in] phase I. If you want to use the revolution, it is best to be in phase II. (I, III are very busy and it is best not to revolutionize.)**" Plan the revolution the turn before, using red tokens only, to avoid corruption (*hcy1*). Newton refunds a white token so you keep one action through a revolution.

**Code of Laws vs Monarchy:** "If you probably are going to stick with Monarchy the entire game then [Monarchy] is better (e.g. you have Pyramids). If you are going to have a need/opportunity to change government later then **Code of Laws is better** (e.g. you have Alchemy)." — https://boardgamegeek.com/thread/2375641. 30k data: "**Code of Laws performs much better than Warfare. This shows that an early CA is better than an early MA.**" — https://boardgamegeek.com/thread/1933554. Also: "**1 extra civil action is as valuable as 2 upgrades of Iron!** So I usually think Code of Laws' priority is higher than Iron" — https://boardgamegeek.com/thread/2732988.

**Civil-action targets by age** — https://boardgamegeek.com/thread/3292858:

| | Age A | Age I | Age II | Age III |
|---|---|---|---|---|
| Gripperas' targets | 4 | 5 | 6.5 | 8 |
| Ranior's *actual averages* (4p) | 4 | 4.75 | ~6 | ~6.5 |
| Minimum floors (thenobleknave) | 4 | **get the 5th in Age I** | **get the 6th in Age II** | — |

"There is an optimum number of CA, something like **five** in the early game, **six** in the middle game, and **seven** in the late game" — https://boardgamegeek.com/thread/2695320. "**6-7 Civil Actions tends to be the sweet spot**, which is why Constitutional Monarchy is so strong… without having unspent Civil Actions at the end of your turn" (killswitch19 via search). Diminishing returns are real: "the difference between **9 and 10** civil actions is barely noticeable" (BGG 2801950). Realistic totals: players achieve 30–60% of theoretical maximum, ≈**100–120 total civil actions per game**, roughly half in Age I.

**Age III governments: mostly skip.** "**Democracy is excellent but expensive, and it cannot be a major means of running**" (akong ★★★☆). "if you reconsider the timing of the revolution, it's **hard to get more than 8 points** with that [+3 culture]" (namu). Counterpoints: "**(III) Democracy. +3 culture is perfect at that stage!**" (BGG 2393942 must-buy list); "**Democracy** is statistically the Age III government you want" but "**Air Force and Civil Service probably deserve your science more**" (BGG 1933554); lightningshroud: "strongest option **if already at Republic/Constitutional Monarchy**" (cheap upgrade path). **Fundamentalism** is the military-build exit (+5 str, −2 science); **Communism** only with a big happiness surplus. **Civil Service:** explicit math against it — best case Age III round ~14 for 1 CA, gaining 1 extra CA for 4 rounds = "**adding 14% to your CA ability for four turns**" if you already have Code of Laws and 7 CA. Take it only if you're stuck on Monarchy with 5 CA or want Impact of Government.

---

## 8. Age III / endgame

**Game length (calibrate your "rounds left" counter).** "Games are almost always **19-21 turns and 20 seems the most common. Evenly spread over eras**" — ~**6 turns per age** — https://boardgamegeek.com/thread/1614742. Same thread: "**We pretty consistently hit Age III by the 13th turn**"; another: "**18-20 turns**… less turns with more players"; "roughly **17 turns for 2 players, 20 for 4**." More precise: "Age I will almost invariably end at **turn 7**. Age II usually at **turn 13**, goes to 14 a lot, sometimes ends at 12. Age III ends on **turn 18** most often, but can end on any turn between **16 and 20**." Median = **19 turns**. — https://boardgamegeek.com/thread/2695320, https://boardgamegeek.com/thread/2766920

**Turn-count estimator:** count the Age III deck and assume **each player takes ~1.5 cards per turn**; e.g. 3p with 23 cards left → 10.5 cards/turn removed → ~3 turns left + the Age IV turn. — https://boardgamegeek.com/blog/9362/blogpost/97753/

**Culture rate target: 10–15 culture/round in Age III.** "**Producing around 10-15 culture per round is pretty good for the third age.** However, **don't let culture production get in the way of your military**, unless you want your culture points to be stolen by someone else" (BGG 2801950). Culture-specialist builds hit "**15-25 points per round**" (Steam guide) but must bank a "**conservative lead of only 50**" / "**earn 100 points more than your opponents**" as a war buffer. Culture engines can reach +30/turn, which makes you the War over Culture target.

**Culture rate vs one-shot: one-shots dominate, more so in the base game.**

- "**Age III is such a huge scoring round it is very common to be down by 100 points and still comfortably win**" (via killswitch19 search).
- "**Aim for Wonders.** Ideally you want a big resource production, something like 4 Iron or 3 Coals… Quite often enough to negate early culture leads." — https://boardgamegeek.com/thread/1933554
- Explicitly for the base game: "I find culture generation even **weaker in the base game** in general, and **wars often a bit more important**. Still, gaps of even **30–40 culture** entering Age III are often able to be overcome." — https://boardgamegeek.com/thread/2425822
- "you can get **over 20 culture points with just 1 event**" (BGG 2801950).

**When to stop investing in economy — conditional, not a fixed round.**

- "**All types of stored resources are worth nothing at the end**, so you must constantly look out for whether it is the right time to stop building economy, and start getting the points" (via killswitch19 search).
- Decide at the Age III transition, after seeing your first ~3 Age III political cards.
- "I don't tend to take the Age III upgrades much. **Age III cards are mostly relevant in combination with specific leaders, wonders and impacts. By themselves they are rarely worth it for the stats alone** — building an Age II equivalent earlier nets you much more." — https://boardgamegeek.com/thread/2724657
- "If you gain a comfortable culture lead and have good culture production, you should **shift to full-on military defense mode and forget about developing more culture**." — https://boardgamegeek.com/thread/2597183
- "When you think you are losing, **take risks and play for your outs**. When you think you are winning, **play safe and against the outs of your opponents** — deny them important military cards even when it is costly for you, because at the end it doesn't matter if you win with 70 or 50 points." — https://boardgamegeek.com/thread/2679238
- Oil payback math as a general template: "every oil worker must work **at least 2 turns** to return the cost… **3 turns** [from Iron]" (BGG 2393942).
- Late-wonder payback template: Eiffel with **3 turns left = 12 culture**; a Movies theater with 3 turns left = the same but costs science, a CA and a worker (BGG 2393942).

**NEVER fire your production workers.** "**Fire your workers producing stone and food only as the last resort** if you don't have enough yellow cubes — you can miss out on gaining a lot of culture points through events if you don't produce stone or food" (BGG 2801950). This is because of `Impact of Agriculture` (culture = food production, +4 if production > consumption), `Impact of Industry` (culture = resource production), and `Impact of Balance` (**2× your lowest of culture/science/food/resource production**) — `sources/namu_events.txt`.

**Balance rule (directly codable):** "try to produce **at least so many science points as your lowest production of food and stone** — for example you produce 6 food and 9 stones, you then need a science production of 6 so that if the event comes out that gives you double the culture points of your lowest production you get **6×2 = 12** culture points" (BGG 2801950).

**Age III Impact list with exact numbers** (`sources/namu_events.txt` — all base game unless marked):

| Impact | Payout |
|---|---|
| Science (rank) | **15/10/5/0** (4p), **14/7/0** (3p), **10/0** (2p) |
| Strength (rank) | **15/10/5/0** (4p), **14/7/0** (3p), **10/0** (2p) |
| Buildings | culture = sum of urban building levels |
| Competition | culture = sum of military unit levels + arena worker levels |
| Technology | **4 culture per Age III tech** |
| Agriculture | culture = food production; **+4 if production > consumption** |
| Industry | culture = resource production |
| Wonders | **5** per Age A, **4** per Age I, **3** per Age II, **2** per Age III wonder |
| Colonies | **3 culture per colony** |
| Population | **2 culture per content worker beyond 10** |
| Government | **2 per civil action token, 1 per military action token** |
| Progress | **2 per level of government + special (blue) tech cards** |
| Happiness | **2 per happy face (cap 16), −2 per discontent worker** |
| Balance | **2× the lowest of culture / science / food / resource production** |
| Variety | **2 per type of military unit / urban building / blue tech** |

Endgame scoring rates from the cheat sheet (`sources/hypercheat.txt`): **2 pts per level-1 tech, 2 per strength, 2 per happy face, 1 culture per food+resource produced, culture equal to your science rating.**

**Rank-awareness rule:** "try to produce **more culture and science than others and be stronger than others** because there are events that grant different amounts of culture depending on your relative strength/science per round/culture per round. **If you see someone increasing their science production or strength at the end of the game without a visible reason** you know they probably have played such an event. **Try to overtake them at least by one point** in strength, culture or science." (BGG 2801950)

**Impact synergy clusters to build toward** (base subset): Computers + Impact of Science/Progress/Technology + Einstein/Gates + First Space Flight; Mechanized Agriculture + Impact of Population/Agriculture/Balance + Fast Food Chains; Libraries/Theaters/Arenas + Impact of Culture + Architecture + Chaplin + Hollywood/Internet. — https://boardgamegeek.com/thread/2425822

**Ending the game early is a lever.** "**End the age quickly.** Getting the really big military numbers requires the game to have gone long. If you come into Age III with a culture lead, **take as many cards as possible each turn**. Sometimes I've even skipped taking good wonders because it took up too many actions that I could instead spend accelerating the game end." — https://boardgamegeek.com/thread/2597183. A documented tournament case: the third player "took a whopping **five cards** from the card row on what became the last turn of Age III" purely to deny the culture leader another turn — https://boardgamegeek.com/blog/9362/blogpost/97753/. The last player in turn order controls whether the game is called — https://boardgamegeek.com/thread/2732988.

**Age III balance checklist at the transition:** 6 civil actions / 4 military actions / 6 resources / 4 science; plus you must be able to raise or disband a worker without sacrificing more than 2 production, and keep an idle/low-level worker available. A higher science target from another player: "Science is king in age 3. You should be producing **10+** as a goal during that age so that you can develop a tech each turn if possible." Compare BGG 2801950: "keep your science production above 3… producing **4 science** per round can be enough if you grab the yellow cards that give you science."

---

## 9. Card row drafting

**Structure:** 13 face-up slots — **positions 1–5 cost 1 CA, 6–9 cost 2 CA, 10–13 cost 3 CA** — https://throughtheages.fandom.com/wiki/Card_Row. At the start of each round **3/2/1** cards are removed in a **2/3/4** player game respectively, then all cards slide left. **Hand limit = your maximum number of civil actions** (`sources/hypercheat.txt`).

**The headline empirical number: across 39 tournament games, 76% of Age I cards were picked at 1 CA, and only 2.5% at 3 CA.** — https://boardgamegeek.com/thread/2494200. Corroborated by a 4p regular: "In Age I I'm taking a card for 3 CA on average **less than once per game**, and probably averaging taking 2 CA cards in the **1.5–2** range" — https://boardgamegeek.com/thread/3292858. **Encode a strong prior toward 1-CA picks in Age I.**

**MasN's CA-value scale** (calibrated in "row position you'd take it from") — https://boardgamegeek.com/thread/2258467:

| Rating | Meaning | ~CA |
|---|---|---|
| 1\* | Nearly useless; negative value by clogging your hand | 0.1 |
| 2\* | Usually not happy to take, sometimes anyway | 0.4 |
| 3\* | Weak side, has its place | 0.7 |
| 4\* | Fair deal for 1 CA | 1.0 |
| 5\* | Happy at 1, sometimes 2 | 1.4 |
| 6\* | Usually taken at 2, too good to pass | 1.9 |
| 7\* | Best in game; almost always 2, sometimes 3 | 2.4 |

**When to pay 3 CA** — https://boardgamegeek.com/thread/713x49 (via old.reddit, see Appendix B) and https://boardgamegeek.com/thread/2766920:

- Threshold rule: "I tend **not to use 3 civil actions until I have 5 or 6 available**. Burning 3 actions when you only have 4 is very painful and will likely cause corruption."
- Age I/II exceptions: **last Knights, last Cannon**, Monarchy/ConMon, Napoleon, an early Selective Breeding, Iron.
- Age III: "this becomes much more common as I aim to have **at least 6–7 civil actions** by then… wonders usually will take up 3–4 actions anyway and Age III wonders can easily be 20–30 points."
- Free-lunch rule: "If I can take and play Constitutional Monarchy for a net of at least **0 civil actions this turn**, it's kind of a no-brainer."
- **The short universal 3-CA list from the top BGA player:** "the only technologies I usually autograb for 2 or 3 CA are **Strategy, the first Air Forces, and potentially Selective Breeding**" (BGG 2393942). Air Forces is the only true universal: "the only card I think is worth **3 CA in every circumstance**."
- Contested: "**no card is worth spending 3 civil actions to take in all games**" vs (2p) "Bach and Shakespeare are even worth 3 civil actions."

**Yellow (action) cards: 1 CA in Age A/I; up to 2 CA for Age II+ yellows.**

- "In Age A/I the Yellow Cards that only require **one Civil Action** are far superior" (killswitch19). "**Most of the other yellows are to be avoided unless they are 1CA**" (BGG 2393942).
- #1 BGA player: "**Most yellow cards are great picks at 1CA from Age 1** (exception: Reserves since it's effectively 2CA), and **from Age 2 cards onwards it's pretty typical to take them for 2CA**."
- Best of them: **Engineering Genius for 1 CA is "a no-brainer"** — "**provides a larger discount in Resources than any other card from its equivalent era, without costing extra Civil Actions to realize**" (killswitch19). EG-A is worth **2 CA** as a denial pick in 2p: "you don't want your opponent to take it for 1 CA — if you can leave it at 6 o'clock for your opponent, that's generally superior."
- Substitution rule: "If Engineering Genius is at 2 and you have Library/Pyramids/Hammurabi + Urban Growth/Rich Land at 1, **then there's no reason to take Engineering Genius**."
- **Never take (Age A):** **Stockpile**, **Patriotism A**, **Cultural Heritage**, **Frugality** — "You just don't have the CA's to spend at this point"; "I **risk getting hit by corruption** using this card" (BGG 2393942). Independently: "avoid taking cards that require an additional action to use them, namely **patriotism, cultural heritage and stock pile**" (BGG 2801950). Note: "**Patriotism I, on the other hand… pretty good card.**"
- Efficiency comparison (codable): with 1 CA left, upgrading a Bronze→Iron beats taking Rich Land I — the former is 1 CA for 1 rock/turn forever; the latter is effectively 2 CA (take + play) for 2 rocks once. "**1 resource is not worth 1 action point in Age I**" — https://boardgamegeek.com/thread/2732988

**The golden rule of tech drafting:** "**Don't take a tech from the card row if it is not highly likely that you are going to activate it in the next few rounds.**" Corollary: "try to always have **1+ free space** in your civil hand." Exceptions: hate drafting (rare), swimming in CA (Age III only), and military coverage (Cannon). — https://boardgamegeek.com/thread/2287034

**Tempo/corruption coupling (a real constraint, not a heuristic):** every card you take is a civil action you can't spend on resources, so drafting must be matched to income. "**there is a thing called tempo**… **The more stages your wonder has and the more resources you get thanks to your leader and events, the fewer civil actions you should spend on taking cards.**" Slow leaders (must spend, so draft less): **Moses, Homer**. Fast leader (draft freely): **Hammurabi**. Special: **Aristotle** — his tech draws already eat 2–3 CA. — https://boardgamegeek.com/thread/2801950

**Let cards slide deliberately:** "For each card you might want, consider whether the other players will want it. Often you can tell they won't, and you can let the card slide down the row so it costs a CA or two less. **Wonders and leaders are often good to let slide** because other players can only have two leaders and be working on one wonder. **The last copy of some critical tech often just has to be taken that turn, even for three CA.**" — https://boardgamegeek.com/thread/2695320

**Card-row placement control (2p specific):** "A veteran usually leaves the card the opponent urgently needs on the **6th place**. The opponent must either pay 2 CA or give it up, since it'll be taken or removed on your next turn. You can end a turn with the card you want on the **6th, 7th or 8th** places and spend only 1 CA next turn. **Don't try this in 3p/4p** — you reduce your next player's efficiency at the risk of reducing your own, and the other players benefit." — https://boardgamegeek.com/thread/2732988

**Opportunity vs Consolidation phases (the best structural framing found):** the start of an age has high card-row variance → be ready to pay up to 3 CA. The end of an age has low variance → spend CA on upgrading workers / increasing population instead, and **bank CA + hand space** for the next age's opening. "**CAs which you spend for grabbing from the card row increase in value as the game progresses, while it is the other way around for CAs you spend to upgrade workers.**" — https://boardgamegeek.com/thread/2724657

**Hand hygiene at age boundaries:** "if you have a **hand full of technologies at the end of an age**, you won't be able to draft the cool new techs of the next age. **I like to end an age with only yellow cards in hand**" — https://boardgamegeek.com/thread/2101737

**Hate drafting is mostly a 2p tool.** "in higher player counts, by hate-drafting player B you're just helping player C at the expense of both of you." Exception: the Air Forces denial pick. "**Spending 3 CA to hate-draft is questionable even in 2p**… A more reasonable 3 CA hate-draft is **Breeding when they don't have Irrigation**" (#1 BGA player). "Card denial is really only important on **farms, mines, and governments**."

**Singletons drive the draft.** In base game the one-of cards include: Strategy, Selective Breeding (in 2p), Drama, Journalism, and **every Age II tactic**. "**The game is all about singletons.**" — https://boardgamegeek.com/thread/2482857

**Card value is time-dependent and type-dependent:** "Maybe a card is worth 3 civil actions in round 2 but is worth only 1 civil action in round 7." And: "**Economy leaders have higher value *before* the event deck flips. Military leaders increase in value *after* the deck flips.**" — https://boardgamegeek.com/thread/2494200

**Must-buy list (most of the time)** — https://boardgamegeek.com/thread/2393942, base cards only: Engineering Genius A; Alchemy; Warfare + Strategy + Military Theory; Knights/Swordsmen; Code of Laws; Iron/Irrigation; Cannon; Architecture; Constitutional Monarchy; Breakthrough (all ages); **Air Forces**; Democracy.
**Never-buy list:** Cultural Heritage A, Patriotism A, Stockpile A, Drama I, Oil III, Satellites III; almost-never: Bread & Circuses I, Theocracy I, Fundamentalism III.
**Bottom four techs by another strong player:** Oil, Satellites, Bread & Circuses, Drama. **Top four techs:** Iron, Cannon, Constitutional Monarchy, Computers. **Masonry is separately singled out as terrible:** "3 Science + 2 CA early game… the time until investment payoff is extremely high" — take **Architecture** instead.

---

## 10. Top mistakes strong players attribute to beginners and bots

1. **Neglecting military, then getting culture-warred.** The single most-repeated lesson. "I had a 60–80 point lead towards the end and then had war declared on me by 3 others and ended up last." — https://boardgamegeek.com/thread/2679238. "Most new players think gaining a culture lead early is super fun. But **gaining culture instead of building your infrastructure and military is a recipe for defeat**." — https://boardgamegeek.com/thread/2597183. "**Ignoring Military will quickly lead to your downfall as both humans and AI players will happily pounce on a weakling**" — https://steamcommunity.com/app/758370/discussions/0/1696043263506122183/
2. **Building culture too early.** "Fast way to throw a game is to play Michelangelo, meld Drama/Theology, and start spamming these culture generators… You'll be at a **60+ culture lead in Age II, then lose two wars over culture for like 50 points each**." — https://boardgamegeek.com/thread/2258467. "In the early game I never take urban buildings like the printing press, because **too much culture early on is usually an invitation to attacks**." — https://boardgamegeek.com/thread/2732988
3. **Hoarding tech cards you can't activate.** "I'm seeing a significant amount of players with their civil hand full of technologies they just can't play in a timely manner. **This is killing your game, I can guarantee you.**" — https://boardgamegeek.com/thread/2287034. Also: "Holding too many undeployed cards simultaneously" (Steam Hard-AI thread).
4. **Incomplete infrastructure — starting upgrades and not finishing them.** "Players delay completing buildings by **2 turns** or more while chasing other cards"; Iron costs **5 science, 3 actions, 3 ore** for **+1 ore/turn** if you only upgrade one mine — no better than just building a 4th Bronze (**0 science, 1 action, 1 worker, 2 ore**). — https://hcy1.blogspot.com/2017/03/tta_31.html
5. **Forgetting the −2 yellow tokens at each age change → uprising.** "**Instant game losing.**" — https://boardgamegeek.com/thread/2695320
6. **Never upgrading civil actions.** "Being in **Age III Despotism with no extra actions is a recipe for disaster**." — https://boardgamegeek.com/thread/2892591. "You can play on with no mine technology, no science technology, no food, but **you can never get away with too few civil actions**." — https://boardgamegeek.com/thread/2732988
7. **Overpaying for cards.** "Avoid overpaying for cards; new players **do this too often**" (Steam Hard-AI). No leader is worth 3 CA before Age III (*hcy1*).
8. **Overpaying for colonies.** "Acceptable maximum: death of **1 unit**… **Avoid spending 3+ units**" (*hcy1*). "**don't take [colonies] if you can't defend yourself by rebuilding your army right after**" (BGG 2801950).
9. **Over-producing food / letting resources sit → corruption.** "Overestimating mineral and food production"; "**2 or 3 food cubes stored but consumption equals production**… those 2-3 resources sit unused and reduce your available blue cubes" (BGG 2101737).
10. **Neglecting science — in both directions.** "**Insufficient science before drafting technology**" *and* "**Collecting more science cards than you should vs. picking more yellow cards**" (Steam Hard-AI).
11. **Undervaluing military cards and the 3rd MA.** "**Underestimating military action card importance**" (Steam "Any tips?"). You draw 1 card per unused MA, max 3.
12. **Buying redundant tech.** "Avoid acquiring **two of the same type of tech** in early ages"; "**Skip intermediate techs** in chains (e.g. Iron→Oil or Bronze→Coal)"; "avoid tech upgrades appearing at age-end; wait for the next age instead."
13. **Playing events just for 1–2 culture.** "**Do not play event cards only to get 1–2 points**, be prepared for the events that you are going to play. When you don't have a favourable event to play, skip or offer a pact." — https://boardgamegeek.com/thread/2695320. "**certainly don't play them when you are the weakest player**" (BGG 2801950).
14. **Missing the antiquated-tactic rule.** See §5.
15. **Noob-trap culture engines.** Michelangelo and Bach "**invite novice players to over-invest in something extremely unwise**" (BGG 2393942).
16. **Adding population without infrastructure or happiness.** "**Adding excess population without supporting infrastructure**" (Steam "Any tips?").
17. **Not tracking the civil deck.** "Get used to tracking how many cards are remaining in the civil deck, and remember that in four player games, **the player who goes fourth has the most control over the pace of the game**." — https://boardgamegeek.com/thread/2695320
18. **Late-game AP / over-planning.** "**Being able to predict the endgame sufficiently correctly at the start of Age III is an illusion**… don't spend too much of your thinking time on long-term planning during the high-randomness phase. **The consolidating phases at the end of ages reward thinking time much better.**" — https://boardgamegeek.com/thread/2724657
19. **Bot-specific (digital AI):** "**Easy AIs tend to over-invest in one thing and ignore another**, which may distort your opinion of how to beat them. **Hard AIs will be meaner and better use wonders and leaders.**" — https://steamcommunity.com/app/758370/discussions/0/1696043263506122183/. And the classic AI failure mode: "**AI players in the TtA-App regularly reach ridiculously high production rates, ending up with lots of corruption losses and tons of resources lying around unused.** That surely can't be any better." — https://boardgamegeek.com/thread/2097526. The pro counter-heuristic: "**Don't try to stockpile your resources. Spend them instead.**"

---

## Consolidated priority list (codable)

### Age A (turns 1–4)

1. Take a leader T1 (Hammurabi > Aristotle > Homer/Alexander > Caesar/Moses), 1 CA. Don't pay 2 CA for a leader you can get for 1 next turn.
2. Take a wonder T1 (LoA ≈ Pyramids > Hanging Gardens > Colossus), 1 CA. Skipping is legitimate if only Colossus/HG are available.
3. Build the 3rd Bronze mine on your first available action.
4. Engineering Genius at 1 CA (2 CA in 2p as denial). All other Age A yellows only at 1 CA; never Stockpile / Patriotism A / Cultural Heritage / Frugality.
5. Reach **2 farm / 3 mine / 2 lab** by turn 3; keep **1 idle worker** (contested); 2nd pop increase at 4–6 stored food (turn 4–5).
6. End of turn: blue bank ≥ 11 tokens (≤ 5 committed).

### Age I (turns ~5–10) — priority order per BGG 2801950

1. Don't be last in military; stay within **3–5** strength of the leader; reach **~10 strength** by end of age. Take Knights or Swordsmen (2 CA OK).
2. 5th CA: Code of Laws (or Pyramids / Hammurabi). Consider skipping Age I governments entirely.
3. Alchemy (2 CA OK) → **4 science/turn** target (3 acceptable).
4. Iron **only if early and finishable in ~2 turns** (3–4p); otherwise stay on 3–4 Bronze and buy CA + yellows (2p).
5. Finish your wonder before the age ends; St. Peter's > Universitas > Great Wall > Taj Mahal (Taj only at 0–1 CA).
6. Happiness: 0 buffer until pop increase #2, then exactly +1 ahead; **2 happy faces banked for the Age II transition**. Prefer wonder/leader happiness; skip Theology and Bread & Circuses if you can.
7. Food: reach **exactly 0 stored food** entering Age II if you'll produce 0.
8. Get the 3rd MA.

### Age II (turns ~11–15)

1. Constitutional Monarchy → 6 CA / 4 MA (or Republic if science-starved; or Strategy for +2 MA / +3 str if you miss both).
2. Selective Breeding (3 CA OK) if you skipped Irrigation. Coal only if no Iron.
3. Cannon; keep military within reach of the leader; reach **15–25 strength** by end of age.
4. Fix science to ≥3–4/turn; pick one of {Scientific Method + Opera}, {Journalism + Team Sports}, {Journalism + Opera}.
5. Blue techs: Architecture (6 sci) > Justice System / Navigation. Skip Masonry.
6. Culture is the **lowest** priority in Age II.
7. Age II wonder only with Iron: Ocean Liner ≈ Eiffel > Kremlin > TCR.
8. Rock production ≥ +5/turn by end of age.

### Age III (turns ~16–21)

1. Air Forces at up to 3 CA, always — even purely as a denial pick.
2. 10–15 culture/turn; 3–4 spare MA/turn to seed events; **~30 strength floor**.
3. Never fire farm/mine workers; keep science ≥ min(food, resource) production.
4. One Age III wonder only if projected ≥ ~20 culture and it doesn't eat a whole turn. Needs Architecture/Engineering.
5. Attack the leader (or the weakest); same target repeatedly; resource/science aggressions first, culture-stealing last. Hold the single-copy Age II tactic until the game-closing war.
6. Stop economy investment once payback exceeds remaining turns (Oil: 2–3 turns — almost never).
7. If ahead on culture: accelerate the game end by taking more cards per turn, and switch to military defence.

---

## Biggest open disagreements — deliberately NOT resolved

Where experts genuinely disagree, this is visible rather than resolved by fiat. Parameterize these; do not hard-code them.

| Question | Camp A | Camp B |
|---|---|---|
| Pyramids vs Library of Alexandria | Pyramids slightly better (cost curve is more convenient) — https://boardgamegeek.com/thread/2166558 | LoA measurably wins more; "**this is a mistake made by strong players**" — https://boardgamegeek.com/thread/1933554; #1 BGA player agrees — https://boardgamegeek.com/thread/2393942 |
| Hanging Gardens | **Best Age A wonder** (statelyplay 2017, the only base-game-only source) — https://statelyplay.com/2017/09/29/strategy-101-through-the-ages-wonder-edition/ | "Do not play tier" / C tier / "**I'd rather have no Age A wonder**" — https://boardgamegeek.com/thread/3584465, https://boardgamegeek.com/thread/2494200 |
| Homer | Tier 1 in 3p/4p tournament data (0.28) — https://boardgamegeek.com/thread/2494200; "a smiley face all game is worth more than any short-term resource" — https://boardgamegeek.com/thread/1761996 | D-tier in 2p ("the CA lost when Homer is replaced can be very costly"); "no immediate benefit, happiness is useless at the start" |
| Julius Caesar | "**terrible** in the new TTA; MA in Age I are worth little" — https://boardgamegeek.com/thread/1761996; worst Age A leader among good players — https://boardgamegeek.com/thread/1933554 | "3 red points is very defensive… Caesar + Colossus is a very good match" (*hcy1* via https://steamcommunity.com/sharedfiles/filedetails/?id=1367549747) |
| Iron vs 3-Bronze | "must grab" in 3p/4p; skipping risks Coal being hate-drafted — https://boardgamegeek.com/thread/2569870, https://boardgamegeek.com/thread/2393942 | 3-bronze-all-game is a winning line; top-10 players "don't improve either mines or farms" — https://boardgamegeek.com/thread/2097526, https://boardgamegeek.com/thread/2393942 |
| Iron at the end of Age I | "**never upgrade a track at its age's end**" — https://boardgamegeek.com/thread/2097526 | "**Taking Iron at the end of Age I is often great**" — https://boardgamegeek.com/thread/2724657 |
| Colossus | Worst wonder in the game, 1\*, anti-correlated with winning — https://boardgamegeek.com/thread/2258467, https://boardgamegeek.com/thread/1933554 | A-tier, "most underrated" — https://boardgamegeek.com/thread/3584465 (**note: partly relies on the expansion's card-draw buff**) |
| Great Wall | "arguably the weakest of the Age I wonders" on culture-opportunity math (−10 vs St. Pete's, −20 vs Taj over 10 turns) — https://boardgamegeek.com/thread/2393942 | Highest Age I tournament CA-spend (0.59); a 250-game BGA player says it won him most of his concessions — https://boardgamegeek.com/thread/2494200, https://boardgamegeek.com/thread/2393942 |
| Taj Mahal | Consensus worst wonder; "only for 0 CA"; tournament 0.00 | +20 culture over Great Wall across 10 turns; 30k data says strong players **undervalue** it — https://boardgamegeek.com/thread/1933554 |
| Age II government | ConMon is the best peaceful development — https://boardgamegeek.com/thread/2258467 | **Republic wins more statistically** (cheap revolution frees science for Strategy) — https://boardgamegeek.com/thread/1933554 |
| Age I government | Skip entirely; take Code of Laws and wait for Age II — https://boardgamegeek.com/thread/2801950 | Very important, especially 2p, since the opponent can otherwise take both Age II governments |
| Float a worker in Age A | CGE designer David Jablonovsky says yes — https://boardgamegeek.com/thread/2695320 | Guaranteed 1 food beats a ~1-in-10 free temple — https://boardgamegeek.com/thread/2166558 |
| Age I culture | "culture is **completely irrelevant** in Age I" — https://boardgamegeek.com/thread/2801950; "never be stressed out by being last in culture in age 1 or 2" | "I find it hard to catch up if you totally ignore culture production in Age I… I will find the way to have **+2 to +4 per turn**" |
| Yellow cubes | **Increasing** returns (enable deferring Theology/Irrigation) — https://boardgamegeek.com/thread/2494200 | **Diminishing/anti-synergistic** (you drown in corruption and miss efficient farm/happiness tech) — same thread |
| Michelangelo | Bottom tier, unreliable, "noob trap", <5% pick rate in BGA meta | "Can create an insurmountable culture lead"; fine with Iron + Masonry to spam 4–5 wonders |
| Napoleon | "**Most important card in the game**" — https://boardgamegeek.com/thread/2393942 | "He's 4–6 strength and some MA. Autopick? I think not." — https://boardgamegeek.com/thread/1761996 |
| Player count | Most of this document | **2p is "a VASTLY different experience"** — repeated by multiple sources; treat 2p as a separate ruleset. The competitive BGA lobby is almost exclusively 2p, which biases much of the online consensus. |

---

## Appendix A — BGG forum sweep (independent research pass)

*Gathered by a separate research agent working only from BoardGameGeek and the Fandom MediaWiki API. Retained because it contains numbers not in the main body. Base-game filtered.*

**Turn 1 consensus, additional citations.** "Build a bronze mine as your first action in the game. Understand why everyone does this… on turn 1, each player can build exactly one infrastructure improvement" — https://boardgamegeek.com/thread/2695320. 30k-game data: "Go for Mine or Lab in Round 1… They are both better than all other choices, such as the 3rd Farm, or directly working on a Wonder" — https://boardgamegeek.com/thread/1933554.

**Age A leader tournament scores** (avg CA spent/game, 39 games): Hammurabi **0.36**, Homer **0.28**, Alexander **0.23**, Aristotle **0.08**, Julius Caesar **0.03**, Moses **0.03** — https://boardgamegeek.com/thread/2494200.

**Age I leader tournament scores:** Joan **0.33**, Barbarossa **0.15**, Genghis **0.08**, Leonardo **0.05**, Columbus **0.03**, Michelangelo **0.00**.

**Age A wonder tournament scores:** Library of Alexandria **0.31**, Pyramids **0.23**, Colossus **0.13**, Hanging Gardens **0.03**.
**Age I wonder tournament scores:** Great Wall **0.59**, St. Peter's **0.49**, Universitas Carolina **0.28**, Taj Mahal **0.00**.

**Rock production benchmarks:** end of Age I = 3 acceptable with Iron/Leonardo; end of Age II = "+5 or better"; Age III = "+6 or better is optimal." Breakpoints +3 and +5 per turn correspond to "an Urban Building/Knight/Swordsman every turn" and "an Age II Military every turn." — https://boardgamegeek.com/thread/2097526

**Iron total cost: 5 CA, 5 science, 9 rocks; Bronze→Iron payback = 3 turns.** — https://boardgamegeek.com/thread/2258467

**MasN's Age I science scale:** 1 = "a way to throw the game"; 2 = struggle; 3 = no panic; **4 = good**; 5 = a lot; 6 = diminishing; 7 = over-invested. — https://boardgamegeek.com/thread/2258467

**Endgame military math:** best rate 1.33–1.6 strength/rock; Age II tactic + Air Forces ≈ 6 strength/pop; Age III tactic ≈ 9 strength/pop; ~65 strength needs ~40 rocks in units. Classic Army + Air Forces = 29 str/army at 19 rocks. — https://boardgamegeek.com/thread/2352577

**Revolution arithmetic:** peaceful Monarchy = 1 CA + 8 science; revolution = all 4 CA + 2 science + likely 2 rocks corruption → "**4 more CA to save 6 science**." As a rate, 1.5 science/CA (Monarchy) or 1.66 science/CA (Theocracy). — https://boardgamegeek.com/thread/2732988, https://boardgamegeek.com/thread/2166558

**CA targets by age:** Gripperas 4/5/6.5/8; Ranior's actual 4p averages 4/4.75/~6/~6.5; floors "5th in Age I, 6th in Age II." — https://boardgamegeek.com/thread/3292858

**Game length:** Age I ends turn 7; Age II turn 13 (sometimes 12 or 14); Age III turn 18 (range 16–20). Median 19. — https://boardgamegeek.com/thread/2695320, https://boardgamegeek.com/thread/2766920

**Card row:** 76% of Age I picks at 1 CA, 2.5% at 3 CA (39 tournament games). 4p regular: <1 three-CA pick per game in Age I; 1.5–2 two-CA picks. — https://boardgamegeek.com/thread/2494200, https://boardgamegeek.com/thread/3292858

**Great Wall opportunity cost:** over 10 turns you lose **10 culture** vs St. Peter's and **20** vs Taj Mahal; the strength bonus "in practice works out usually to about 4 strength." — https://boardgamegeek.com/thread/2393942

**Eiffel Tower late-build value:** built with 3 turns to go = 12 culture; midway through Age II ≈ 36 culture (range 32–40). — https://boardgamegeek.com/thread/2393942, https://boardgamegeek.com/blog/9362/blogpost/97753/

**Age I wonder culture value:** many produce 2 culture/turn = up to 30 culture across the game if completed turn 5 of a 19-round game; usually at least 20. Age A culture wonders ~14–17. — https://boardgamegeek.com/thread/2494200

**Yellow-cube model:** every 4 yellow cubes reduces growth cost and consumption by 1; every 2 reduces required happiness by 1. "The first yellow cube in a bin is worth the most; the second is worth nothing." — https://boardgamegeek.com/thread/2494200

**Theology selection rate in 39 tournament games: zero.** — https://boardgamegeek.com/thread/2494200

**Additional threads worth mining later:** https://boardgamegeek.com/thread/2424523 (military system deep-dive), https://boardgamegeek.com/thread/1866366 (military floors by age), https://boardgamegeek.com/thread/2287034 (hand management), https://boardgamegeek.com/thread/2724657 (opportunity vs consolidation phases), https://boardgamegeek.com/thread/2482857 (singletons), https://boardgamegeek.com/thread/2957812 (3-farm opening).

## Appendix B — Reddit + champion interviews (independent research pass)

*Gathered by a separate research agent via `old.reddit.com` HTML over curl (reddit.com is blocked to the normal crawler). Retained separately because provenance and reliability differ from the main body. Base-game filtered.*

**Champion interviews.**
- frotes, 2× International Champion: "**military strength might not win you the game but it will definitely lose you the game if you neglect it**"; "Oftentimes players try to go for the **flashy or win-more combos instead of going for solid play**"; "some people play too many events and are **too optimistic about resolving them**." — https://old.reddit.com/r/throughtheages/comments/i5kmtj/interview_with_frotes_the_winner_of_international/
- Palino, Intermezzo winner: "**What I often see is that players rely on getting one card. You need to keep your options open.** Betting on one card may make sense only if you are too far behind"; "I often see players ending up in a situation when they **cannot efficiently use their CAs or resources**… This especially happens in age I"; "**That one or two culture points are usually not worth it** if there is high risk that the event will impact you negatively"; "Colonies can be very tricky, **betting too little or too much can lose you the game**." — https://old.reddit.com/r/throughtheages/comments/mktq48/

**Broad beginner tips thread** — https://old.reddit.com/r/throughtheages/comments/hdspay/8_very_broad_strategy_tips_for_new_players/
- "First turn mine pop, second turn lab pop, then think."
- **War over Culture swing formula: "X disadvantage in strength makes you lose 2X + 10 culture advantage to the winner"** (culture is taken from you and given to them). A real example cited a **71-culture swing = 142-point differential**.
- "**Don't hoard technology cards that you aren't going to use soon.** Taking a card costs civil actions, so that's likely a waste… until half age II, any turn that a technology is in your hand means you haven't ever had those CAs that you used to get it."
- "**Watch what cards you leave for the next player.** Do you give him one turn OP combo? Look at his civilization, is there a card he might desperately need but you wouldn't mind having too, even if it costs 2?"
- "**Watch out when the game will end.** Sometimes you can accelerate game ending by taking more cards, so that some players don't have a chance to declare that final war on you."
- "Plan ahead for 1–3 turns in advance. Think chess."
- "**Don't seed events just to seed them**… be very careful about seeding an event if you are the weakest."
- Digital-play meta-tip: skip the political phase, resolve your whole action phase, then undo back and choose the political action last.

**When to pay 3 CA** — https://old.reddit.com/r/throughtheages/comments/713x49/daily_strategy_topic_when_do_you_pay_three_civil/
- "I tend **not to use 3 civil actions until I have 5 or 6 available**. Burning 3 actions when you only have 4 is very painful and will likely cause corruption."
- Age III: "this becomes much more common as I aim to have **at least 6–7 civil actions** by then… Age III wonders can easily be 20–30 points."
- "If I can take and play Constitutional Monarchy for a net of at least 0 civil actions this turn, it's kind of a no-brainer."

**Age I strategy discussion** — https://old.reddit.com/r/throughtheages/comments/difwir/age_i_strategy_discussion/
- "I generally skip Age I government and save science for Age II government. **In 4p game there is enough Age II government for everyone** so it's quite safe to wait." (level-44 player)
- "**Theocracy is mediocre since it doesn't give you CAs.**"
- "Getting an age I aggression off is super hard, **I wouldn't recommend trying too hard for it**."
- "**3 Bronze mines is enough before I need Coal**" if you build only what you need in Age I.
- Colony valuation: "early (Age I plus half of Age II), a colony is **as good as the number of yellow tokens it gives you. None of the other crap matters**." "Don't make a bid that makes you vulnerable to aggressions."
- Dissent on early culture: "I find it hard to catch up if you totally ignore culture production in Age I. I will find the way to have my culture generated for about **+2 to +4 per turn**… Temple + Drama + Wonder(s) give me about 4–5 culture per turn."
- Wonder-loss hedging: "don't take *only* an Age I wonder — when it suffers Ravages you will be ultra sad. **Either both, neither, or only Age A are fine.**"

**Iron vs Irrigation** — https://old.reddit.com/r/throughtheages/comments/gz3o41/value_of_ironirrigation/
- Contrarian pricing: "iron would be worth **4.5 science**, whereas irrigation would be worth **3.5**."
- Pro-Irrigation: "In general I consider Iron pretty mediocre and Irrigation very good. **With an extra civil action or two you can substitute action cards for rocks much more easily than for food.**"
- Anti-Rats argument for Irrigation: "Rats set everyone's food stock to zero so those who have Irrigation can increase population quicker next turns."
- "**Card denial is really only important on farms, mines, and governments**" — in 3p there are 4 Age II governments so you can't be denied a government, only stuck with Republic, whereas Iron/Irrigation *can* be denied.
- Civil Service math: best case Age III round ~14 for 1 CA → 1 extra CA for 4 rounds = "**adding 14% to your CA ability for four turns**" if you already have Code of Laws and 7 CA.

**Age III balance checklist** — https://old.reddit.com/r/throughtheages/comments/gj02nk/start_of_age_iii_am_i_balanced/
> A) **6 civil actions** B) **4 military actions** C) **6 resources** D) **4 sciences**
> Plus: you must be able to raise or disband a worker without sacrificing more than 2 production; keep an idle/low-level worker available.
> "I usually choose my path after watching my **first 3 politic age III cards**. If you are playing an aggressive game, building a culture engine prematurely might end up getting overrun."

Higher science target from another player: "**Science is king in age 3. You should be producing 10+** as a goal during that age so that you can develop a tech each turn if possible." — https://old.reddit.com/r/throughtheages/comments/gn3abv/help_a_beginner/

**Death-spiral prophylaxis** — https://old.reddit.com/r/throughtheages/comments/1mjuimv/
The classic unrecoverable position is Age II transition + Pestilence + Rats → forced disband. Stated prophylaxis: **always keep one idle worker** (protects vs Pestilence) and **keep food stock low by growing every turn** (protects vs Rats).

**2p-specific tier claims** — https://old.reddit.com/r/throughtheages/comments/qql972/1v1_skills/
- "**3 science production is enough in AGE I and AGE II**."
- "**Iron is not worth 3 civil actions in most situations**; Code of Laws is worth 3 CA sometimes."
- "**Bach is Tier 1**; Bach and Shakespeare are even worth 3 civil actions."
- Government ordering in 2p: **ConMon > Theocracy > Monarchy > Republic** — "Republic is weak because it just has 2 MA."
- "**1–2 colonies are enough**" (without Suez Canal, which is expansion).
- "If you wanna win a game by pure military strategy, you should take aggressions and declare wars at the **end of Age II or the beginning of Age III**. Building an Age III army is too late."
- "The best way to get happy faces is by **leaders and wonders or theaters**."

**Anti-bot script (digital Hard AI, 2p)** — https://old.reddit.com/r/throughtheages/comments/eviw8c/tutorial_beating_hard_ai_on_a_two_player_game_the/
Homer + Great Wall → yellow cards over urban buildings → skip the second Philosophy in favour of Library/Universitas → Napoleon + Cannon + Strategy in Age II → Air Forces + multiple armies + repeated War over Culture in Age III. Also: "against AI, especially the easier ones… **they play a very specific way which you can learn and exploit**."

**P1 turn-1 probability note** — https://old.reddit.com/r/throughtheages/comments/iy53k5/
With 9 leaders/wonders and 7 open slots there's a **~50% chance of missing one of 5 wonders and an 8.3% chance of missing 2**.

**Age A wonder ranking (3/4p reddit tier list)** — https://old.reddit.com/r/throughtheages/comments/gylz9a/
Library (S) > Pyramids (A) > Colossus (C) > Hanging Gardens ("Do not play tier"). Quantified: Library ≈ 14–16 science+culture over the game; Pyramids ≈ 14–16 extra civil actions, "early on having **20% more civil actions** than your opponents can be incredibly valuable."

**4p offensive-style Age A ranking** (inverts the tournament data on Homer) — https://old.reddit.com/r/throughtheages/comments/g8oi4b/
Strong: Hammurabi, Caesar (as P1). Medium: Aristotle, Alexander. Weak: **Moses, Homer** — "Homer… doesn't provide any bonus right away. **Happiness is useless at the start.**" Note the source's own caveat that **Caesar is strong as P1 and weak as P4**, and Ashoka is the reverse — leader value is turn-order dependent.

**2017 base-game-era Age A ordering** — https://old.reddit.com/r/throughtheages/comments/70xz0i/daily_strategy_topic_age_a_leaders/
"Aristotle > Alexander = Hammurabi = Homer > Moses = Caesar."

---

## Local source files used

- `sources/bga_card_counts.tsv` — authoritative base-game card list with costs (extracted from the BGA implementation)
- `sources/ubg_player-areas.txt`, `sources/ubg_subsequent-rounds.txt` — exact rules thresholds (16 blue tokens, ≥11 to avoid corruption; 18 yellow bank; 7 starting workers; uprising condition; happiness subsection trigger)
- `sources/namu_events.txt` — full Age I/II/III event and Impact list with exact payouts
- `sources/namu_military.txt` — aggression/war/pact cards with exact costs and payouts
- `sources/namu_wonders.txt`, `sources/namu_gov.txt`, `sources/namu_urban.txt` — Korean-community card evaluations (independent of the English forums)
- `sources/hypercheat.txt` — rules summary and endgame scoring rates
- `sources/gamefaqs_75690.txt` — **NOT usable**; contains only a Cloudflare interstitial
