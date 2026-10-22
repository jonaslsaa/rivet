import com.google.gson.*;
import java.io.PrintStream;
import java.security.MessageDigest;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.structure.*;

/** Runs genuine Paper fillFromNoise with nonzero actual structure influence,
 * a controlled constant negative router, genuine aquifer/material selection,
 * and real ProtoChunk sections/heightmaps. No copied density or fill algorithm. */
public final class BeardifierNoiseProbe {
    public static void main(String[] args)throws Exception {
        try(var output=new PrintStream(args[0])) {
            SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
            var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
            net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
            try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())) {
                var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
                var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
                var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
                var access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess();
                var source=new FixedBiomeSource(access.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS));
                var zero=DensityFunctions.zero();
                var router=new NoiseRouter(zero,zero,zero,zero,zero,zero,zero,zero,zero,zero,DensityFunctions.constant(100),DensityFunctions.interpolated(DensityFunctions.cacheOnce(DensityFunctions.constant(-0.1))),zero,zero,zero);
                var settings=new NoiseGeneratorSettings(NoiseSettings.create(48,32,1,1),Blocks.STONE.defaultBlockState(),Blocks.AIR.defaultBlockState(),router,SurfaceRules.state(Blocks.AIR.defaultBlockState()),List.of(),0,true,false,false,false);
                var generator=new NoiseBasedChunkGenerator(source,Holder.direct(settings));
                var factory=PalettedContainerFactory.create(access);
                for(long seed:new long[]{0,42})for(var pos:List.of(new ChunkPos(0,0),new ChunkPos(-2,1)))for(var adjustment:TerrainAdjustment.values()) {
                    int x=pos.getMinBlockX(),z=pos.getMinBlockZ();
                    var starts=List.of(BeardifierProbe.start(adjustment,List.of(BeardifierProbe.ordinary(new BoundingBox(x+2,60,z+2,x+13,68,z+13)))));
                    var chunk=new ProtoChunk(pos,UpgradeData.EMPTY,LevelHeightAccessor.create(48,32),factory,null);
                    var random=RandomState.create(settings,access.lookupOrThrow(Registries.NOISE),seed);
                    generator.fillFromNoise(Blender.empty(),random,new BeardifierProbe.Manager(starts),chunk).join();
                    var hash=MessageDigest.getInstance("SHA-256");int solid=0;
                    for(int y=48;y<80;y++)for(int dz=0;dz<16;dz++)for(int dx=0;dx<16;dx++) {
                        var state=chunk.getBlockState(new BlockPos(x+dx,y,z+dz));
                        if(!state.isAir())solid++;
                        hash.update(java.nio.ByteBuffer.allocate(4).putInt(Block.getId(state)).array());
                    }
                    var row=new JsonObject();row.addProperty("seed",seed);row.addProperty("x",pos.x());row.addProperty("z",pos.z());row.addProperty("adjustment",adjustment.getSerializedName());
                    row.addProperty("block_hash",HexFormat.of().formatHex(hash.digest()));row.addProperty("solid_count",solid);
                    for(var type:List.of(Heightmap.Types.WORLD_SURFACE_WG,Heightmap.Types.OCEAN_FLOOR_WG)) {
                        var heights=new JsonArray();for(int dz=0;dz<16;dz++)for(int dx=0;dx<16;dx++)heights.add(chunk.getHeight(type,dx,dz));row.add(type.getSerializationKey(),heights);
                    }
                    int postprocessing=0;for(var values:chunk.getPostProcessing())if(values!=null)postprocessing+=values.size();row.addProperty("postprocessing",postprocessing);
                    output.println(row);
                }
            }
        }
    }
}
