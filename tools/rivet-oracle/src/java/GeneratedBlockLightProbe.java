import com.google.gson.*;
import java.security.MessageDigest;
import java.util.*;
import ca.spottedleaf.moonrise.patches.starlight.light.*;
import ca.spottedleaf.moonrise.patches.starlight.chunk.StarlightChunk;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.*;
import net.minecraft.world.level.chunk.status.ChunkStatus;

/** Real BlockStarLightEngine over real ProtoChunks and a height-only Level shell. */
public final class GeneratedBlockLightProbe {
    static final LevelHeightAccessor HEIGHT = LevelHeightAccessor.create(-64,384);
    static PalettedContainerFactory factory;
    static Level heightOnlyLevel()throws Exception {
        var field=sun.misc.Unsafe.class.getDeclaredField("theUnsafe");field.setAccessible(true);
        var unsafe=(sun.misc.Unsafe)field.get(null);
        var level=(Level)unsafe.allocateInstance(net.minecraft.server.level.ServerLevel.class);
        // Concrete Level is required by Starlight. Only its cached height and
        // client-side getters are used; blocks live in real ProtoChunks below.
        for(var value:Map.of("minY",-64,"height",384,"maxY",319,"minSectionY",-4,"maxSectionY",19,"sectionsCount",24).entrySet()) {
            var heightField=Level.class.getDeclaredField(value.getKey());heightField.setAccessible(true);heightField.setInt(level,value.getValue());
        }
        return level;
    }
    static ProtoChunk chunk(int x,int z) {return new ProtoChunk(new ChunkPos(x,z),UpgradeData.EMPTY,HEIGHT,factory,null);}
    static String hash(byte[] bytes)throws Exception {return HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(bytes));}
    public static void main(String[] args) {
        var errors=System.err;
        try { runAll(); } catch(Throwable error) { error.printStackTrace(errors);System.exit(1); }
    }
    static void runAll()throws Exception {
        var out=System.out;
        SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
        var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
        try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())) {
            var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
            var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
            var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
            factory=PalettedContainerFactory.create(layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess());
            for(String scenario:List.of("empty","torch","glowstone","lava","overlap","stone_wall","water_wall","negative_y","bottom","top","unlit_neighbor","lit_neighbor","low_status_lit_neighbor","edge_source")) out.println("BLOCK_LIGHT\t"+run(scenario));
        }
    }
    static JsonObject run(String scenario)throws Exception {
        var chunks=new LinkedHashMap<ChunkPos,ProtoChunk>();var center=chunk(0,0);chunks.put(center.getPos(),center);
        if(scenario.contains("neighbor")||scenario.equals("edge_source")){var neighbor=chunk(1,0);chunks.put(neighbor.getPos(),neighbor);}
        var blocks=new JsonArray();
        java.util.function.BiConsumer<BlockPos,BlockState> put=(pos,state)->{
            chunks.get(new ChunkPos(pos.getX()>>4,pos.getZ()>>4)).setBlockState(pos,state,2);
            var cell=new JsonArray();cell.add(pos.getX());cell.add(pos.getY());cell.add(pos.getZ());cell.add(Block.getId(state));blocks.add(cell);
        };
        if(scenario.contains("neighbor")) put.accept(new BlockPos(16,64,8),Blocks.GLOWSTONE.defaultBlockState());
        else if(!scenario.equals("empty")) {
            int y=switch(scenario){case "negative_y"->-17;case "bottom"->-64;case "top"->319;default->64;};
            Block source=switch(scenario){case "glowstone","edge_source"->Blocks.GLOWSTONE;case "lava"->Blocks.LAVA;default->Blocks.TORCH;};
            put.accept(new BlockPos(scenario.equals("edge_source")?15:8,y,8),source.defaultBlockState());
        }
        if(scenario.equals("overlap")) put.accept(new BlockPos(11,64,8),Blocks.GLOWSTONE.defaultBlockState());
        if(scenario.endsWith("_wall")) for(int y=50;y<80;y++) for(int z=0;z<16;z++) put.accept(new BlockPos(9,y,z),(scenario.equals("water_wall")?Blocks.WATER:Blocks.STONE).defaultBlockState());
        Level level=heightOnlyLevel();
        LightChunkGetter access=new LightChunkGetter(){
            public LightChunk getChunkForLighting(int x,int z){return chunks.get(new ChunkPos(x,z));}
            public BlockGetter getLevel(){return level;}
        };
        var engine=new BlockStarLightEngine();engine.setWorld(level);
        if(scenario.equals("lit_neighbor")||scenario.equals("low_status_lit_neighbor")||scenario.equals("edge_source")){
            var neighbor=chunks.get(new ChunkPos(1,0));
            engine.light(access,neighbor,StarLightEngine.getEmptySectionsForChunk(neighbor));
            neighbor.setPersistedStatus(scenario.equals("low_status_lit_neighbor")?ChunkStatus.FEATURES:ChunkStatus.LIGHT);neighbor.setLightCorrect(true);
        }
        engine.light(access,center,StarLightEngine.getEmptySectionsForChunk(center));
        var row=new JsonObject();row.addProperty("scenario",scenario);row.add("blocks",blocks);
        var outputs=new JsonArray();
        for(var chunk:chunks.values()) {
            var output=new JsonObject();output.addProperty("x",chunk.getPos().x());output.addProperty("z",chunk.getPos().z());
            var nibbles=((StarlightChunk)chunk).starlight$getBlockNibbles();var sections=new JsonArray();
            for(var nibble:nibbles){var section=new JsonObject();int state=nibble.isNullNibbleVisible()?0:nibble.isUninitialisedVisible()?1:nibble.isInitialisedVisible()?2:3;section.addProperty("state",state);byte[] values=new byte[4096];for(int i=0;i<4096;i++)values[i]=(byte)nibble.getVisible(i);section.addProperty("sha256",hash(values));sections.add(section);}
            output.add("sections",sections);output.add("emptiness",new Gson().toJsonTree(((StarlightChunk)chunk).starlight$getBlockEmptinessMap()));outputs.add(output);
        }
        row.add("chunks",outputs);return row;
    }
}
