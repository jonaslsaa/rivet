import com.google.gson.*;
import java.lang.reflect.Proxy;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.material.Fluid;

/** Actual state.updateShape effects over controlled storage, never mocked block behavior. */
public final class BlockShapeBehaviorProbe {
    record Case(String name, BlockState state, Direction direction, BlockState neighbor, BlockState above, BlockState below) {}
    public static void main(String[] args) {
        var output = System.out; var errors = System.err;
        try {
            SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
            var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
            net.minecraft.server.MinecraftServer.configurePackRepository(packs,
                new net.minecraft.world.level.WorldDataConfiguration(
                    new net.minecraft.world.level.DataPackConfig(List.of("vanilla"), List.of()),
                    net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), true, false);
            try (var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(
                    net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected())) {
                var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
                net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,
                    layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(pending -> pending.apply());
                var cases = new ArrayList<Case>();
                var air = Blocks.AIR.defaultBlockState(); var dirt = Blocks.DIRT.defaultBlockState();
                for (boolean wet : new boolean[]{false,true}) for (Direction dir : Direction.values())
                    cases.add(new Case("roots_"+wet+"_"+dir, Blocks.MANGROVE_ROOTS.defaultBlockState().setValue(BlockStateProperties.WATERLOGGED,wet), dir, air, air, dirt));
                for (boolean hanging : new boolean[]{false,true}) for (boolean wet : new boolean[]{false,true})
                    for (Block above : List.of(Blocks.MANGROVE_LEAVES,Blocks.OAK_LEAVES,Blocks.AIR))
                        for (Block below : List.of(Blocks.DIRT,Blocks.CLAY,Blocks.STONE)) {
                            var state = Blocks.MANGROVE_PROPAGULE.defaultBlockState()
                                .setValue(BlockStateProperties.HANGING,hanging).setValue(BlockStateProperties.WATERLOGGED,wet);
                            cases.add(new Case("propagule_"+hanging+"_"+wet+"_"+BuiltInRegistries.BLOCK.getKey(above)+"_"+BuiltInRegistries.BLOCK.getKey(below),
                                state,Direction.EAST,air,above.defaultBlockState(),below.defaultBlockState()));
                        }
                for (int waterLevel : new int[]{0,1,8}) for (Direction dir : new Direction[]{Direction.DOWN,Direction.UP,Direction.EAST})
                    for (BlockState neighbor : List.of(air, Blocks.WATER.defaultBlockState(),
                            Blocks.WATER.defaultBlockState().setValue(BlockStateProperties.LEVEL,1),
                            Blocks.MAGMA_BLOCK.defaultBlockState(),Blocks.SOUL_SAND.defaultBlockState()))
                        cases.add(new Case("water_"+waterLevel+"_"+dir+"_"+Block.getId(neighbor),
                            Blocks.WATER.defaultBlockState().setValue(BlockStateProperties.LEVEL,waterLevel),dir,neighbor,air,dirt));
                for (Direction dir : Direction.values()) {
                    cases.add(new Case("heart_"+dir, Blocks.CREAKING_HEART.defaultBlockState(),dir,air,air,dirt));
                    for (Block above : List.of(Blocks.AIR,Blocks.STONE,Blocks.OAK_LEAVES,Blocks.PALE_HANGING_MOSS))
                        for (Block below : List.of(Blocks.AIR,Blocks.PALE_HANGING_MOSS))
                            cases.add(new Case("hanging_moss_"+dir+"_"+BuiltInRegistries.BLOCK.getKey(above)+"_"+BuiltInRegistries.BLOCK.getKey(below),
                                Blocks.PALE_HANGING_MOSS.defaultBlockState(),dir,air,above.defaultBlockState(),below.defaultBlockState()));
                    for (boolean base : new boolean[]{false,true})
                        for (BlockState below : List.of(air,dirt,Blocks.PALE_MOSS_CARPET.defaultBlockState(),
                                Blocks.PALE_MOSS_CARPET.defaultBlockState().setValue(MossyCarpetBlock.BASE,false)))
                            for (BlockState neighbor : List.of(air,Blocks.STONE.defaultBlockState(),Blocks.OAK_LEAVES.defaultBlockState()))
                                cases.add(new Case("moss_carpet_"+dir+"_"+base+"_"+Block.getId(below)+"_"+Block.getId(neighbor),
                                    Blocks.PALE_MOSS_CARPET.defaultBlockState().setValue(MossyCarpetBlock.BASE,base),dir,neighbor,air,below));
                    for (var half : net.minecraft.world.level.block.state.properties.DoubleBlockHalf.values())
                        for (BlockState neighbor : List.of(air,dirt,Blocks.TALL_GRASS.defaultBlockState(),
                                Blocks.TALL_GRASS.defaultBlockState().setValue(DoublePlantBlock.HALF,net.minecraft.world.level.block.state.properties.DoubleBlockHalf.UPPER)))
                            cases.add(new Case("double_plant_"+dir+"_"+half+"_"+Block.getId(neighbor),
                                Blocks.TALL_GRASS.defaultBlockState().setValue(DoublePlantBlock.HALF,half),dir,neighbor,air,dirt));
                }
                for (boolean wet : new boolean[]{false,true}) for (Direction dir : Direction.values()) {
                    var lower=Blocks.SMALL_DRIPLEAF.defaultBlockState().setValue(BlockStateProperties.DOUBLE_BLOCK_HALF,
                        net.minecraft.world.level.block.state.properties.DoubleBlockHalf.LOWER);
                    var upper=lower.setValue(BlockStateProperties.DOUBLE_BLOCK_HALF,
                        net.minecraft.world.level.block.state.properties.DoubleBlockHalf.UPPER);
                    for (var half : net.minecraft.world.level.block.state.properties.DoubleBlockHalf.values())
                        for (BlockState below : List.of(air,dirt,Blocks.CLAY.defaultBlockState(),Blocks.STONE.defaultBlockState()))
                            for (BlockState neighbor : List.of(air,Blocks.STONE.defaultBlockState(),lower,upper)) {
                                var state=lower.setValue(BlockStateProperties.DOUBLE_BLOCK_HALF,half).setValue(BlockStateProperties.WATERLOGGED,wet);
                                cases.add(new Case("small_dripleaf_"+wet+"_"+dir+"_"+half+"_"+Block.getId(below)+"_"+Block.getId(neighbor),
                                    state,dir,neighbor,upper,below));
                            }
                    var head=Blocks.BIG_DRIPLEAF.defaultBlockState();var stem=Blocks.BIG_DRIPLEAF_STEM.defaultBlockState();
                    for (Direction facing : Direction.Plane.HORIZONTAL)
                        for (BlockState below : List.of(air,Blocks.CLAY.defaultBlockState(),stem,head))
                            for (BlockState neighbor : List.of(air,stem,head)) {
                                var state=head.setValue(BlockStateProperties.HORIZONTAL_FACING,facing).setValue(BlockStateProperties.WATERLOGGED,wet)
                                    .setValue(BlockStateProperties.TILT,net.minecraft.world.level.block.state.properties.Tilt.FULL);
                                cases.add(new Case("big_dripleaf_"+wet+"_"+dir+"_"+facing+"_"+Block.getId(below)+"_"+Block.getId(neighbor),
                                    state,dir,neighbor,air,below));
                            }
                    for (BlockState below : List.of(air,Blocks.CLAY.defaultBlockState(),stem,head))
                        for (BlockState above : List.of(air,stem,head))
                            for (BlockState neighbor : List.of(air,stem,head)) {
                                var state=stem.setValue(BlockStateProperties.HORIZONTAL_FACING,Direction.EAST).setValue(BlockStateProperties.WATERLOGGED,wet);
                                cases.add(new Case("big_dripleaf_stem_"+wet+"_"+dir+"_"+Block.getId(below)+"_"+Block.getId(above)+"_"+Block.getId(neighbor),
                                    state,dir,neighbor,above,below));
                            }
                }
                for (var test : cases) {
                    var pos = new BlockPos(3,70,-5); var states = new HashMap<BlockPos,BlockState>();
                    states.put(pos,test.state()); states.put(pos.above(),test.above()); states.put(pos.below(),test.below());
                    states.put(pos.relative(test.direction()),test.neighbor());
                    var ticks = new JsonArray();
                    WorldGenLevel level = (WorldGenLevel) Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},
                        (proxy,method,values) -> switch(method.getName()) {
                            case "getBlockState" -> states.getOrDefault((BlockPos)values[0],air);
                            case "getFluidState" -> states.getOrDefault((BlockPos)values[0],air).getFluidState();
                            case "scheduleTick" -> {
                                var p=(BlockPos)values[0]; var tick=new JsonArray(); tick.add(p.getX());tick.add(p.getY());tick.add(p.getZ());
                                tick.add(values[1] instanceof Block b ? BuiltInRegistries.BLOCK.getKey(b).toString() : BuiltInRegistries.FLUID.getKey((Fluid)values[1]).toString());
                                tick.add((int)values[2]);ticks.add(tick);yield null;
                            }
                            default -> throw new UnsupportedOperationException(method.toString());
                        });
                    var random=new XoroshiroRandomSource(42);
                    var result=test.state().updateShape(level,level,pos,test.direction(),pos.relative(test.direction()),test.neighbor(),random);
                    var row=new JsonObject();row.addProperty("name",test.name());row.addProperty("state",Block.getId(test.state()));
                    row.addProperty("direction",test.direction().getSerializedName());row.addProperty("neighbor",Block.getId(test.neighbor()));
                    row.addProperty("above",Block.getId(test.above()));row.addProperty("below",Block.getId(test.below()));
                    row.addProperty("result",Block.getId(result));row.add("ticks",ticks);row.addProperty("next_long",random.nextLong());
                    if (test.name().contains("dripleaf")) row.addProperty("can_survive",test.state().canSurvive(level,pos));
                    output.println("BLOCK_SHAPE\t"+row);
                }
            }
        } catch(Throwable error) {error.printStackTrace(errors);System.exit(1);}
    }
}
