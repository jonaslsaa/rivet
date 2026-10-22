import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.Proxy;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.biome.*;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.pieces.PiecesContainer;
import net.minecraft.world.level.levelgen.structure.structures.BuriedTreasurePieces;

/** Actual ChunkGenerator.createReferences + StructureManager reference writes.
 * Proxy supplies only canonical chunk lookup and records its exact read order.
 * Each source has one structure key: no Java identity-map order is invented. */
public final class StructureReferencesProbe {
    static JsonArray pair(int x,int z) { var a=new JsonArray();a.add(x);a.add(z);return a; }
    public static void main(String[] args) throws Exception {
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
                var generator=new NoiseBasedChunkGenerator(source,access.lookupOrThrow(Registries.NOISE_SETTINGS).getOrThrow(NoiseGeneratorSettings.OVERWORLD));
                var factory=PalettedContainerFactory.create(access);
                var structure=access.lookupOrThrow(Registries.STRUCTURE).getValue(net.minecraft.resources.Identifier.parse("minecraft:buried_treasure"));
                for(var center:List.of(new ChunkPos(0,0),new ChunkPos(-3,5))) for(boolean preexisting:List.of(false,true)) {
                    var chunks=new HashMap<Long,ProtoChunk>();
                    for(int dx=-8;dx<=8;dx++)for(int dz=-8;dz<=8;dz++) {
                        var pos=new ChunkPos(center.x()+dx,center.z()+dz);
                        var chunk=new ProtoChunk(pos,UpgradeData.EMPTY,LevelHeightAccessor.create(-64,384),factory,null);
                        chunk.setPersistedStatus(ChunkStatus.STRUCTURE_STARTS);chunks.put(pos.pack(),chunk);
                    }
                    int x=center.getMinBlockX(),z=center.getMinBlockZ();
                    int[][] inputs={{-8,-8,x-4,-900,z-4,x,-899,z,7},{-1,1,x+15,900,z+15,x+18,910,z+18,2},{8,8,x+16,0,z+16,x+20,8,z+20,1},{0,0,0,0,0,0,0,0,0}};
                    var inputRows=new JsonArray();
                    for(int[] input:inputs) {
                        var pos=new ChunkPos(center.x()+input[0],center.z()+input[1]);
                        var row=new JsonObject();row.add("pos",pair(pos.x(),pos.z()));row.addProperty("references",input[8]);
                        boolean invalid=input[0]==0&&input[1]==0;row.addProperty("invalid",invalid);
                        var box=new int[]{input[2],input[3],input[4],input[5],input[6],input[7]};var bounds=new JsonArray();for(int n:box)bounds.add(n);row.add("bounds",bounds);inputRows.add(row);
                        var tag=new CompoundTag();tag.putString("id","minecraft:btp");tag.putIntArray("BB",box);tag.putInt("O",-1);tag.putInt("GD",0);
                        var start=invalid?StructureStart.INVALID_START:new StructureStart(structure,pos,input[8],new PiecesContainer(List.of(new BuriedTreasurePieces.BuriedTreasurePiece(tag))));
                        chunks.get(pos.pack()).setStartForStructure(structure,start);
                    }
                    var target=chunks.get(center.pack());
                    if(preexisting)target.addReferenceForStructure(structure,new ChunkPos(center.x()-8,center.z()-8).pack());
                    target.tryMarkSaved();
                    var reads=new JsonArray();var writes=new JsonArray();
                    WorldGenLevel level=(WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},(proxy,method,values)->switch(method.getName()) {
                        case "getChunk" -> {int sx=(int)values[0],sz=(int)values[1];reads.add(pair(sx,sz));yield chunks.get(ChunkPos.pack(sx,sz));}
                        default -> throw new UnsupportedOperationException(method.toString());
                    });
                    var manager=new StructureManager(level,new WorldOptions(42,true,false),null) {
                        @Override public void addReferenceForStructure(SectionPos pos,Structure value,long reference,StructureAccess chunk) {
                            var row=new JsonArray();row.add(reference);writes.add(row);super.addReferenceForStructure(pos,value,reference,chunk);
                        }
                    };
                    generator.createReferences(level,manager,target);
                    var refs=new JsonArray();for(long reference:target.getReferencesForStructure(structure))refs.add(reference);
                    var row=new JsonObject();row.add("center",pair(center.x(),center.z()));row.addProperty("preexisting",preexisting);row.add("inputs",inputRows);row.add("reads",reads);row.add("writes",writes);row.add("references",refs);row.addProperty("dirty",target.isUnsaved());row.addProperty("status",target.getPersistedStatus().getName());output.println(row);
                }
            }
        }
    }
}
