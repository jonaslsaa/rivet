# rivet-reference-oracle

A small JSON Lines CLI that exposes deterministic behavior from the pinned
Paper 26.2 Java implementation. Rivet tests can send the same input to this
process and the Rust port, then compare structured responses instead of
hand-writing expected values.

The tool currently supports:

- `ping` — verify the process and protocol version.
- `snbt.parse` — parse any SNBT tag and return compact and pretty canonical
  forms plus its Java tag type.
- `nbt.encode` — parse a compound SNBT root and return its uncompressed binary
  NBT representation as base64.
- `nbt.decode` — decode uncompressed compound NBT from base64 and return its
  canonical SNBT representation.
- `component.json` — decode a component JSON string through
  `ComponentSerialization.CODEC` under non-compressed `JsonOps` and re-encode
  it: `accept` is whether the input decodes at all, and `canonical` is the
  exact JSON the codec re-emits (a chat/title/player-info/scoreboard wire
  form). Malformed input (invalid JSON or an undecodable component) returns
  `accept:false`. Consumed by the committed issue-#98 text corpus
  (`tools/rivet-oracle/fixtures/text/`) and the `rivet-parity` differential
  checks.

- `codec.reencode` — `{codec, ops, input}`: decode `input` with a named vanilla
  `Codec` constant (`Class.CONSTANT`, the keys of `CodecTable`) under `RegistryOps`
  over the frozen vanilla server registries (`ops` is `json`, input a JSON value,
  or `nbt`, input SNBT text) and re-encode. Answers `{accept:true, canonical}`
  (the encoder's compact JSON/SNBT) or `{accept:false, phase:"decode"|"encode"}`.
- `packet.reencode` — `{state, direction, packet_id, body}`: look the packet up in
  `ProtocolInfo`, decode the hex body, re-encode and answer `{name, body}`; an
  unregistered id, a decode failure or a `DecoderException` is an input
  rejection. Paper's `GlobalConfiguration` is initialised with defaults first.
- `brigadier.dispatch` (`BrigadierDispatchOp.java`) — `{spec, input, cursor, want}`:
  build a `CommandDispatcher<String>` from a declarative JSON spec (literals,
  arguments with Brigadier's built-in types and bounds, `requires` as a fixed
  boolean, `executes` result codes, redirect/fork/forward with fixed modifiers,
  fixed suggestion providers), then answer the requested facets for `input`:
  `run` (parse result, execute result or exception, result-consumer calls),
  `suggestions` (at `cursor`), `usage` (`getAllUsage`/`getSmartUsage` of the node
  at `usage_path`) and `ambiguities` (`findAmbiguities`). Brigadier's own
  `CommandSyntaxException`s are data in the result, so the op never rejects; the
  message text is Paper's runtime one (`SharedConstants` installs
  `net.minecraft.commands.BrigadierExceptions`).

- `damage.calc`, `damage_source.scaling`, `food.exhaustion`, `xp.curves` — the pure
  entity ops (VERIFICATION item 9), implemented in `src/EntityOps.java` (every `src/*.java`
  is compiled and enters the one op source SHA-256 the manifest pins). Floats travel as
  `0x`+8 hex digits in requests and results. `damage.calc`: `{fn:"after_absorb", damage,
  armor, toughness}` and `{fn:"after_magic_absorb", damage, protection}` run
  `CombatRules` (no weapon, so no victim is read), `{fn:"type", damage_type}` answers the
  vanilla damage type definition and its tags. The other three run the real Paper
  methods (`Player.hurtServer` scaling, `FoodData.tick/eat/addExhaustion`,
  `ServerPlayer.giveExperiencePoints/Levels`) on entities allocated without their
  constructor; the exact harness contract (what the level, health, events and sounds
  answer) is the class comment of `EntityOps.java`, and the Rust twin in
  `tools/rivet-parity/src/ops/entity.rs` reproduces it.

- The gameplay-data ops (`GameplayOps.java`, `GameplayData.java`; VERIFICATION
  section 5 item 8), each swept over the whole vanilla registry it names:
  - `recipe.match {recipe, kind, ...}` — `Recipe.matches(input, level)` and the
    assembled stack for one recipe of the data pack and one input (`crafting
    {width,height,grid}`, `single {input}`, `smithing {template,base,addition}`;
    a cell is `""` or `<item id>[#<count>]`). Answers `{match, result?, threw?}`
    with `result = {item, count, components}` (component-patch id to its value, encoded by the component's codec under `JsonOps`, in canonical JSON).
  - `loot.roll {table, seed, luck, entity, block}` — `LootTable.getRandomItems(params, seed)`
    over fixed `LootParams` built from the table's own context-key set (below).
    Answers the ordered stacks as SNBT (`items`), or `threw`.
  - `enchant.select {kind, ...}` — `cost {enchantment, level}` (min/max cost),
    `slot_cost {item, enchantable, slot, bookcases, seed}`
    (`EnchantmentHelper.getEnchantmentCost`) and `select {item, enchantable, power,
    seed}` (`selectEnchantment` over `#in_enchanting_table`). `enchantable` is the
    stack's enchantability, an input.
  - `effect {effect, amplifier}` — `MobEffect.createModifiers`, category and colour.
  - `trade.offers {trade_set, merchant, variant, seed}` — the offers of a trade set
    for a fresh merchant (the selection loops of `AbstractVillager.addOffersFrom*`
    over `VillagerTrade.getOffer(LootContext)`), as `MerchantOffer.CODEC` SNBT.
  - `scoreboard.saved_data {input}` — `ScoreboardSaveData.TYPE.codec()` round trip
    and the encoding after loading into a scoreboard; compound keys sorted in the
    canonical SNBT (D12), and one holder's `PlayerScores` entries sorted by objective
    (Paper iterates them by `Objective` identity hash, a different order on every JVM start).

  **The live-server context.** There is no running `MinecraftServer`. The reloadable
  data (loot tables, recipes) loads the way `ReloadableServerResources` loads it
  (`ReloadableServerRegistries.reload`, `RecipeManager.prepare`) over the vanilla
  trusted pack repository, and the `ServerLevel` an op needs is `GameplayData.LevelShell`:
  an unconstructed level (allocated with `Unsafe`, no constructor run) over the frozen
  registries, with game time 0, NORMAL difficulty, Paper's world configuration populated
  with its defaults, an empty `EnvironmentAttributeSystem` (every attribute answers its
  default) and a server shell whose only live part is the reloadable registries. Entities
  (`THIS_ENTITY`, the villager of a trade) are real `EntityType.create` instances in that
  level. `loot.roll` parameters: ORIGIN (0.5, 64, 0.5); luck from the request (1.0); TOOL
  a diamond pickaxe (`fishing`: fishing rod, `shearing`: shears, `archaeology`: brush);
  BLOCK_STATE the default state of the request's `block`; THIS_ENTITY/TARGET_ENTITY, only
  where the set requires them, a fresh `entity`; DAMAGE_SOURCE `generic` with no attacker.
  No killer player, block entity or explosion radius is supplied, so `killed_by_player`
  and the survives-explosion radius take their absent-parameter branch. The recorded
  gaps of that shell (a roll that throws inside Paper because the level has no chunk
  source, structure search, Bukkit server, raids or scoreboard, or an entity it cannot
  create) are `threw` answers, listed in `rivet-parity`'s tests.

  `harvest_gameplay.py` extracts the recipe documents, resolved item tags, loot table
  ids and trade set ids from the Paper jar into `fixtures/ops/corpus-src/gameplay-data.json`
  and the `blocks/*` loot table documents into `corpus-src/block-loot-tables.json`
  (the Rust `loot.roll` handler evaluates those; every other table is `unavailable`);
  the corpus generators are `rivet-parity`'s `ops/gameplay/`.

- `feature.place` — `{feature, seed, region}`: place one configured feature of the
  vanilla registry into a fixed synthetic region (a 3 x 3 window of proto chunks
  behind a `WorldGenRegion`-like facade; the `region` profile names the base fill,
  origin, biome and sea level) with `XoroshiroRandomSource(seed)`. Answers
  `{placed, rng_next, scheduled, blocks[, threw]}`: the changed blocks (position and
  global state id, sorted), the ticks scheduled, the placement random's next long.
- `carver.carve` — `{carver, seed, index}`: carve a 3 x 3 chunk grid of stone with one
  configured carver the way `applyCarvers` loops, and answer per chunk the start and
  carve counts, the carving mask and the changed blocks as index runs.
- `structure.start` — `{structure_set, seed, dimension}`: scan the chunk grid
  [-48, 47]^2 of the dimension for the set's structure chunks (Paper's keyed,
  spigot-salted placements) and answer the candidates and the start of each: the
  pieces' type, bounding box, rotation, mirror and generation depth. No block is placed.
  The three `corpusCheck`s make `--record` fail unless the corpus covers every
  registry entry for every one of `WORLDGEN_SEEDS`; the region and grid contracts are
  on the Java ops and in `docs/VERIFICATION.md` section 5 item 5.
- `shapes.eval` — `{kind: vec3 | aabb | shape, ...}`: evaluate `Vec3`/`AABB` members
  (every double/float path, including `Math.round`, `atan2`/`asin` and the fuzzy
  compares) or build a `VoxelShape` from a spec (`Shapes.box/create/join/or`,
  `move`, `optimize`, `getFaceShape`, `Shapes.empty/block/INFINITY`) and answer a
  query (`collide`, `clip`, `closestPointTo`, `min/max`, `join`, `equal`,
  `occludes`) with a full shape summary (coords, voxels, AABBs, face shapes) as f64
  bit hex. Misses are absent keys (the canonical writer drops nulls); a
  `Shapes.joinUnoptimized` ambiguous operator is an input rejection.
  `--dump-vanilla-shapes <out.jsonl>` writes every distinct vanilla block-state
  collision/shape/occlusion/visual shape to `fixtures/ops/corpus-src/vanilla_shapes.jsonl`
  (the corpus's vanilla seeds).

Registry loading logs to stdout, so the first `codec.reencode`/`packet.reencode`
request silences log4j until the registries are ready (live mode speaks JSON
Lines on that stream). `harvest_datapack.py` extracts `codec.reencode` seeds
from the vanilla data pack in the Paper jar into `fixtures/ops/corpus-src/`; the same
script writes `worldgen-registries.json`, the key list of the configured features,
configured carvers and structure sets the worldgen sweep covers.

All twenty operations live in one op registry (`OPS` in
`RivetReferenceOracle.java`; an op family may keep its handlers in its own file next to it: `EntityOps.java`, `GameplayOps.java` and `BrigadierDispatchOp.java`). Each op declares its result-key order (the
canonical-JSON contract: results are re-emitted in exactly that order, an
undeclared key or a raw floating-point number is an `unavailable` fault — floats
travel as IEEE-754 bit hex via `f32Bits`/`f64Bits`, with human decimals only in
the recorded `dec` object) and which exception classes are input-rejection
verdicts. The same handlers serve two modes:

- **live** (default): one JVM boot answers any number of JSON Lines requests on
  stdin/stdout, for any mix of ops. The Rust client is
  `rivet_harness_common::oracle_client` (id correlation, pipelining, tamper
  hook).
- **record**: `run.sh --record <corpus-dir> <out-dir>` runs every
  `<corpus-dir>/<op>.jsonl` corpus (one `{"id":...,<request fields>}` per line)
  through Paper and writes `<out-dir>/<op>.jsonl` (one
  `{"id","ok":true,"result"[,"dec"]}` or `{"id","ok":false,"kind":"input_rejected","detail"}`
  line per input) plus a deterministic `manifest.json` (Paper pin and commit,
  op source SHA-256, per-op corpus SHA-256/count and output SHA-256/count). An
  input with no verdict aborts the recording.

The committed recordings are under `fixtures/ops/` (`corpus/`, `<op>.jsonl`,
`manifest.json`, and the comparator's `exclusions.tsv`). To re-record after
changing an op or a corpus:

```sh
cargo run -p rivet-parity -- ops emit-corpus tools/rivet-reference-oracle/fixtures/ops/corpus
tools/rivet-reference-oracle/run.sh --record tools/rivet-reference-oracle/fixtures/ops/corpus tools/rivet-reference-oracle/fixtures/ops
```

`cargo run -p rivet-parity -- ops check` (offline, also a cargo test) compares
the Rust handlers with the recordings (the worldgen sweeps are heavy: the default cargo tests replay a sample of each, and the
whole-corpus replays are `#[ignore]`d tests that `scripts/gate.sh` runs in release mode with
`--ignored --test-threads=1`); `ops live` streams the
corpora through a live oracle and also requires Paper to still reproduce the
recordings (the worldgen sweep ops compare Rust for the two seeds of
`corpus-src/worldgen-live-seeds.json` only);
`rivet-oracle regen --check --only rivet-reference-oracle` re-records into scratch twice and
byte-diffs (see `tools/rivet-oracle/README.md`). Editing any `src/*.java` file changes
the op source SHA-256 the manifest pins (the hash covers every file of `src/` in file-name order), so the offline check fails until the
ops are re-recorded.

It compiles against `working/Paper/paper-server/build/libs/paper-server-*.jar`
and uses libraries materialized by the M0 Paper run under
`tools/rivet-oracle/work/run/libraries`. Both locations can be overridden with
`RIVET_PAPER_JAR` and `RIVET_PAPER_LIBRARIES`; when overriding them, point
`RIVET_PAPER_RUNTIME_JAR` at the Paper jar materialized beside those runtime
libraries. The launcher requires the compile and runtime jars to have the same
SHA-256 and verifies their manifest identifies Paper 26.2.
It also requires the jar's Git commit to match the canonical `paper` revision
in `tools/rivet-oracle/fixtures/manifest.json`; changing the reference server
therefore requires deliberate fixture regeneration and re-pinning.
Paper 26.2 requires Java 25. The launcher finds a Java 25 SDKMAN installation
or uses `JAVA_HOME`; set `RIVET_JAVA_HOME` when neither points to the required
JDK.

Start the persistent oracle:

```sh
tools/rivet-reference-oracle/run.sh
```

Every non-empty stdin line must be one JSON object. Every stdout line is one
JSON response carrying the request's optional `id`:

```json
{"id":"example","op":"snbt.parse","input":"{answer:42,ok:true}"}
{"id":"example","protocol":1,"ok":true,"result":{"tag_id":10,"tag_type":"COMPOUND","snbt":"{answer:42,ok:1b}","pretty_snbt":"{\n    answer: 42,\n    ok: 1b\n}"}}
{"id":"comp","op":"component.json","input":"{\"text\":\"hello\",\"bold\":true}"}
{"id":"comp","protocol":1,"ok":true,"result":{"accept":true,"canonical":"{\"text\":\"hello\",\"bold\":true}"}}
```

Failed operations include `error.kind` (`input_rejected` or `unavailable`),
`error.operation`, `error.type`, and, when provided by Java, `error.message`.
Only known operation-specific input errors are rejection verdicts. Unexpected
exceptions, malformed requests, and ambiguous runtime failures remain unavailable
evidence; clients must not treat every `ok:false` response as input rejection.
Component codec rejection remains a successful result with `accept:false`.

Run the built-in smoke test (protocol, registry contract, float encoding)
without starting a persistent session:

```sh
tools/rivet-reference-oracle/run.sh --self-test
```

Diagnostics and compilation output go to stderr. Stdout is reserved for the
JSON Lines protocol. Build artifacts stay in the tool's ignored `target/`
directory; no Paper or Mojang-derived source is copied into Rivet.
