//! Port of `net.minecraft.world.level.levelgen.feature.treedecorators.
//! TreeDecorator` (abstract class, 26.2) — the dispatch root of the
//! tree-decorator framework.
//!
//! Java is the abstract base of the ten concrete decorators, with the dispatch
//! codec `CODEC`:
//!
//! ```text
//! CODEC = BuiltInRegistries.TREE_DECORATOR_TYPE.byNameCodec()
//!     .dispatch(TreeDecorator::type, TreeDecoratorType::codec);
//! ```
//!
//! The port splits identity from behavior the same way `FoliagePlacer`/
//! `TrunkPlacer`/`RootPlacer` do: [`TreeDecorator`] is the behavior contract
//! (with the `type()` identity and `place(Context)`), and
//! [`ErasedTreeDecorator`] is the object-safe carrier stored as
//! `Arc<dyn ErasedTreeDecorator>`. Every concrete decorator implements
//! `TreeDecorator`, so the erased carrier is blanket-derived; `as_any` is the
//! explicit downcast seam the tree feature uses to reach `place`.
//!
//! The `Context` is the `TreeDecorator.Context` inner class: it holds the
//! `WorldGenLevel`, the `decorationSetter` (`BiConsumer<BlockPos, BlockState>` →
//! `&mut dyn FnMut`), the `RandomSource`, and the `ObjectArrayList<BlockPos>`
//! logs/leaves/roots built from the placement sets and sorted by Y
//! (`Comparator.comparingInt(Vec3i::getY)`). The tree feature fills these lists
//! through the `Set<BlockPos>` constructor, so the port keeps the sorted-Vec
//! shape (`sort_by_key(get_y)`).

use crate::level::WorldGenLevel;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::Property;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_serialization::codec;
use rivet_serialization::codec::Codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::RandomSource;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `TreeDecorator.Context` — the per-decorator placement context.
///
/// The logs/leaves/roots lists are the Y-sorted `ObjectArrayList<BlockPos>`
/// snapshots Java builds in the constructor (`new ObjectArrayList<>(set)` then
/// `sort(comparingInt(Vec3i::getY))`).
#[derive(Debug)]
pub struct Context<'a, R: RandomSource> {
    level: &'a dyn WorldGenLevel,
    decoration_setter: &'a mut dyn FnMut(&BlockPos, BlockState),
    random: &'a mut R,
    logs: Vec<BlockPos>,
    leaves: Vec<BlockPos>,
    roots: Vec<BlockPos>,
}

impl<'a, R: RandomSource> Context<'a, R> {
    /// `new Context(WorldGenLevel, BiConsumer, RandomSource, Set, Set, Set)` —
    /// copies each set and sorts by Y.
    pub fn new(
        level: &'a dyn WorldGenLevel,
        decoration_setter: &'a mut dyn FnMut(&BlockPos, BlockState),
        random: &'a mut R,
        trunk_set: &[BlockPos],
        foliage_set: &[BlockPos],
        root_set: &[BlockPos],
    ) -> Context<'a, R> {
        let mut logs: Vec<BlockPos> = trunk_set.to_vec();
        logs.sort_by_key(BlockPos::get_y);
        let mut leaves: Vec<BlockPos> = foliage_set.to_vec();
        leaves.sort_by_key(BlockPos::get_y);
        let mut roots: Vec<BlockPos> = root_set.to_vec();
        roots.sort_by_key(BlockPos::get_y);
        Context {
            level,
            decoration_setter,
            random,
            logs,
            leaves,
            roots,
        }
    }

    /// `Context.placeVine(BlockPos, BooleanProperty)` — `setBlock(pos,
    /// Blocks.VINE.defaultBlockState().setValue(direction, true))`.
    pub fn place_vine(&mut self, pos: &BlockPos, direction: Property) {
        let vine = BlockState::of(
            BlockId::from_name("minecraft:vine").expect("vine is in the generated block table"),
        );
        let state = vine
            .set_value(direction, true)
            .expect("vine carries every horizontal direction property");
        self.set_block(pos, state);
    }

    /// `Context.setBlock(BlockPos, BlockState)` — forward to the decoration
    /// setter.
    pub fn set_block(&mut self, pos: &BlockPos, state: BlockState) {
        (self.decoration_setter)(pos, state);
    }

    /// `Context.isAir(BlockPos)` — the position holds air.
    pub fn is_air(&self, pos: &BlockPos) -> bool {
        self.level.is_state_at_position(pos, &|s: &BlockState| s.is_air())
    }

    /// `Context.checkBlock(BlockPos, Predicate<BlockState>)`.
    pub fn check_block(&self, pos: &BlockPos, predicate: &dyn Fn(&BlockState) -> bool) -> bool {
        self.level.is_state_at_position(pos, predicate)
    }

    /// `Context.level()`.
    pub fn level(&self) -> &dyn WorldGenLevel {
        self.level
    }

    /// `Context.random()`.
    pub fn random(&mut self) -> &mut R {
        self.random
    }

    /// `Context.logs()` — the Y-sorted log positions.
    pub fn logs(&self) -> &[BlockPos] {
        &self.logs
    }

    /// `Context.leaves()` — the Y-sorted leaf positions.
    pub fn leaves(&self) -> &[BlockPos] {
        &self.leaves
    }

    /// `Context.roots()` — the Y-sorted root positions.
    pub fn roots(&self) -> &[BlockPos] {
        &self.roots
    }
}

/// `net.minecraft.world.level.levelgen.feature.treedecorators.TreeDecorator` —
/// the behavior contract of a tree decorator (Java's abstract `place(Context)`
/// + `type()`).
pub trait TreeDecorator: Any + Debug + Send + Sync + 'static {
    /// `TreeDecorator.type()` — the registry-held `TreeDecoratorType<?>`
    /// identity.
    fn type_id(&self) -> crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId;

    /// `TreeDecorator.place(Context)` — apply the decoration.
    fn place<R: RandomSource>(&self, context: &mut Context<'_, R>);

    /// `as_any` — the downcast seam (Java's erased `TreeDecorator` cast).
    fn as_any(&self) -> &dyn Any;
}

/// The object-safe carrier the codec graph stores each decorator as — the
/// dispatch identity plus the `dyn`-compatible surface. Every `TreeDecorator`
/// implements it via the blanket impl.
pub trait ErasedTreeDecorator: Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId;

    /// `as_any` — the downcast seam over the erased carrier.
    fn as_any(&self) -> &dyn Any;
}

impl<P: TreeDecorator + ?Sized> ErasedTreeDecorator for P {
    fn type_id(
        &self,
    ) -> crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId {
        TreeDecorator::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        TreeDecorator::as_any(self)
    }
}

/// `TreeDecorator.CODEC` — the by-name dispatch codec, as the ops-generic
/// `tree_decorator_codec::<Ops>()` factory. The decorator fields are plain
/// (ints, block predicates, state predicates — no `BlockStateProvider`), so
/// unlike the `BlockStateProvider` dispatch this one is plain `DynamicOps`.
pub fn tree_decorator_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<Arc<dyn ErasedTreeDecorator>, Ops>> {
    // `BuiltInRegistries.TREE_DECORATOR_TYPE.byNameCodec().dispatch(...)`.
    map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId,
        Arc<dyn ErasedTreeDecorator>,
        Ops,
    >(
        "type",
        tree_decorator_type_by_name_codec::<Ops>(),
        Arc::new(|p: &Arc<dyn ErasedTreeDecorator>| {
            DataResult::success(ErasedTreeDecorator::type_id(&**p))
        }),
        codec_for_type(),
    ))
}

/// `TreeDecoratorType::codec` — resolve a `TreeDecoratorTypeId` to its
/// `MapCodec<Arc<dyn ErasedTreeDecorator>>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static>(
) -> key_dispatch_codec::CodecFn<
    crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId,
    Arc<dyn ErasedTreeDecorator>,
    Ops,
> {
    use crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypes;
    Arc::new(
        move |k: &crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId| {
            if *k == TreeDecoratorTypes::TRUNK_VINE {
                DataResult::success(erase_map_codec::<
                    crate::levelgen::feature::treedecorators::trunk_vine_decorator::TrunkVineDecorator,
                    Ops,
                >(
                    crate::levelgen::feature::treedecorators::trunk_vine_decorator::trunk_vine_decorator_map_codec::<
                        Ops,
                    >(),
                    Arc::new(|d: &crate::levelgen::feature::treedecorators::trunk_vine_decorator::TrunkVineDecorator| {
                        Arc::new(d.clone()) as Arc<dyn ErasedTreeDecorator>
                    }),
                    Arc::new(|p: &Arc<dyn ErasedTreeDecorator>| {
                        p.as_any()
                            .downcast_ref::<crate::levelgen::feature::treedecorators::trunk_vine_decorator::TrunkVineDecorator>()
                            .unwrap_or_else(|| panic!("tree-decorator dispatch produced a non-trunk-vine value"))
                            .clone()
                    }),
                ))
            } else {
                DataResult::error(format!(
                    "Tree decorator type '{}' is not ported",
                    k.location
                ))
            }
        },
    )
}

/// Lift a concrete decorator's `MapCodec<C>` to
/// `MapCodec<Arc<dyn ErasedTreeDecorator>>` — Java's
/// `MapCodec<? extends TreeDecorator>` variance, via xmap (the same lift every
/// dispatch file defines).
#[allow(clippy::type_complexity)]
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> Arc<dyn ErasedTreeDecorator> + Send + Sync>,
    unwrap: Arc<dyn Fn(&Arc<dyn ErasedTreeDecorator>) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedTreeDecorator>, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.TREE_DECORATOR_TYPE.byNameCodec()` over the type id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key()
/// .identifier())`, with the same unknown-key error shape every by-name codec
/// reproduces (`Registries.TREE_DECORATOR_TYPE` = `createRegistryKey(
/// "worldgen/tree_decorator_type")`).
#[allow(clippy::doc_lazy_continuation)]
pub fn tree_decorator_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<
    dyn Codec<
        crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId,
        Ops,
    >,
> {
    use crate::levelgen::feature::treedecorators::tree_decorator_type::tree_decorator_type_by_name;
    codec::comap_flat_map::<rivet_registry::Identifier, crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match tree_decorator_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/tree_decorator_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &crate::levelgen::feature::treedecorators::tree_decorator_type::TreeDecoratorTypeId| {
            rivet_registry::Identifier::parse(id.location)
        }),
    )
}

/// The horizontal `BooleanProperty` constants the vine decorators place vines
/// with — `Blocks.VINE`'s `BooleanProperty` fields are the four directions.
pub const VINE_DIRECTIONS: [Property; 4] = [
    BlockStateProperties::NORTH,
    BlockStateProperties::EAST,
    BlockStateProperties::SOUTH,
    BlockStateProperties::WEST,
];
