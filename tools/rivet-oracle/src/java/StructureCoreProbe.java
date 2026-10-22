import com.google.gson.*;
import com.mojang.serialization.Lifecycle;
import java.lang.reflect.Proxy;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.Registries;
import net.minecraft.nbt.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.pieces.*;
import net.minecraft.world.level.levelgen.structure.structures.*;

/** Genuine Paper structure core. Proxy supplies world storage only. The
 * clipped treasure cases deliberately do not enter Bukkit's server-dependent
 * CraftChest boundary; positive chest authority is tested natively separately. */
public final class StructureCoreProbe {
    static JsonArray xyz(BlockPos p){var a=new JsonArray();a.add(p.getX());a.add(p.getY());a.add(p.getZ());return a;}
    static JsonArray bounds(BoundingBox b){var a=new JsonArray();for(int n:new int[]{b.minX(),b.minY(),b.minZ(),b.maxX(),b.maxY(),b.maxZ()})a.add(n);return a;}
    static CompoundTag tag(int[] box,int orientation){var t=new CompoundTag();t.putString("id","minecraft:btp");t.putIntArray("BB",box);t.putInt("O",orientation);t.putInt("GD",-7);return t;}
    public static void main(String[] args)throws Exception{
        var output=args.length==0?System.out:new java.io.PrintStream(args[0]);SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
        var worldPos=StructurePiece.class.getDeclaredMethod("getWorldPos",int.class,int.class,int.class);worldPos.setAccessible(true);
        int[][] boxes={{-5,-7,-9,4,8,12},{0,0,0,0,0,0},{Integer.MIN_VALUE,-2,-3,Integer.MAX_VALUE,2,3},{8,9,10,-8,-9,-10}};
        for(int[] box:boxes)for(int orientation:new int[]{-1,0,1,2,3,-5,6}){
            var input=tag(box,orientation);var piece=new BuriedTreasurePieces.BuriedTreasurePiece(input);
            var row=new JsonObject();row.addProperty("kind","piece");row.addProperty("input",input.toString());row.addProperty("saved",piece.createTag(null).toString());row.add("center",xyz(piece.getBoundingBox().getCenter()));row.add("world",xyz((BlockPos)worldPos.invoke(piece,2,-3,4)));output.println("STRUCTURE_CORE\t"+row);
        }
        var registry=new MappedRegistry<Structure>(Registries.STRUCTURE,Lifecycle.stable());
        var plain=new BuriedTreasureStructure(new Structure.StructureSettings(HolderSet.empty()));
        var adjusted=new BuriedTreasureStructure(new Structure.StructureSettings(HolderSet.empty(),Map.of(),GenerationStep.Decoration.SURFACE_STRUCTURES,TerrainAdjustment.BEARD_THIN));
        Registry.register(registry,"buried_treasure",plain);Registry.register(registry,"adjusted_treasure",adjusted);
        var access=new RegistryAccess.ImmutableRegistryAccess(List.of(registry.freeze())).freeze();
        var context=new StructurePieceSerializationContext(null,access,null);
        for(var structure:List.of(plain,adjusted))for(int references:new int[]{-1,0,1,Integer.MAX_VALUE}){
            var pieces=new PiecesContainer(List.of(new BuriedTreasurePieces.BuriedTreasurePiece(tag(new int[]{-5,60,-3,5,65,8},-1)),new BuriedTreasurePieces.BuriedTreasurePiece(new BlockPos(19,90,-20))));
            var start=new StructureStart(structure,new ChunkPos(-3,8),references,pieces);
            var row=new JsonObject();row.addProperty("kind","start");row.addProperty("id",registry.getKey(structure).toString());row.addProperty("references",references);row.addProperty("referencable",start.canBeReferenced());start.addReference();row.addProperty("after_reference",start.getReferences());row.add("bounds",bounds(start.getBoundingBox()));row.addProperty("saved",start.createTag(context,new ChunkPos(7,-9)).toString());row.addProperty("inside",pieces.isInsidePiece(new BlockPos(5,65,8)));output.println("STRUCTURE_CORE\t"+row);
        }
        var invalid=new JsonObject();invalid.addProperty("kind","invalid");invalid.addProperty("saved",StructureStart.INVALID_START.createTag(context,ChunkPos.ZERO).toString());output.println("STRUCTURE_CORE\t"+invalid);
        for(String terrain:List.of("sand","water","lava","void")){
            var states=new HashMap<BlockPos,BlockState>();var writes=new JsonArray();
            java.util.function.Function<BlockPos,BlockState> get=p->states.getOrDefault(p,(terrain.equals("void")?Blocks.AIR:p.getY()<63?Blocks.STONE:p.getY()==63?terrain.equals("water")?Blocks.WATER:terrain.equals("lava")?Blocks.LAVA:Blocks.SAND:Blocks.AIR).defaultBlockState());
            var level=(WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},(proxy,method,a)->switch(method.getName()){
                case "getHeight" -> 70;
                case "getMinY" -> -64;
                case "getBlockState" -> get.apply((BlockPos)a[0]);
                case "setBlock" -> {var p=((BlockPos)a[0]).immutable();var s=(BlockState)a[1];states.put(p,s);var w=xyz(p);w.add(Block.getId(s));w.add((int)a[2]);writes.add(w);yield true;}
                default -> throw new UnsupportedOperationException(method.toString());
            });
            var piece=new BuriedTreasurePieces.BuriedTreasurePiece(new BlockPos(0,90,0));var random=new XoroshiroRandomSource(42);
            var start=new StructureStart(plain,ChunkPos.ZERO,0,new PiecesContainer(List.of(piece)));var cachedBefore=start.getBoundingBox();
            piece.postProcess(level,null,null,random,new BoundingBox(-16,-64,-16,16,62,16),ChunkPos.ZERO,BlockPos.ZERO);
            var row=new JsonObject();row.addProperty("kind","buried_clipped");row.addProperty("terrain",terrain);row.add("writes",writes);row.addProperty("saved",piece.createTag(null).toString());row.addProperty("next_long",random.nextLong());row.addProperty("chest",Block.getId(StructurePiece.reorient(level,new BlockPos(0,63,0),Blocks.CHEST.defaultBlockState())));row.add("cached_before",bounds(cachedBefore));row.add("cached_after",bounds(start.getBoundingBox()));output.println("STRUCTURE_CORE\t"+row);
        }
        output.flush();if(args.length>0)output.close();
    }
}
