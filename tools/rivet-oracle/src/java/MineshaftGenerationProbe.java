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
import net.minecraft.world.level.levelgen.structure.pieces.*;
/** Actual eager mineshaft graph, adjusted biome eligibility, saved starts and
 * GenerationContext RNG continuation; no Bukkit event or postProcess. */
public final class MineshaftGenerationProbe {
 public static void main(String[] args)throws Exception {
  try(var output=new PrintStream(args[0])){
   SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
   if(args.length>1 && args[1].equals("moves")){
    for(int direction:new int[]{1,-1}){
     int low=direction==1?Integer.MAX_VALUE-1:Integer.MIN_VALUE;
     int high=low+1;
     var tag=new net.minecraft.nbt.CompoundTag();tag.putIntArray("BB",new int[]{low,low,low,high,high,high});tag.putInt("O",-1);tag.putInt("MST",0);tag.putInt("GD",0);
     var entrances=new net.minecraft.nbt.ListTag();entrances.add(new net.minecraft.nbt.IntArrayTag(new int[]{low,low,low,high,high,high}));tag.put("Entrances",entrances);
     var piece=new net.minecraft.world.level.levelgen.structure.structures.MineshaftPieces.MineShaftRoom(tag);
     var row=new JsonObject();row.addProperty("input",piece.createTag(null).toString());row.addProperty("delta",direction);
     piece.move(direction,direction,direction);row.addProperty("saved",piece.createTag(null).toString());output.println(row);
    }
    return;
   }
   var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
   net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
   try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())){
    var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
    var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
    var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
    var access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess();
    var height=LevelHeightAccessor.create(-64,384);
    var serialization=new StructurePieceSerializationContext(null,access,null);
    var settings=access.lookupOrThrow(Registries.NOISE_SETTINGS).getOrThrow(NoiseGeneratorSettings.OVERWORLD);
    for(long seed:new long[]{0L,42L,Long.MIN_VALUE}){
     var random=RandomState.create(access,NoiseGeneratorSettings.OVERWORLD,seed);
     for(String name:new String[]{"mineshaft","mineshaft_mesa"}){
      var selected=access.lookupOrThrow(Registries.STRUCTURE).getOrThrow(ResourceKey.create(Registries.STRUCTURE,Identifier.withDefaultNamespace(name)));
      var structure=selected.value();
      for(var biomeKey:List.of(Biomes.PLAINS,Biomes.BADLANDS)){
       var source=new FixedBiomeSource(access.lookupOrThrow(Registries.BIOME).getOrThrow(biomeKey));
       var generator=new NoiseBasedChunkGenerator(source,settings);
       for(int[] pos:new int[][]{{0,0},{4,0},{-12,7}}){
        var chunk=new ChunkPos(pos[0],pos[1]);
        var context=new Structure.GenerationContext(access,generator,source,random,null,seed,chunk,height,structure.biomes()::contains);
        var stub=structure.findValidGenerationPoint(context);
        var start=stub.isPresent()?new StructureStart(structure,chunk,5,stub.get().getPiecesBuilder().build()):StructureStart.INVALID_START;
        var direct=structure.generate(selected,Level.OVERWORLD,access,generator,source,random,null,seed,chunk,5,height,structure.biomes()::contains);
        var saved=start.createTag(serialization,chunk).toString();
        if(!saved.equals(direct.createTag(serialization,chunk).toString()))throw new AssertionError("genuine generate disagrees");
        var row=new JsonObject();row.addProperty("seed",seed);row.addProperty("structure",name);row.addProperty("biome",biomeKey.identifier().toString());row.addProperty("x",chunk.x());row.addProperty("z",chunk.z());row.addProperty("valid",start.isValid());row.addProperty("piece_count",start.getPieces().size());row.addProperty("rng_next",context.random().nextLong());row.addProperty("saved",saved);
        if(stub.isPresent()){var p=stub.get().position();row.add("generation_point",new Gson().toJsonTree(new int[]{p.getX(),p.getY(),p.getZ()}));}
        output.println(row);output.flush();
       }
      }
     }
    }
   }
  }
 }
}
