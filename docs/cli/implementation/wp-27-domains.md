# WP-27 domain taxonomy `domains-v1`

Draft 2026-10-24, for review before step 1 of [WP-27](wp-27.md) turns it into `resources/taxonomy/domains-v1.json`.

## Sources

| Tree | Source | What it covers |
|---|---|---|
| A. Everyday life | Council of Europe, J. A. van Ek and J. L. M. Trim, *Threshold 1990* (1991), chapter 6 "Specific notions": themes 1–14 and their sub-themes. The CEFR (2001, §4.1) builds its domains and themes on it. | Words a learner meets in daily life. |
| B. Specialist fields | UNESCO Institute for Statistics, *ISCED Fields of Education and Training 2013 (ISCED-F 2013)*: broad (2-digit), narrow (3-digit) and detailed (4-digit) fields. Field 00 (generic programmes) is left out. | Technical and academic vocabulary. |
| C. General notions | *Threshold 1990*, chapter 7 "General notions": existential, spatial, temporal, quantitative, qualitative, mental and relational notions. | Abstract words, function words and the meaning of grammar patterns. |

Tree A's level 3 and the split of two themes are crafted for this deck:
- "at work" from theme 3 is its own level-1 node, matching the CEFR occupational domain;
- flora and fauna from theme 2 join weather (theme 14) as `nature`.

Tree B keeps ISCED-F's three levels and codes; only the labels are shortened. Tree C uses the *Threshold* categories as levels 1 and 2.

## Rules for the LLM

1. Choose one **primary** leaf path and at most two **secondary** leaf paths. Every path has exactly three levels: `l1::l2::l3`.
2. Judge the **selected sense** shown on the card, not every sense of the word.
3. **Everyday or specialist:**
   - a word a layperson uses in daily life goes to tree A, even when it names a technical thing (風邪, 薬, パソコン);
   - a term used mainly by specialists, or labelled with a field by the dictionary, goes to tree B (心筋梗塞, 判例, 微分);
   - a word that fits both may take tree A as primary and tree B as secondary.
4. **Abstract words, function words and grammar patterns** go to tree C (それに, however, importance, 〜たら → `relational::logic::condition`), unless their meaning is a concrete theme.
5. The **hints** (JMdict field label, Cambridge SMART Vocabulary topic) describe the dictionary entry. Follow them unless the selected sense is clearly elsewhere.

## A. Everyday life

Each level-2 line lists its level-3 leaves.

### `person` — personal identity (Threshold theme 1)
Who someone is, how they look, feel and behave.
- `identity`: names-titles, age-birthday, nationality-origin, personal-documents
- `appearance`: build-height, face-features, hair, looks-impression
- `character`: personality-positive, personality-negative, temperament, values-beliefs
- `feelings`: joy, sadness, anger, fear-anxiety, surprise, love-affection, shame-embarrassment, gratitude-relief
- `likes`: likes-dislikes, preferences, wishes-dreams
- `life-stages`: birth-childhood, youth, adulthood, old-age, death

### `home` — house, home and surroundings (theme 2)
- `dwelling`: house-types, rooms, parts-of-building, moving-renting
- `furniture`: furniture, bedding-textiles, fixtures-fittings
- `household-items`: kitchenware, appliances, tools-diy, containers-storage
- `housework`: cleaning, laundry, tidying, home-repairs
- `utilities`: water, electricity-gas, heating-cooling, rubbish-recycling
- `surroundings`: neighbours, local-area, town-country, garden

### `daily` — daily life (theme 3)
- `routine`: waking-sleeping, washing-dressing, mealtimes, commuting
- `time-calendar`: clock-time, days-dates, calendar-events, schedules-appointments
- `planning`: plans-arrangements, punctuality-delay, busy-free, habits

### `work` — work and occupations (theme 3 "at work"; CEFR occupational domain)
- `jobs`: occupations, job-hunting, employment-contracts, retirement
- `workplace`: office, colleagues-hierarchy, meetings, business-trips
- `tasks`: office-tasks, documents-forms, deadlines, performance-evaluation
- `conditions`: wages-salary, working-hours, leave, labour-issues

### `leisure` — free time and entertainment (theme 4)
- `hobbies`: collecting, crafts, games-puzzles, outdoor-activities
- `sport`: ball-sports, martial-arts-sumo, athletics-fitness, water-winter-sports, watching-sport
- `media`: tv-radio, press-magazines, internet-social-media, film
- `performing`: music, theatre-stage, concerts-festivals, dance
- `culture`: museums-exhibitions, books-reading, visual-arts, traditional-arts
- `celebrations`: parties, festivals, seasonal-customs, gifts

### `travel` — travel and transport (theme 5)
- `public-transport`: train-rail, bus, air, ship-ferry, tickets-fares
- `private-transport`: car-driving, bicycle, walking, taxi
- `traffic`: roads, traffic-rules, traffic-accidents, parking
- `holidays`: sightseeing, accommodation, luggage-packing, trip-planning
- `abroad`: passport-visa, customs-immigration, currency-exchange, living-abroad

### `relations` — relations with other people (theme 6)
- `family`: parents-children, siblings, extended-family, marriage-divorce
- `friends`: friendship, quarrels-reconciliation, love-dating, acquaintances
- `social-contact`: invitations-visits, greetings-manners, gifts-thanks, correspondence
- `community`: clubs-groups, volunteering, social-roles, neighbourhood-life
- `society`: politics-opinions, social-issues, customs-traditions, religion-everyday

### `health` — health and body care (theme 7)
- `body`: body-parts, internal-organs, senses, body-positions
- `illness`: common-illness, injuries, symptoms, chronic-conditions
- `medical-care`: doctor-hospital, medicine-pharmacy, dentist, health-insurance
- `wellbeing`: diet-exercise, sleep-rest, stress-mental-health, hygiene-grooming
- `emergency`: emergency-services, first-aid, disasters-safety

### `school` — education and learning (theme 8; CEFR educational domain)
- `schooling`: school-types, school-life, university, exams-qualifications
- `subjects`: language-subjects, maths-science-subjects, humanities-subjects, arts-pe
- `study`: study-methods, homework-assignments, reading-writing, grades-results
- `classroom`: classroom-objects, teachers-students, lessons-timetable, rules-discipline

### `shopping` — shopping and money (theme 9)
- `shops`: shop-types, markets, online-shopping, opening-hours
- `buying`: prices-discounts, payment, returns-complaints, receipts-wrapping
- `goods`: clothes-fashion, shoes-accessories, household-goods, electronics
- `money`: cash-coins, saving-spending, everyday-banking, debts-loans
- `measures`: weights-sizes, quantities-packaging, quality

### `food` — food and drink (theme 10)
- `foodstuffs`: vegetables-fruit, meat-fish, grains-staples, dairy-eggs, seasonings
- `dishes`: japanese-dishes, western-dishes, noodles-rice-dishes, snacks-sweets
- `drinks`: soft-drinks, tea-coffee, alcohol, water
- `cooking`: preparation, cooking-methods, recipes-measuring, taste-texture
- `meals`: breakfast, lunch, dinner, diet-nutrition
- `eating-out`: restaurants, ordering-paying, table-manners, takeaway-delivery

### `services` — public and commercial services (theme 11)
- `post-telecom`: post, telephone, internet-service, parcel-delivery
- `public-office`: city-office, registration-documents, police, embassy
- `repairs`: repair-shops, garage-petrol, utility-service, cleaning-services
- `personal-services`: hairdresser-beauty, laundry-service, childcare, elderly-care

### `places` — places and directions (theme 12)
- `buildings`: public-buildings, commercial-buildings, religious-buildings, landmarks
- `town`: streets-blocks, stations-stops, parks-squares, districts
- `geography`: countries, regions-prefectures, cities, natural-features
- `directions`: asking-directions, maps-addresses, orientation, distance-routes

### `language` — language and communication (theme 13)
- `languages`: language-names, language-learning, dialects, writing-systems
- `communication`: conversation, listening-understanding, misunderstanding-repair, politeness-honorifics
- `writing`: letters-email, spelling-script, punctuation, texts-documents
- `words`: vocabulary-meaning, grammar-terms, translation, idioms-sayings

### `nature` — weather, climate and nature (themes 14 and 2)
- `weather`: sun-clouds, rain-snow, wind-storms, temperature
- `climate`: seasons, climate-types, forecasts, natural-disasters
- `animals`: pets, wild-animals, birds, insects, sea-life
- `plants`: flowers, trees, garden-plants, wild-plants
- `landscape`: mountains, rivers-lakes, sea-coast, sky-space

## B. Specialist fields (ISCED-F 2013)

The ISCED-F code is in brackets.

### `education-science` — education (01)
- `education` (011): education-science (0111), preschool-teaching (0112), teacher-training (0113), subject-teaching (0114)

### `arts-humanities` — arts and humanities (02)
- `arts` (021): media-production (0211), design (0212), fine-arts (0213), handicrafts (0214), music-performing-arts (0215)
- `humanities` (022): religion-theology (0221), history-archaeology (0222), philosophy-ethics (0223)
- `languages` (023): language-acquisition (0231), literature-linguistics (0232)

### `social-sciences` — social sciences, journalism and information (03)
- `social-behavioural` (031): economics (0311), political-science (0312), psychology (0313), sociology-cultural-studies (0314)
- `journalism-information` (032): journalism (0321), library-archives (0322)

### `business-law` — business, administration and law (04)
- `business-admin` (041): accounting-tax (0411), finance-banking-insurance (0412), management (0413), marketing-advertising (0414), office-work (0415), sales-retail (0416), work-skills (0417)
- `law` (042): law (0421)

### `natural-sciences` — natural sciences, mathematics and statistics (05)
- `biology` (051): biology (0511), biochemistry (0512)
- `environment` (052): environmental-science (0521), wildlife-natural-environments (0522)
- `physical-sciences` (053): chemistry (0531), earth-sciences (0532), physics (0533, includes astronomy)
- `mathematics-statistics` (054): mathematics (0541), statistics (0542)

### `ict` — information and communication technologies (06)
- `ict` (061): computer-use (0611), databases-networks (0612), software-development (0613)

### `engineering` — engineering, manufacturing and construction (07)
- `engineering-trades` (071): chemical-engineering (0711), environmental-technology (0712), electricity-energy (0713), electronics-automation (0714), mechanics-metal (0715), vehicles-ships-aircraft (0716)
- `manufacturing` (072): food-processing (0721), materials (0722), textiles-clothing (0723), mining (0724)
- `architecture-construction` (073): architecture-planning (0731), building-civil-engineering (0732)

### `agriculture` — agriculture, forestry, fisheries and veterinary (08)
- `agriculture` (081): crops-livestock (0811), horticulture (0812)
- `forestry` (082): forestry (0821)
- `fisheries` (083): fisheries (0831)
- `veterinary` (084): veterinary (0841)

### `health-welfare` — health and welfare (09)
- `health` (091): dentistry (0911), medicine (0912), nursing-midwifery (0913), diagnostics-treatment-technology (0914), therapy-rehabilitation (0915), pharmacy (0916), traditional-medicine (0917)
- `welfare` (092): elderly-disabled-care (0921), childcare-youth (0922), social-work-counselling (0923)

### `service-sector` — services (10)
- `personal-services` (101): domestic-services (1011), hair-beauty (1012), hospitality-catering (1013), sports (1014), travel-tourism-leisure (1015)
- `hygiene-safety` (102): sanitation (1021), occupational-safety (1022)
- `security` (103): military-defence (1031), protection-policing (1032)
- `transport` (104): transport-services (1041)

## C. General notions (Threshold 1990, chapter 7)

### `existential` — existence and occurrence
- `being`: existence, presence-absence, availability
- `happening`: occurrence, change-of-state, possibility-impossibility

### `spatial` — space
- `location`: position, here-there, inside-outside
- `relative-position`: above-below, front-back, beside-between
- `size-distance`: distance, dimensions-size, length-width-height
- `motion`: movement, direction, origin-destination, arrangement-order

### `temporal` — time
- `time-points`: points-of-time, divisions-of-time, earliness-lateness
- `time-relations`: before-after, sequence, simultaneity
- `time-reference`: past, present, future
- `aspect`: duration, frequency, continuity-repetition, beginning-ending, change-permanence, speed

### `quantitative` — quantity
- `number`: counting, ordinal-rank, fractions-ratio
- `quantity`: amount, increase-decrease, more-less, degree-intensity

### `qualitative` — qualities
- `physical`: shape, colour, texture-material, taste-smell, sound-light, temperature-moisture, age-newness, condition-cleanliness
- `evaluative`: value-price, good-bad, correctness, importance, normality-strangeness, ease-difficulty, desirability, acceptability

### `mental` — thinking and expressing
- `reflection`: thinking-belief, knowledge-memory, opinion-judgement, expectation-intention, certainty-doubt
- `expression`: saying-telling, asking-answering, describing-explaining

### `relational` — relations
- `action-event`: agent, recipient-beneficiary, instrument-means, cause-effect, purpose
- `comparison`: sameness, similarity-difference, comparative-superlative, correspondence
- `possession`: ownership, giving-receiving, belonging
- `logic`: addition, alternatives, contrast-concession, condition, reason, result, inclusion-exclusion, focus-emphasis, examples-listing
- `reference`: demonstratives-deixis, persons-pronouns, question-words

## Communicative functions (grammar only)

This is a closed list for `lab::function::<group>::<function>`, from *Threshold 1990*, chapter 5 "Language functions". A grammar note takes 1–2 functions. Meanings such as condition or sequence are notions (tree C), not functions.

- `information`: identifying, reporting-describing, correcting, asking
- `attitudes`: agreement-disagreement, knowledge, certainty-probability, necessity-obligation, ability, permission-prohibition, wants-desires, intention, preference, emotions, apology, approval-disapproval, regret, gratitude
- `suasion`: suggesting, requesting, advising, warning, encouraging, instructing, inviting, offering, asking-permission
- `socialising`: greeting, introducing, attracting-attention, congratulating, leave-taking, addressing
- `discourse`: opening, enumerating-sequencing, exemplifying, emphasising, summarising, changing-topic, hesitating, quoting-reported-speech, closing
- `repair`: checking-understanding, asking-repetition, spelling-out, paraphrasing

## Hints

JMdict field labels (as Jisho shows them) are mapped to one leaf each. The mapping is given to the LLM as a hint; it is not a decision. A Cambridge SMART Vocabulary topic is given verbatim, with no mapping.

| JMdict label | Hint |
|---|---|
| Computing, Internet | `ict::ict::computer-use` |
| Medicine, Anatomy, Psychiatry, Physiology | `health-welfare::health::medicine` |
| Pharmacology | `health-welfare::health::pharmacy` |
| Dentistry | `health-welfare::health::dentistry` |
| Law | `business-law::law::law` |
| Finance, Business | `business-law::business-admin::finance-banking-insurance` |
| Economics | `social-sciences::social-behavioural::economics` |
| Politics | `social-sciences::social-behavioural::political-science` |
| Psychology | `social-sciences::social-behavioural::psychology` |
| Mathematics, Geometry | `natural-sciences::mathematics-statistics::mathematics` |
| Statistics | `natural-sciences::mathematics-statistics::statistics` |
| Physics, Astronomy | `natural-sciences::physical-sciences::physics` |
| Chemistry | `natural-sciences::physical-sciences::chemistry` |
| Geology, Meteorology | `natural-sciences::physical-sciences::earth-sciences` |
| Biology, Botany, Zoology, Genetics | `natural-sciences::biology::biology` |
| Biochemistry | `natural-sciences::biology::biochemistry` |
| Linguistics, Grammar | `arts-humanities::languages::literature-linguistics` |
| Buddhism, Shinto, Christianity | `arts-humanities::humanities::religion-theology` |
| History, Archeology | `arts-humanities::humanities::history-archaeology` |
| Philosophy | `arts-humanities::humanities::philosophy-ethics` |
| Art, aesthetics | `arts-humanities::arts::fine-arts` |
| Photography, Film | `arts-humanities::arts::media-production` |
| Music | `leisure::performing::music` |
| Food, cooking | `food::cooking::cooking-methods` |
| Sumo, Martial arts | `leisure::sport::martial-arts-sumo` |
| Baseball, Sports, Soccer, Golf, Tennis | `leisure::sport::ball-sports` |
| Go (game), Shogi, Mahjong, Card games, Hanafuda | `leisure::hobbies::games-puzzles` |
| Military | `service-sector::security::military-defence` |
| Architecture | `engineering::architecture-construction::architecture-planning` |
| Civil engineering, Construction | `engineering::architecture-construction::building-civil-engineering` |
| Electricity, Electronics | `engineering::engineering-trades::electronics-automation` |
| Engineering, Mechanical engineering | `engineering::engineering-trades::mechanics-metal` |
| Agriculture | `agriculture::agriculture::crops-livestock` |
| Fishing | `agriculture::fisheries::fisheries` |
| Veterinary terms | `agriculture::veterinary::veterinary` |
| Railway | `travel::public-transport::train-rail` |
| Aviation | `travel::public-transport::air` |
| Automobile | `travel::private-transport::car-driving` |
| Clothing | `shopping::goods::clothes-fashion` |
| Television | `leisure::media::tv-radio` |

Step 1 checks this table against the 723 labels in the store and adds a row for every field label that is still missing.

## Size

| Tree | Level 1 | Level 2 | Level 3 leaves |
|---|---|---|---|
| A. Everyday life | 15 | 72 | 293 |
| B. Specialist fields | 10 | 26 | 77 |
| C. General notions | 7 | 21 | 89 |

459 leaves in all. ISCED-F has 80 detailed fields; the 3 of field 00 are left out. The step 1 test checks these counts.
