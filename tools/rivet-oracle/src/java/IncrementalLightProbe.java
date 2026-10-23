import com.google.gson.*;
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

/** Real blocksChangedInChunk over retained sky/block nibbles, never relight. */
public final class IncrementalLightProbe {
    public static void main(String[] args) {
        var out=System.out;var errors=System.err;
        try {
            SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
            var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
            net.minecraft.server.MinecraftServer.configurePackRepository(packs,new WorldDataConfiguration(new DataPackConfig(List.of("vanilla"),List.of()),net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
            try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())) {
                var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
                var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));
                var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();
                GeneratedBlockLightProbe.factory=PalettedContainerFactory.create(layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess());
                for(String name:List.of("add_torch","remove_torch","replace_emitter","insert_roof","remove_roof","section_only","edge_add","edge_remove","unchanged","check_unchanged_torch","check_unchanged_dark"))
                    out.println("INCREMENTAL_LIGHT\t"+runIncremental(name));
            }
        } catch(Throwable error) {error.printStackTrace(errors);System.exit(1);}
    }
    static JsonObject runIncremental(String name)throws Exception {
        var chunks=new LinkedHashMap<ChunkPos,ProtoChunk>();chunks.put(new ChunkPos(0,0),GeneratedBlockLightProbe.chunk(0,0));
        if(name.startsWith("edge_")) chunks.put(new ChunkPos(1,0),GeneratedBlockLightProbe.chunk(1,0));
        var initial=new JsonArray();var changes=new JsonArray();
        java.util.function.BiConsumer<BlockPos,BlockState> putInitial=(pos,state)->{
            chunks.get(new ChunkPos(pos.getX()>>4,pos.getZ()>>4)).setBlockState(pos,state,2);initial.add(cell(pos,state));
        };
        var pos=new BlockPos(name.startsWith("edge_")?15:8,64,8);
        if(name.equals("remove_torch")||name.equals("replace_emitter")||name.equals("edge_remove")) putInitial.accept(pos,Blocks.GLOWSTONE.defaultBlockState());
        if(name.equals("remove_roof")) putInitial.accept(pos,Blocks.STONE.defaultBlockState());
        if(name.equals("check_unchanged_torch")) putInitial.accept(pos,Blocks.TORCH.defaultBlockState());
        if(name.equals("check_unchanged_dark")) putInitial.accept(pos.below(),Blocks.STONE.defaultBlockState());
        var level=GeneratedBlockLightProbe.heightOnlyLevel();
        var notifications=new JsonArray();
        var access=new LightChunkGetter(){
            public LightChunk getChunkForLighting(int x,int z){return chunks.get(new ChunkPos(x,z));}
            public BlockGetter getLevel(){return level;}
            public void onLightUpdate(LightLayer layer,SectionPos section){
                var event=new JsonArray();event.add(layer.name());event.add(section.x());event.add(section.y());event.add(section.z());notifications.add(event);
            }
        };
        var sky=new SkyStarLightEngine();sky.setWorld(level);
        var block=new BlockStarLightEngine();block.setWorld(level);
        for(var chunk:chunks.values()) {
            sky.light(access,chunk,StarLightEngine.getEmptySectionsForChunk(chunk));
            block.light(access,chunk,StarLightEngine.getEmptySectionsForChunk(chunk));
            chunk.setPersistedStatus(ChunkStatus.FULL);chunk.setLightCorrect(true);
        }
        while(notifications.size()!=0)notifications.remove(notifications.size()-1);
        var center=chunks.get(new ChunkPos(0,0));var changedSections=new Boolean[24];var positions=new HashSet<BlockPos>();
        if(!name.equals("unchanged")) {
            var state=switch(name) {
                case "add_torch","edge_add","replace_emitter","check_unchanged_torch" -> Blocks.TORCH.defaultBlockState();
                case "insert_roof","section_only" -> Blocks.STONE.defaultBlockState();
                default -> Blocks.AIR.defaultBlockState();
            };
            var section=center.getSection(center.getSectionIndex(pos.getY()));var wasEmpty=section.hasOnlyAir();
            // The light probe owns only section storage and explicit queue
            // input, not ProtoChunk's separate LevelLightEngine facade.
            section.setBlockState(pos.getX()&15,pos.getY()&15,pos.getZ()&15,state);changes.add(cell(pos,state));
            if(wasEmpty!=section.hasOnlyAir()) changedSections[center.getSectionIndex(pos.getY())]=section.hasOnlyAir();
            if(!name.equals("section_only")) positions.add(pos);
        }
        sky.blocksChangedInChunk(access,0,0,positions,changedSections);
        block.blocksChangedInChunk(access,0,0,positions,changedSections);
        var row=new JsonObject();row.addProperty("name",name);row.add("initial",initial);row.add("changes",changes);
        row.add("notifications",notifications);
        row.addProperty("block_changes",!name.equals("section_only")&&!name.equals("unchanged"));
        row.add("changed_sections",new Gson().toJsonTree(changedSections));
        var outputs=new JsonArray();
        for(var chunk:chunks.values()) {
            var output=new JsonObject();output.addProperty("x",chunk.getPos().x());output.addProperty("z",chunk.getPos().z());
            var star=(StarlightChunk)chunk;
            output.add("sky",nibbles(star.starlight$getSkyNibbles()));output.add("block",nibbles(star.starlight$getBlockNibbles()));
            output.add("sky_emptiness",new Gson().toJsonTree(star.starlight$getSkyEmptinessMap()));
            output.add("block_emptiness",new Gson().toJsonTree(star.starlight$getBlockEmptinessMap()));outputs.add(output);
        }
        row.add("chunks",outputs);return row;
    }
    static JsonArray cell(BlockPos pos,BlockState state) {
        var cell=new JsonArray();cell.add(pos.getX());cell.add(pos.getY());cell.add(pos.getZ());cell.add(Block.getId(state));return cell;
    }
    static JsonArray nibbles(SWMRNibbleArray[] nibbles)throws Exception {
        var sections=new JsonArray();
        for(var nibble:nibbles) {
            var section=new JsonObject();section.addProperty("state",nibble.isNullNibbleVisible()?0:nibble.isUninitialisedVisible()?1:nibble.isInitialisedVisible()?2:3);
            byte[] values=new byte[4096];for(int i=0;i<4096;i++)values[i]=(byte)nibble.getVisible(i);
            section.addProperty("sha256",GeneratedBlockLightProbe.hash(values));sections.add(section);
        }
        return sections;
    }
}
