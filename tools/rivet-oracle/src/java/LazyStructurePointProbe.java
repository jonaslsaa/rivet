import com.google.gson.*;
import java.io.PrintStream;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
/** Genuine lazy findGenerationPoint/findValidGenerationPoint calls, without
 * executing the unavailable piece-builder callback or requiring fake templates. */
public final class LazyStructurePointProbe {
 @SuppressWarnings("unchecked")
 public static void main(String[] args)throws Exception {
  try(var output=new PrintStream(args[0])){
   SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
   var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
   net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
   try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())){
    var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
    var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
    var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
    layers=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen);
    var allLookups=new ArrayList<HolderLookup.RegistryLookup<?>>(lookups);worldgen.listRegistries().forEach(allLookups::add);
    var dimensions=RegistryDataLoader.load(resources,allLookups,RegistryDataLoader.DIMENSION_REGISTRIES,Runnable::run).join();
    var access=layers.replaceFrom(net.minecraft.server.RegistryLayer.DIMENSIONS,dimensions).compositeAccess().freeze();
    var normal=(NoiseBasedChunkGenerator)access.lookupOrThrow(Registries.WORLD_PRESET).getOrThrow(net.minecraft.world.level.levelgen.presets.WorldPresets.NORMAL).value().createWorldDimensions().overworld();
    var height=LevelHeightAccessor.create(-64,384);
    var settings=access.lookupOrThrow(Registries.NOISE_SETTINGS).getOrThrow(NoiseGeneratorSettings.OVERWORLD);
    var find=Structure.class.getDeclaredMethod("findGenerationPoint",Structure.GenerationContext.class);find.setAccessible(true);
    for(long seed:new long[]{0L,42L,Long.MIN_VALUE}){
     var random=RandomState.create(access,NoiseGeneratorSettings.OVERWORLD,seed);
     for(String name:new String[]{"shipwreck","shipwreck_beached","igloo","desert_pyramid"}){
      var selected=access.lookupOrThrow(Registries.STRUCTURE).getOrThrow(ResourceKey.create(Registries.STRUCTURE,Identifier.withDefaultNamespace(name)));
      var structure=selected.value();
      for(String biomeName:new String[]{"normal","plains","ocean","desert","snowy_plains","beach"}){
       BiomeSource source=biomeName.equals("normal")?normal.getBiomeSource():new FixedBiomeSource(access.lookupOrThrow(Registries.BIOME).getOrThrow(ResourceKey.create(Registries.BIOME,Identifier.withDefaultNamespace(biomeName))));
       var generator=new NoiseBasedChunkGenerator(source,settings);
       for(int[] pos:new int[][]{{0,0},{-8,7},{1,6},{1,7}}){
        var chunk=new ChunkPos(pos[0],pos[1]);
        var context=new Structure.GenerationContext(access,generator,source,random,null,seed,chunk,height,structure.biomes()::contains);
        var stub=(Optional<Structure.GenerationStub>)find.invoke(structure,context);
        var validatedContext=new Structure.GenerationContext(access,generator,source,random,null,seed,chunk,height,structure.biomes()::contains);
        var valid=structure.findValidGenerationPoint(validatedContext);
        var row=new JsonObject();row.addProperty("seed",seed);row.addProperty("structure",name);row.addProperty("generator_biome",biomeName);row.addProperty("x",chunk.x());row.addProperty("z",chunk.z());
        row.addProperty("point_present",stub.isPresent());row.addProperty("valid",valid.isPresent());row.addProperty("rng_next",context.random().nextLong());row.addProperty("valid_rng_next",validatedContext.random().nextLong());
        if(stub.isPresent()){
         var p=stub.get().position();row.add("point",new Gson().toJsonTree(new int[]{p.getX(),p.getY(),p.getZ()}));
         var biome=source.getNoiseBiome(p.getX()>>2,p.getY()>>2,p.getZ()>>2,random.sampler());row.addProperty("biome",biome.unwrapKey().orElseThrow().identifier().toString());
        }
        output.println(row);output.flush();
       }
      }
     }
    }
   }
  }
 }
}
