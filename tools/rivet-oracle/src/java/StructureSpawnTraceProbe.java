import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.presets.WorldPresets;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.pieces.*;
import net.minecraft.world.level.levelgen.structure.templatesystem.*;
import net.minecraft.world.level.levelgen.structure.templatesystem.loader.*;

/** Actual seed42 NORMAL structure-state, placement, weighted retries and body generation.
 * No Bukkit plugin manager is supplied. A valid body is captured before the spawn
 * event using a sentinel, recorded in the disposable ProtoChunk, then orchestration
 * resumes; completed invalid body callbacks are memoized across that resume.
 * No piece postProcess/terrain writes, event acceptance or gameplay spawn is claimed.
 * TemplateManager shell uses genuine vanilla resource loaders, without world custom
 * templates/directories. Spigot seeds are initialized by the actual source method.
 */
public final class StructureSpawnTraceProbe {
    static final long SEED=42L;
    static final int CENTER_X=0,CENTER_Z=0,RADIUS=8;
    static final sun.misc.Unsafe UNSAFE;
    static {
        try {var f=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");f.setAccessible(true);UNSAFE=(sun.misc.Unsafe)f.get(null);}
        catch(Exception e){throw new ExceptionInInitializerError(e);}
    }
    static void put(Object target,String name,Object value)throws Exception {
        var f=target.getClass().getDeclaredField(name);f.setAccessible(true);f.set(target,value);
    }
    static final class Captured extends RuntimeException {}
    static JsonArray attempts;
    static ProtoChunk current;
    static StructurePieceSerializationContext serialization;
    static final class TraceStructure extends Structure {
        final Holder<Structure> original;
        final Identifier key;
        boolean attempted;
        TraceStructure(Holder<Structure> original) {
            super(new StructureSettings(original.value().biomes()));
            this.original=original;this.key=original.unwrapKey().orElseThrow().identifier();
        }
        @Override public StructureStart generate(Holder<Structure> selected,ResourceKey<Level> dimension,
            RegistryAccess access,ChunkGenerator generator,BiomeSource source,RandomState random,
            StructureTemplateManager templates,long seed,ChunkPos chunk,int references,
            LevelHeightAccessor height,java.util.function.Predicate<Holder<Biome>> validBiome) {
            if(attempted)return StructureStart.INVALID_START;
            attempted=true;
            var row=new JsonObject();row.addProperty("structure",key.toString());
            row.addProperty("type",BuiltInRegistries.STRUCTURE_TYPE.getKey(original.value().type()).toString());
            var checks=new JsonArray();row.add("biome_checks",checks);attempts.add(row);
            var start=original.value().generate(original,dimension,access,generator,source,random,templates,seed,chunk,references,height,biome->{
                boolean accepted=original.value().biomes().contains(biome);
                var check=new JsonObject();check.addProperty("biome",biome.unwrapKey().orElseThrow().identifier().toString());check.addProperty("accepted",accepted);checks.add(check);
                return accepted;
            });
            row.addProperty("valid",start.isValid());
            row.addProperty("piece_count",start.getPieces().size());
            row.addProperty("saved",start.createTag(serialization,chunk).toString());
            if(start.isValid()){
                var b=start.getBoundingBox();var bounds=new JsonArray();
                for(int v:new int[]{b.minX(),b.minY(),b.minZ(),b.maxX(),b.maxY(),b.maxZ()})bounds.add(v);
                row.add("bounds",bounds);
                current.setStartForStructure(this,start);
                throw new Captured();
            }
            return start;
        }
        @Override protected Optional<GenerationStub> findGenerationPoint(GenerationContext context){throw new AssertionError("body delegated");}
        @Override public StructureType<?> type(){return original.value().type();}
    }
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
                var preset=access.lookupOrThrow(Registries.WORLD_PRESET).getOrThrow(WorldPresets.NORMAL);
                var generator=(NoiseBasedChunkGenerator)preset.value().createWorldDimensions().overworld();
                var source=generator.getBiomeSource();var random=RandomState.create(access,NoiseGeneratorSettings.OVERWORLD,SEED);
                if(!(source instanceof MultiNoiseBiomeSource))throw new AssertionError("not real OVERWORLD MultiNoiseBiomeSource");
                var conf=(org.spigotmc.SpigotWorldConfig)UNSAFE.allocateInstance(org.spigotmc.SpigotWorldConfig.class);
                put(conf,"config",new org.bukkit.configuration.file.YamlConfiguration());put(conf,"worldName","minecraft:overworld");
                var init=org.spigotmc.SpigotWorldConfig.class.getDeclaredMethod("initWorldGenSeeds");init.setAccessible(true);init.invoke(conf);
                var state=ChunkGeneratorStructureState.createForNormal(random,SEED,source,access.lookupOrThrow(Registries.STRUCTURE_SET),conf);
                state.ensureStructuresGenerated();
                var templateSource=new ResourceManagerTemplateSource(net.minecraft.util.datafix.DataFixers.getDataFixer(),access.lookupOrThrow(Registries.BLOCK),resources,new FileToIdConverter("structure",".nbt"));
                var templates=(StructureTemplateManager)UNSAFE.allocateInstance(StructureTemplateManager.class);
                put(templates,"structureRepository",new java.util.concurrent.ConcurrentHashMap<Identifier,Optional<StructureTemplate>>());
                put(templates,"resourceManagerSource",templateSource);put(templates,"sources",List.of(templateSource));
                serialization=new StructurePieceSerializationContext(null,access,templates);
                var originalSets=state.possibleStructureSets();var wrappedSets=new ArrayList<Holder<StructureSet>>();
                var wrappers=new ArrayList<TraceStructure>();var setNames=new ArrayList<String>();
                var metadata=new JsonObject();metadata.addProperty("kind","metadata");metadata.addProperty("seed",SEED);
                metadata.addProperty("paper","26.2-DEV-main@0a99345");metadata.addProperty("generator","minecraft:normal");
                metadata.addProperty("spawn_source","ServerLevelConfig::default; configured Rivet spawn, NOT Paper spawn search");
                metadata.addProperty("capture_boundary","genuine structure bodies and createStructures weighted retry; stop before Bukkit event; no piece postProcess or terrain writes");
                metadata.addProperty("template_source","actual vanilla resource manager loader; no world custom-template directory");
                var seedsJson=new JsonObject();
                var seedFields=Arrays.stream(org.spigotmc.SpigotWorldConfig.class.getFields()).filter(f->f.getName().endsWith("Seed")).sorted(Comparator.comparing(Field::getName)).toList();
                for(var field:seedFields)seedsJson.add(field.getName(),new Gson().toJsonTree(field.get(conf)));
                metadata.add("actual_spigot_defaults",seedsJson);
                metadata.add("configured_spawn",new Gson().toJsonTree(new int[]{0,-63,0}));
                metadata.addProperty("center_x",CENTER_X);metadata.addProperty("center_z",CENTER_Z);metadata.addProperty("radius",RADIUS);
                var setsJson=new JsonArray();metadata.add("possible_structure_sets",setsJson);
                for(var set:originalSets){
                    var placement=set.value().placement();
                    var key=placement instanceof ChunkGeneratorStructureState.KeyedRandomSpreadStructurePlacement p?p.key.identifier():set.unwrapKey().orElseThrow().identifier();
                    setNames.add(key.toString());
                    var entries=new ArrayList<StructureSet.StructureSelectionEntry>();
                    for(var entry:set.value().structures()){
                        var wrapper=new TraceStructure(entry.structure());wrappers.add(wrapper);
                        entries.add(StructureSet.entry(Holder.direct(wrapper),entry.weight()));
                    }
                    wrappedSets.add(Holder.direct(new StructureSet(entries,placement)));
                    var setJson=new JsonObject();setJson.addProperty("key",key.toString());
                    setJson.add("placement",net.minecraft.world.level.levelgen.structure.placement.StructurePlacement.CODEC.encodeStart(RegistryOps.create(com.mojang.serialization.JsonOps.INSTANCE,access),placement).getOrThrow());
                    setsJson.add(setJson);
                }
                put(state,"possibleStructureSets",List.copyOf(wrappedSets));
                metadata.addProperty("structure_count",access.lookupOrThrow(Registries.STRUCTURE).size());
                metadata.addProperty("structure_set_count",access.lookupOrThrow(Registries.STRUCTURE_SET).size());
                output.println(metadata);output.flush();
                var factory=PalettedContainerFactory.create(access);
                for(int x=CENTER_X-RADIUS;x<=CENTER_X+RADIUS;x++)for(int z=CENTER_Z-RADIUS;z<=CENTER_Z+RADIUS;z++){
                    for(var wrapper:wrappers)wrapper.attempted=false;
                    var chunk=new ChunkPos(x,z);current=new ProtoChunk(chunk,UpgradeData.EMPTY,LevelHeightAccessor.create(-64,384),factory,null);
                    attempts=new JsonArray();var hits=new JsonArray();
                    for(int i=0;i<originalSets.size();i++){
                        var p=originalSets.get(i).value().placement();
                        var key=p instanceof ChunkGeneratorStructureState.KeyedRandomSpreadStructurePlacement k?k.key:null;
                        if(p.isStructureChunk(state,x,z,key))hits.add(setNames.get(i));
                    }
                    int resumes=0;
                    while(true){
                        try{generator.createStructures(access,state,new StructureManager(null,new WorldOptions(SEED,true,false),null),current,templates,Level.OVERWORLD);break;}
                        catch(Captured done){if(++resumes>originalSets.size())throw new AssertionError("resume did not converge");}
                    }
                    var row=new JsonObject();row.addProperty("kind","chunk");row.addProperty("seed",SEED);row.addProperty("x",x);row.addProperty("z",z);row.add("placement_hits",hits);row.add("attempts",attempts);output.println(row);output.flush();
                }
            }
        }
    }
}
