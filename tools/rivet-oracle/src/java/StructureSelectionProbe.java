import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.Constructor;
import java.util.*;
import java.util.stream.Stream;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.pieces.*;
import net.minecraft.world.level.levelgen.structure.placement.*;
import net.minecraft.world.level.levelgen.structure.structures.BuriedTreasurePieces;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplateManager;

/** Genuine ChunkGenerator.createStructures orchestration. Only generation
 * callbacks are replaced: invalid stubs and a terminal sentinel capture the
 * actual weighted retry order without inventing Bukkit event authority. */
public final class StructureSelectionProbe {
    static final List<Integer> attempts = new ArrayList<>();
    static int stopIndex;
    static final class Stop extends RuntimeException {}
    static final class Candidate extends Structure {
        final int index;
        Candidate(int index, Holder<Biome> biome) { super(new StructureSettings(HolderSet.direct(biome))); this.index=index; }
        @Override public StructureStart generate(Holder<Structure> selected, ResourceKey<Level> dimension,
            RegistryAccess access, ChunkGenerator generator, BiomeSource source, RandomState random,
            StructureTemplateManager templates, long seed, ChunkPos chunk, int references,
            LevelHeightAccessor height, java.util.function.Predicate<Holder<Biome>> validBiome) {
            attempts.add(index);
            if (index == stopIndex) throw new Stop();
            return StructureStart.INVALID_START;
        }
        @Override protected Optional<GenerationStub> findGenerationPoint(GenerationContext context) { throw new AssertionError("generation intercepted"); }
        @Override public StructureType<?> type() { return StructureType.BURIED_TREASURE; }
    }
    static final class Everywhere extends StructurePlacement {
        Everywhere() { super(Vec3i.ZERO, FrequencyReductionMethod.DEFAULT,1.0F,0,Optional.empty()); }
        @Override protected boolean isPlacementChunk(ChunkGeneratorStructureState state,int x,int z) { return true; }
        @Override public StructurePlacementType<?> type() { return StructurePlacementType.RANDOM_SPREAD; }
    }
    public static void main(String[] args) throws Exception {
        try (var output = new PrintStream(args[0])) {
            SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
            var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
            net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
            try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())) {
                var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
                var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
                var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
                var access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess();
                var biome=access.lookupOrThrow(Registries.BIOME).getOrThrow(Biomes.PLAINS);
                var source=new FixedBiomeSource(biome);
                var settings=access.lookupOrThrow(Registries.NOISE_SETTINGS).getOrThrow(NoiseGeneratorSettings.OVERWORLD);
                var generator=new NoiseBasedChunkGenerator(source,settings);
                var factory=PalettedContainerFactory.create(access);
                for(long seed:new long[]{0,42,-123456789,Long.MIN_VALUE}) for(int[] pos:new int[][]{{0,0},{-1,-1},{123,-456},{Integer.MIN_VALUE,Integer.MAX_VALUE}})
                    for(int[] weights:new int[][]{{1},{1,1},{1,2,7},{7,3,2,1},{Integer.MAX_VALUE,1}})
                        for(int mode=0;mode<4;mode++) {
                            attempts.clear(); stopIndex=mode==1?weights.length-1:-1;
                            var structures=new ArrayList<StructureSet.StructureSelectionEntry>();
                            for(int i=0;i<weights.length;i++) structures.add(StructureSet.entry(Holder.direct(new Candidate(i,biome)),weights[i]));
                            var set=Holder.direct(new StructureSet(structures,new Everywhere()));
                            var chunkPos=new ChunkPos(pos[0],pos[1]);
                            var chunk=new ProtoChunk(chunkPos,UpgradeData.EMPTY,LevelHeightAccessor.create(-64,384),factory,null);
                            if(mode==2) chunk.setStartForStructure(structures.get(0).structure().value(),new StructureStart(structures.get(0).structure().value(),chunkPos,0,
                                new PiecesContainer(List.of(new BuriedTreasurePieces.BuriedTreasurePiece(new BlockPos(0,90,0))))));
                            var state=ChunkGeneratorStructureState.createForFlat(null,seed,source,mode==3?Stream.of(set,set):Stream.of(set),null);
                            String result="exhausted";
                            try { generator.createStructures(access,state,new StructureManager(null,new WorldOptions(seed,true,false),null),chunk,null,Level.OVERWORLD); }
                            catch(Stop stopped) {result="stopped";}
                            catch(IllegalArgumentException invalid) {result="invalid_bound";}
                            var row=new JsonObject();row.addProperty("seed",seed);row.addProperty("x",pos[0]);row.addProperty("z",pos[1]);
                            row.add("weights",new Gson().toJsonTree(weights));row.addProperty("stop_index",stopIndex);row.addProperty("existing",mode==2);row.addProperty("set_count",mode==3?2:1);
                            row.addProperty("result",result);row.add("attempts",new Gson().toJsonTree(attempts));output.println(row);
                        }
            }
        }
    }
}
