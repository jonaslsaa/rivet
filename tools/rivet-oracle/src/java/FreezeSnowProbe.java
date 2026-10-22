import java.lang.reflect.Proxy;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.tags.FluidTags;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.LightLayer;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BlockStateProperties;

/** Actual pinned Paper Biome methods and SnowLayerBlock.canSurvive, with live tags. */
public final class FreezeSnowProbe {
    private record Case(float temperature, boolean precipitation, int brightness, int y,
                        String target, String support, String neighbors, boolean checkNeighbors) {}

    private static BlockState state(String text) {
        String[] parts = text.split(":");
        BlockState state = BuiltInRegistries.BLOCK.getValue(Identifier.withDefaultNamespace(parts[0])).defaultBlockState();
        if (parts.length == 2) {
            state = state.setValue(parts[0].equals("snow") ? BlockStateProperties.LAYERS : BlockStateProperties.LEVEL,
                Integer.parseInt(parts[1]));
        }
        return state;
    }

    public static void main(String[] args) throws Exception {
        java.io.PrintStream output = System.out;
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var packs = net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(java.util.List.of("minecraft", "vanilla"), java.util.List.of()),
                net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS), true, false);
        try (var resources = new net.minecraft.server.packs.resources.MultiPackResourceManager(
                net.minecraft.server.packs.PackType.SERVER_DATA, packs.openAllSelected())) {
            var layers = net.minecraft.server.RegistryLayer.createRegistryAccess();
            net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,
                layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(pending -> pending.apply());
            Case[] cases = {
                new Case(0, true, 0, 64, "water", "stone", "air", false),
                new Case(0, false, 0, 64, "water", "stone", "air", false),
                new Case(0.15f, true, 0, 64, "water", "stone", "air", false),
                new Case(0, true, 9, 64, "water", "stone", "air", false),
                new Case(0, true, 10, 64, "water", "stone", "air", false),
                new Case(0, true, 0, 64, "water:1", "stone", "air", false),
                new Case(0, true, 0, 64, "seagrass", "stone", "air", false),
                new Case(0, true, 0, 64, "water", "stone", "water", true),
                new Case(0, true, 0, 64, "water", "stone", "water:1", true),
                new Case(0, true, 0, 64, "water", "stone", "air", true),
                new Case(0, true, 0, 64, "air", "stone", "air", false),
                new Case(0, false, 0, 64, "air", "stone", "air", false),
                new Case(0, true, 9, 64, "air", "stone", "air", false),
                new Case(0, true, 10, 64, "air", "stone", "air", false),
                new Case(0, true, 0, 64, "air", "ice", "air", false),
                new Case(0, true, 0, 64, "air", "honey_block", "air", false),
                new Case(0, true, 0, 64, "air", "oak_leaves", "air", false),
                new Case(0, true, 0, 64, "air", "snow:7", "air", false),
                new Case(0, true, 0, 64, "air", "snow:8", "air", false),
                new Case(0, true, 0, 64, "snow", "stone", "air", false),
                new Case(0, true, 0, 64, "short_grass", "stone", "air", false),
                new Case(0, true, 0, -65, "water", "stone", "air", false),
                new Case(0, true, 0, 320, "air", "stone", "air", false)
            };
            for (Case c : cases) {
                BlockPos pos = new BlockPos(0, c.y, 0);
                BlockState target = state(c.target), support = state(c.support), neighbors = state(c.neighbors);
                java.util.function.Function<BlockPos, BlockState> lookup = at -> at.equals(pos) ? target : at.equals(pos.below()) ? support : neighbors;
                LevelReader level = (LevelReader) Proxy.newProxyInstance(LevelReader.class.getClassLoader(), new Class<?>[]{LevelReader.class},
                    (proxy, method, arguments) -> switch (method.getName()) {
                        case "getBlockState" -> lookup.apply((BlockPos) arguments[0]);
                        case "getFluidState" -> lookup.apply((BlockPos) arguments[0]).getFluidState();
                        case "isWaterAt" -> lookup.apply((BlockPos) arguments[0]).getFluidState().is(FluidTags.WATER);
                        case "getBrightness" -> {
                            if (arguments[0] != LightLayer.BLOCK) throw new AssertionError("Biome must read BLOCK light");
                            yield c.brightness;
                        }
                        case "getSeaLevel" -> 63;
                        case "getHeight" -> 384;
                        case "getMinY" -> -64;
                        case "isInsideBuildHeight" -> (int) arguments[0] >= -64 && (int) arguments[0] < 320;
                        default -> throw new UnsupportedOperationException(method.toString());
                    });
                Biome biome = new Biome.BiomeBuilder().hasPrecipitation(c.precipitation).temperature(c.temperature).downfall(0.5f)
                    .specialEffects(new BiomeSpecialEffects.Builder().waterColor(0x3f76e4).build())
                    .mobSpawnSettings(MobSpawnSettings.EMPTY).generationSettings(BiomeGenerationSettings.EMPTY).build();
                output.println("FREEZE_SNOW\t" + c.temperature + "\t" + c.precipitation + "\t" + c.brightness + "\t" + c.y
                    + "\t" + c.target + "\t" + c.support + "\t" + c.neighbors + "\t" + c.checkNeighbors
                    + "\t" + biome.shouldFreeze(level, pos, c.checkNeighbors) + "\t" + biome.shouldSnow(level, pos));
            }
        }
    }
}
