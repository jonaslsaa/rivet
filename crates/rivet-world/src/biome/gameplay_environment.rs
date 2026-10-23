//! The live Boolean environment attributes used by beehives and creaking hearts.
//!
//! This is an explicit projection of the pinned full server registry, not a second
//! biome registry and not an implementation of unrelated color/audio attributes.
//! Layer order is EnvironmentAttributeSystem.addDefaultLayers: dimension, biome,
//! timelines, weather. Boolean attributes are not spatially interpolated; the
//! caller supplies the actual noise biome at the *center* of the queried block.
use rivet_nbt::{compound_tag::CompoundTag, tag::Tag, tag_parser::parse_compound_fully};
use rivet_registry::{Identifier, biome_id::BiomeId};
use std::{collections::HashMap, sync::LazyLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameplayBoolean {
    BeesStayInHive,
    CreakingActive,
}

impl GameplayBoolean {
    pub fn name(self) -> &'static str {
        match self {
            Self::BeesStayInHive => "minecraft:gameplay/bees_stay_in_hive",
            Self::CreakingActive => "minecraft:gameplay/creaking_active",
        }
    }

    fn index(self) -> usize {
        match self {
            Self::BeesStayInHive => 0,
            Self::CreakingActive => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanModifier {
    Override,
    And,
    Nand,
    Or,
    Nor,
    Xor,
    Xnor,
}

impl BooleanModifier {
    fn name(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::And => "and",
            Self::Nand => "nand",
            Self::Or => "or",
            Self::Nor => "nor",
            Self::Xor => "xor",
            Self::Xnor => "xnor",
        }
    }

    pub fn apply(self, subject: bool, argument: bool) -> bool {
        match self {
            Self::Override => argument,
            Self::And => argument && subject,
            Self::Nand => !argument || !subject,
            Self::Or => argument || subject,
            Self::Nor => !argument && !subject,
            Self::Xor => argument ^ subject,
            Self::Xnor => argument == subject,
        }
    }

    fn decode(name: &str) -> Result<Self, GameplayEnvironmentError> {
        match name {
            "override" => Ok(Self::Override),
            "and" => Ok(Self::And),
            "nand" => Ok(Self::Nand),
            "or" => Ok(Self::Or),
            "nor" => Ok(Self::Nor),
            "xor" => Ok(Self::Xor),
            "xnor" => Ok(Self::Xnor),
            _ => Err(invalid(format!("unknown Boolean modifier {name}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    modifier: BooleanModifier,
    argument: bool,
}

/// The two requested entries of an actual EnvironmentAttributeMap. All other
/// attributes are deliberately outside this projection, not decoded as empty.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameplayAttributeMap([Option<Entry>; 2]);

impl GameplayAttributeMap {
    pub fn from_nbt(input: &CompoundTag) -> Result<Self, GameplayEnvironmentError> {
        let mut result = Self::default();
        for attribute in [
            GameplayBoolean::BeesStayInHive,
            GameplayBoolean::CreakingActive,
        ] {
            let Some(tag) = input.get(attribute.name()) else {
                continue;
            };
            let entry = if let Some(value) = codec_bool(tag) {
                Entry {
                    modifier: BooleanModifier::Override,
                    argument: value,
                }
            } else if let Tag::Compound(full) = tag {
                Entry {
                    modifier: BooleanModifier::decode(
                        full.get_string("modifier")
                            .ok_or_else(|| invalid("missing modifier"))?,
                    )?,
                    argument: full
                        .get("argument")
                        .and_then(codec_bool)
                        .ok_or_else(|| invalid("missing Boolean argument"))?,
                }
            } else {
                return Err(invalid(format!("invalid {} entry", attribute.name())));
            };
            result.0[attribute.index()] = Some(entry);
        }
        Ok(result)
    }

    /// Canonical CODEC representation of this explicit two-attribute
    /// projection: override entries encode as direct values, other modifiers
    /// encode the operation/argument pair. Unprojected attributes are not owned
    /// by this value and are not represented as a purported full-map save.
    pub fn to_nbt(&self) -> CompoundTag {
        let mut output = CompoundTag::new();
        for attribute in [
            GameplayBoolean::BeesStayInHive,
            GameplayBoolean::CreakingActive,
        ] {
            if let Some(entry) = self.0[attribute.index()] {
                if entry.modifier == BooleanModifier::Override {
                    output.put_boolean(attribute.name(), entry.argument);
                } else {
                    let mut data = CompoundTag::new();
                    data.put_string("modifier", entry.modifier.name());
                    data.put_boolean("argument", entry.argument);
                    output.put(attribute.name().into(), Tag::Compound(data));
                }
            }
        }
        output
    }

    pub fn apply(&self, attribute: GameplayBoolean, base: bool) -> bool {
        self.0[attribute.index()].map_or(base, |entry| entry.modifier.apply(base, entry.argument))
    }
}

#[derive(Debug, Clone)]
struct Track {
    attribute: GameplayBoolean,
    clock: Identifier,
    period: Option<i32>,
    modifier: BooleanModifier,
    keyframes: Vec<(i32, bool)>,
}

impl Track {
    fn sample(&self, ticks: i64) -> bool {
        // KeyframeTrackSampler loops with floorMod, including negative clocks.
        let ticks = self
            .period
            .map_or(ticks, |period| ticks.rem_euclid(i64::from(period)));
        // Boolean argumentKeyframeLerp is ofStep(1), so the left value holds
        // until the next keyframe. Before the first looping keyframe the left
        // endpoint is the last keyframe of the previous period.
        self.keyframes
            .iter()
            .rev()
            .find(|(at, _)| i64::from(*at) <= ticks)
            .map_or_else(
                || {
                    if self.period.is_some() {
                        self.keyframes.last().unwrap().1
                    } else {
                        self.keyframes[0].1
                    }
                },
                |(_, value)| *value,
            )
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum GameplayEnvironmentError {
    #[error("environment registry {registry} has no element {element}")]
    MissingRegistryElement { registry: String, element: String },
    #[error("invalid environment attribute data: {0}")]
    InvalidData(String),
    #[error("actual world clock {0} is unavailable")]
    MissingClock(Identifier),
    #[error("biome id {0} is not in the actual environment registry")]
    MissingBiome(u16),
    #[error("actual noise-biome chunk {0:?} is unavailable")]
    MissingBiomeChunk(rivet_registry::core::ChunkPos),
}

fn invalid(message: impl Into<String>) -> GameplayEnvironmentError {
    GameplayEnvironmentError::InvalidData(message.into())
}

// Codec.BOOL via NbtOps.getBooleanValue uses doubleValue != 0, unlike
// CompoundTag.getBoolean's byteValue coercion (e.g. 256 and 0.5 are true).
fn codec_bool(tag: &Tag) -> Option<bool> {
    tag.as_number_f64().map(|value| value != 0.0)
}

/// A tick's actual Level.getRainLevel(1) and getThunderLevel(1) values.
/// The latter already includes Level's multiplication by rain strength.
#[derive(Debug, Clone, Copy)]
pub struct EnvironmentWeather {
    pub rain: f32,
    pub thunder: f32,
}

#[derive(Debug, Clone)]
pub struct GameplayEnvironment {
    dimension: GameplayAttributeMap,
    biomes: HashMap<BiomeId, GameplayAttributeMap>,
    biome_layers: [bool; 2],
    tracks: Vec<Track>,
    can_have_weather: bool,
}

impl GameplayEnvironment {
    /// Decode actual full dimension/biome/timeline payloads. The world
    /// dimension key is distinct from its dimension-type key (Level.END gate).
    pub fn from_pinned_registries(
        dimension_type: &Identifier,
        world_dimension: &Identifier,
    ) -> Result<Self, GameplayEnvironmentError> {
        let dimension = registry_element("minecraft:dimension_type", &dimension_type.to_string())?;
        let attributes = projected_map(dimension)?;
        let can_have_weather = dimension
            .get_boolean("has_skylight")
            .ok_or_else(|| invalid("has_skylight"))?
            && !dimension
                .get_boolean("has_ceiling")
                .ok_or_else(|| invalid("has_ceiling"))?
            && *world_dimension != Identifier::parse("minecraft:the_end");
        let mut biomes = HashMap::new();
        for row in pinned_data()?
            .iter()
            .filter(|row| row.registry == "minecraft:worldgen/biome")
        {
            let id = BiomeId::from_name(&row.key)
                .ok_or_else(|| invalid(format!("unknown biome {}", row.key)))?;
            biomes.insert(id, projected_map(&row.data)?);
        }
        let biome_layers = [0, 1].map(|index| biomes.values().any(|map| map.0[index].is_some()));
        let mut tracks = Vec::new();
        if let Some(timelines) = dimension.get("timelines") {
            let names: Vec<&str> = match timelines {
                Tag::String(name) => {
                    return Err(invalid(format!(
                        "timeline holder tag/list required: {name:?}"
                    )));
                }
                Tag::List(names) => names
                    .list
                    .iter()
                    .map(|name| {
                        name.as_string()
                            .map(String::as_str)
                            .ok_or_else(|| invalid("timeline holder is not a string"))
                    })
                    .collect::<Result<_, _>>()?,
                _ => return Err(invalid("invalid timeline holder set")),
            };
            for name in names {
                let timeline = registry_element("minecraft:timeline", name)?;
                decode_tracks(timeline, &mut tracks)?;
            }
        }
        Ok(Self {
            dimension: attributes,
            biomes,
            biome_layers,
            tracks,
            can_have_weather,
        })
    }

    /// No clock or weather default is invented. The caller selects the actual
    /// noise biome at position and supplies the live ClockManager lookup.
    pub fn sample(
        &self,
        attribute: GameplayBoolean,
        biome: BiomeId,
        clock: impl FnMut(&Identifier) -> Result<i64, GameplayEnvironmentError>,
        weather: EnvironmentWeather,
    ) -> Result<bool, GameplayEnvironmentError> {
        self.sample_at(attribute, || Ok(biome), clock, weather)
    }

    /// Source addBiomeLayer only creates a positional layer when at least one
    /// registered biome supplies the requested attribute. In the pinned full
    /// registry neither bee nor heart attribute has such a biome layer.
    pub fn has_biome_layer(&self, attribute: GameplayBoolean) -> bool {
        self.biome_layers[attribute.index()]
    }

    /// Lazy actual getNoiseBiomeAtPosition authority. No biome lookup is made
    /// when the registry has no corresponding positional layer, exactly as
    /// EnvironmentAttributeSystem. The callback must select the center of the
    /// original BlockPos (EnvironmentAttributeReader.getValue(BlockPos)).
    pub fn sample_at(
        &self,
        attribute: GameplayBoolean,
        mut biome: impl FnMut() -> Result<BiomeId, GameplayEnvironmentError>,
        mut clock: impl FnMut(&Identifier) -> Result<i64, GameplayEnvironmentError>,
        weather: EnvironmentWeather,
    ) -> Result<bool, GameplayEnvironmentError> {
        let mut value = self.dimension.apply(attribute, false);
        if self.has_biome_layer(attribute) {
            let id = biome()?;
            let map = self
                .biomes
                .get(&id)
                .ok_or(GameplayEnvironmentError::MissingBiome(id.id()))?;
            value = map.apply(attribute, value);
        }
        for track in &self.tracks {
            if track.attribute == attribute {
                value = track
                    .modifier
                    .apply(value, track.sample(clock(&track.clock)?));
            }
        }
        if self.can_have_weather && attribute == GameplayBoolean::BeesStayInHive {
            // WeatherAttributes applies rain-minus-thunder, then thunder.
            // Boolean stateChangeLerp is ofStep(0), NOT spatialLerp's
            // ofStep(0.5). WeatherAttributes gates each layer on strength>0,
            // then every positive strength immediately selects its true value.
            let rain = weather.rain - weather.thunder;
            if rain > 0.0 {
                value = true;
            }
            if weather.thunder > 0.0 {
                value = true;
            }
        }
        Ok(value)
    }
}

fn projected_map(input: &CompoundTag) -> Result<GameplayAttributeMap, GameplayEnvironmentError> {
    match input.get("attributes") {
        None => Ok(GameplayAttributeMap::default()),
        Some(Tag::Compound(map)) => GameplayAttributeMap::from_nbt(map),
        _ => Err(invalid("attributes is not a map")),
    }
}

fn decode_tracks(
    input: &CompoundTag,
    output: &mut Vec<Track>,
) -> Result<(), GameplayEnvironmentError> {
    let Some(tracks) = input.get_compound("tracks") else {
        return Ok(());
    };
    let clock = Identifier::parse(
        input
            .get_string("clock")
            .ok_or_else(|| invalid("missing timeline clock"))?,
    );
    let period = input.get_int("period_ticks");
    if period.is_some_and(|value| value <= 0) {
        return Err(invalid("nonpositive period"));
    }
    for attribute in [
        GameplayBoolean::BeesStayInHive,
        GameplayBoolean::CreakingActive,
    ] {
        let Some(track) = tracks.get_compound(attribute.name()) else {
            continue;
        };
        if track.get("ease").is_some() {
            return Err(invalid("Boolean track easing requires an easing sampler"));
        }
        let modifier = BooleanModifier::decode(track.get_string_or("modifier", "override"))?;
        let frames = track
            .get_list("keyframes")
            .ok_or_else(|| invalid("missing keyframes"))?;
        let mut keyframes = Vec::new();
        for frame in &frames.list {
            let Tag::Compound(frame) = frame else {
                return Err(invalid("invalid keyframe"));
            };
            let ticks = frame
                .get_int("ticks")
                .ok_or_else(|| invalid("missing keyframe ticks"))?;
            let value = frame
                .get("value")
                .and_then(codec_bool)
                .ok_or_else(|| invalid("missing keyframe Boolean"))?;
            if keyframes.last().is_some_and(|(last, _)| *last >= ticks)
                || period.is_some_and(|period| ticks < 0 || ticks >= period)
            {
                return Err(invalid(
                    "keyframes outside period or not strictly ascending",
                ));
            }
            keyframes.push((ticks, value));
        }
        if keyframes.is_empty() {
            return Err(invalid("empty keyframes"));
        }
        output.push(Track {
            attribute,
            clock: clock.clone(),
            period,
            modifier,
            keyframes,
        });
    }
    Ok(())
}

fn registry_element(
    registry: &str,
    element: &str,
) -> Result<&'static CompoundTag, GameplayEnvironmentError> {
    let name = Identifier::parse(element);
    pinned_data()?
        .iter()
        .find(|row| row.registry == registry && Identifier::parse(&row.key) == name)
        .map(|row| &row.data)
        .ok_or_else(|| GameplayEnvironmentError::MissingRegistryElement {
            registry: registry.into(),
            element: element.into(),
        })
}

struct RegistryRow {
    registry: String,
    key: String,
    data: CompoundTag,
}

fn pinned_data() -> Result<&'static [RegistryRow], GameplayEnvironmentError> {
    static DATA: LazyLock<Result<Vec<RegistryRow>, GameplayEnvironmentError>> =
        LazyLock::new(|| {
            include_str!("gameplay_environment.jsonl")
                .lines()
                .map(|line| {
                    let row: serde_json::Value =
                        serde_json::from_str(line).map_err(|error| invalid(error.to_string()))?;
                    let registry = row["registry"]
                        .as_str()
                        .ok_or_else(|| invalid("missing registry"))?
                        .to_owned();
                    let key = row["key"]
                        .as_str()
                        .ok_or_else(|| invalid("missing key"))?
                        .to_owned();
                    let data = parse_compound_fully(
                        row["snbt"]
                            .as_str()
                            .ok_or_else(|| invalid("missing snbt"))?,
                    )
                    .map_err(|error| invalid(error.to_string()))?;
                    Ok(RegistryRow {
                        registry,
                        key,
                        data,
                    })
                })
                .collect()
        });
    DATA.as_ref().map(Vec::as_slice).map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameplay_sampling_matches_actual_full_paper_registry_and_layers() {
        let mut environments = HashMap::new();
        let mut count = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/environment-attributes.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let dimension = row["dimension"].as_str().unwrap();
            let environment = environments.entry(dimension.to_owned()).or_insert_with(|| {
                let key = Identifier::parse(dimension);
                GameplayEnvironment::from_pinned_registries(&key, &key).unwrap()
            });
            let biome = BiomeId::from_name(row["biome"].as_str().unwrap()).unwrap();
            let weather = EnvironmentWeather {
                rain: row["rain"].as_f64().unwrap() as f32,
                thunder: row["thunder"].as_f64().unwrap() as f32,
            };
            for (attribute, expected) in [
                (GameplayBoolean::BeesStayInHive, "bees"),
                (GameplayBoolean::CreakingActive, "creaking"),
            ] {
                let result = environment
                    .sample(
                        attribute,
                        biome,
                        |clock| {
                            assert_eq!(*clock, Identifier::parse("minecraft:overworld"));
                            Ok(row["clock"].as_i64().unwrap())
                        },
                        weather,
                    )
                    .unwrap();
                assert_eq!(
                    result,
                    row[expected].as_bool().unwrap(),
                    "{line}: {expected}"
                );
            }
            count += 1;
        }
        assert_eq!(count, 1368);
        assert_eq!(pinned_data().unwrap().len(), 74);
    }

    #[test]
    fn real_clock_and_biome_authority_are_required_instead_of_time_defaults() {
        let key = Identifier::parse("minecraft:overworld");
        let environment = GameplayEnvironment::from_pinned_registries(&key, &key).unwrap();
        let weather = EnvironmentWeather {
            rain: 0.0,
            thunder: 0.0,
        };
        let biome = BiomeId::from_name("minecraft:plains").unwrap();
        assert!(matches!(
            environment.sample(
                GameplayBoolean::BeesStayInHive,
                biome,
                |clock| Err(GameplayEnvironmentError::MissingClock(clock.clone())),
                weather
            ),
            Err(GameplayEnvironmentError::MissingClock(_))
        ));
        for attribute in [
            GameplayBoolean::BeesStayInHive,
            GameplayBoolean::CreakingActive,
        ] {
            assert!(!environment.has_biome_layer(attribute));
            assert!(
                !environment
                    .sample_at(
                        attribute,
                        || panic!("source has no positional biome layer"),
                        |_| Ok(0),
                        weather
                    )
                    .unwrap()
            );
        }
    }

    #[test]
    fn boolean_map_codec_and_all_operations_match_actual_paper() {
        let mut count = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/environment-boolean-maps.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let input = parse_compound_fully(row["input_snbt"].as_str().unwrap()).unwrap();
            let map = GameplayAttributeMap::from_nbt(&input).unwrap();
            assert_eq!(
                map.apply(
                    GameplayBoolean::BeesStayInHive,
                    row["base"].as_bool().unwrap()
                ),
                row["value"].as_bool().unwrap(),
                "{line}"
            );
            assert_eq!(
                map.to_nbt(),
                parse_compound_fully(row["output_snbt"].as_str().unwrap()).unwrap(),
                "{line}"
            );
            count += 1;
        }
        assert_eq!(count, 70);
    }
}
