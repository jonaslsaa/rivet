import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.Field;
import java.util.*;
import java.util.function.Predicate;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.*;
import net.minecraft.world.level.block.Rotation;
import net.minecraft.world.level.levelgen.*;
import net.minecraft.world.level.levelgen.structure.*;
import net.minecraft.world.level.levelgen.structure.pieces.PiecesContainer;
import net.minecraft.world.level.levelgen.structure.pools.*;
import net.minecraft.world.level.levelgen.structure.structures.*;
import net.minecraft.world.level.levelgen.structure.templatesystem.LiquidSettings;

/** Genuine Paper Beardifier constructor, collector, compute, fillArray and kernel.
 * Collector fixture inputs are actual pieces/starts supplied in explicit order;
 * this probe does not pretend to implement world structure-reference traversal. */
public final class BeardifierProbe {
    static final Gson JSON=new Gson();
    static Object field(Object target,String name)throws Exception {
        Field f=Beardifier.class.getDeclaredField(name);f.setAccessible(true);return f.get(target);
    }
    static JsonArray bounds(BoundingBox b) {return b==null?null:JSON.toJsonTree(new int[]{b.minX(),b.minY(),b.minZ(),b.maxX(),b.maxY(),b.maxZ()}).getAsJsonArray();}
    static JsonObject rigid(Beardifier.Rigid r) {
        var value=new JsonObject();value.add("bounds",bounds(r.box()));value.addProperty("adjustment",r.terrainAdjustment().getSerializedName());value.addProperty("ground_delta",r.groundLevelDelta());return value;
    }
    static JsonArray junction(JigsawJunction j) {return JSON.toJsonTree(new int[]{j.getSourceX(),j.getSourceGroundY(),j.getSourceZ()}).getAsJsonArray();}
    static final class Manager extends StructureManager {
        final List<StructureStart> starts;
        Manager(List<StructureStart> starts) {super(null,null,null);this.starts=starts;}
        @Override public List<StructureStart> startsForStructure(ChunkPos chunk,Predicate<Structure> predicate) {
            return starts.stream().filter(start->predicate.test(start.getStructure())).toList();
        }
    }
    static StructurePiece ordinary(BoundingBox bounds)throws Exception {
        var piece=new BuriedTreasurePieces.BuriedTreasurePiece(BlockPos.ZERO);
        var field=StructurePiece.class.getDeclaredField("boundingBox");field.setAccessible(true);field.set(piece,bounds);return piece;
    }
    static PoolElementStructurePiece pool(BoundingBox bounds,StructureTemplatePool.Projection projection,int delta,List<JigsawJunction> junctions) {
        var element=StructurePoolElement.single("probe:unread_template").apply(projection);
        var piece=new PoolElementStructurePiece(null,element,BlockPos.ZERO,delta,Rotation.NONE,bounds,LiquidSettings.APPLY_WATERLOGGING);
        junctions.forEach(piece::addJunction);return piece;
    }
    static StructureStart start(TerrainAdjustment adjustment,List<StructurePiece> pieces) {
        var structure=new BuriedTreasureStructure(new Structure.StructureSettings(HolderSet.empty(),Map.of(),GenerationStep.Decoration.SURFACE_STRUCTURES,adjustment));
        return new StructureStart(structure,new ChunkPos(0,0),0,new PiecesContainer(pieces));
    }
    static JsonObject startInput(StructureStart start) {
        var row=new JsonObject();row.addProperty("adjustment",start.getStructure().terrainAdaptation().getSerializedName());var pieces=new JsonArray();
        for(var piece:start.getPieces()) {
            var p=new JsonObject();p.add("bounds",bounds(piece.getBoundingBox()));
            if(piece instanceof PoolElementStructurePiece pool) {
                p.addProperty("projection",pool.getElement().getProjection().getName());p.addProperty("ground_delta",pool.getGroundLevelDelta());
                var junctions=new JsonArray();pool.getJunctions().forEach(j->junctions.add(junction(j)));p.add("junctions",junctions);
            }
            pieces.add(p);
        }
        row.add("pieces",pieces);return row;
    }
    static void capture(PrintStream output,String name,Beardifier beard,int x,int z,JsonObject input)throws Exception {
        var row=new JsonObject();row.addProperty("kind","samples");row.addProperty("name",name);row.add("input",input);
        var rigids=new JsonArray();for(var r:(List<Beardifier.Rigid>)field(beard,"pieces"))rigids.add(rigid(r));row.add("rigids",rigids);
        var junctions=new JsonArray();for(var j:(List<JigsawJunction>)field(beard,"junctions"))junctions.add(junction(j));row.add("junctions",junctions);
        var box=(BoundingBox)field(beard,"affectedBox");row.add("affected",box==null?JsonNull.INSTANCE:bounds(box));
        var points=new ArrayList<DensityFunction.SinglePointContext>();
        for(int dx:new int[]{-25,-13,-12,-1,0,1,8,11,12,15,27,40}) for(int y:new int[]{-100,48,52,58,63,64,65,70,75,76,77,88,100})
            points.add(new DensityFunction.SinglePointContext(x+dx,y,z+(dx%4)));
        points.add(new DensityFunction.SinglePointContext(Integer.MIN_VALUE,Integer.MAX_VALUE,Integer.MAX_VALUE));
        points.add(new DensityFunction.SinglePointContext(Integer.MAX_VALUE,Integer.MIN_VALUE,Integer.MIN_VALUE));
        var coordinates=new JsonArray();var bits=new JsonArray();for(var point:points) {
            coordinates.add(JSON.toJsonTree(new int[]{point.blockX(),point.blockY(),point.blockZ()}));bits.add(Long.toUnsignedString(Double.doubleToRawLongBits(beard.compute(point))));
        }
        row.add("points",coordinates);row.add("bits",bits);
        int[] calls={0};double[] filled=new double[points.size()];Arrays.fill(filled,Double.NaN);
        beard.fillArray(filled,new DensityFunction.ContextProvider() {
            @Override public DensityFunction.FunctionContext forIndex(int index) {return points.get(index);}
            @Override public void fillAllDirectly(double[] values,DensityFunction function) {calls[0]++;for(int i=0;i<values.length;i++)values[i]=function.compute(forIndex(i));}
        });
        var fillBits=new JsonArray();for(double value:filled)fillBits.add(Long.toUnsignedString(Double.doubleToRawLongBits(value)));
        row.add("fill_bits",fillBits);row.addProperty("fill_calls",calls[0]);
        row.addProperty("min_bits",Long.toUnsignedString(Double.doubleToRawLongBits(beard.minValue())));row.addProperty("max_bits",Long.toUnsignedString(Double.doubleToRawLongBits(beard.maxValue())));
        output.println(row);
    }
    public static void main(String[] args)throws Exception {
        try(var output=new PrintStream(args[0])) {
            SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
            var kernelField=Beardifier.class.getDeclaredField("BEARD_KERNEL");kernelField.setAccessible(true);var kernel=(float[])kernelField.get(null);
            var kernelRow=new JsonObject();kernelRow.addProperty("kind","kernel");var bits=new JsonArray();for(float value:kernel)bits.add(Integer.toUnsignedLong(Float.floatToRawIntBits(value)));kernelRow.add("bits",bits);output.println(kernelRow);
            for(var adjustment:TerrainAdjustment.values())for(int delta:new int[]{-4,0,5}) {
                var r=new Beardifier.Rigid(new BoundingBox(1,64,2,8,76,10),adjustment,delta);
                capture(output,adjustment.getSerializedName()+"_"+delta,new Beardifier(List.of(r),List.of(),new BoundingBox(-30,30,-30,40,110,40)),0,0,new JsonObject());
            }
            capture(output,"empty",Beardifier.EMPTY,0,0,new JsonObject());
            var junctions=List.of(new JigsawJunction(0,64,0,123,StructureTemplatePool.Projection.RIGID),new JigsawJunction(-2,70,3,-10,StructureTemplatePool.Projection.TERRAIN_MATCHING));
            capture(output,"junctions",new Beardifier(List.of(),junctions,new BoundingBox(-30,30,-30,40,110,40)),0,0,new JsonObject());
            var extremes=List.of(new Beardifier.Rigid(new BoundingBox(Integer.MIN_VALUE,Integer.MIN_VALUE,Integer.MIN_VALUE,Integer.MAX_VALUE,Integer.MAX_VALUE,Integer.MAX_VALUE),TerrainAdjustment.BEARD_BOX,Integer.MAX_VALUE));
            capture(output,"extremes",new Beardifier(extremes,junctions,new BoundingBox(Integer.MIN_VALUE,Integer.MIN_VALUE,Integer.MIN_VALUE,Integer.MAX_VALUE,Integer.MAX_VALUE,Integer.MAX_VALUE)),0,0,new JsonObject());
            for(var chunk:List.of(new ChunkPos(0,0),new ChunkPos(-2,1),new ChunkPos(Integer.MIN_VALUE,Integer.MAX_VALUE),new ChunkPos(Integer.MAX_VALUE,Integer.MIN_VALUE)))for(boolean reverse:new boolean[]{false,true}) {
                int x=chunk.getMinBlockX(),z=chunk.getMinBlockZ();
                var nearby=List.of(new JigsawJunction(x-12,64,z,0,StructureTemplatePool.Projection.RIGID),new JigsawJunction(x-11,60,z,0,StructureTemplatePool.Projection.RIGID),new JigsawJunction(x+26,73,z,0,StructureTemplatePool.Projection.RIGID),new JigsawJunction(x+27,64,z,0,StructureTemplatePool.Projection.RIGID));
                var starts=new ArrayList<StructureStart>();
                starts.add(start(TerrainAdjustment.BEARD_THIN,List.of(ordinary(new BoundingBox(x+1,64,z+2,x+8,76,z+10)),pool(new BoundingBox(x-12,62,z-4,x-8,70,z+4),StructureTemplatePool.Projection.RIGID,-3,nearby))));
                starts.add(start(TerrainAdjustment.ENCAPSULATE,List.of(pool(new BoundingBox(x+27,70,z-1,x+28,75,z+1),StructureTemplatePool.Projection.TERRAIN_MATCHING,5,nearby),pool(new BoundingBox(x+28,70,z,x+30,75,z+1),StructureTemplatePool.Projection.RIGID,2,nearby))));
                starts.add(start(TerrainAdjustment.NONE,List.of(ordinary(new BoundingBox(x,0,z,x+10,255,z+10)))));
                if(reverse)Collections.reverse(starts);
                var input=new JsonObject();input.add("chunk",JSON.toJsonTree(new int[]{chunk.x(),chunk.z()}));var inputStarts=new JsonArray();starts.forEach(s->inputStarts.add(startInput(s)));input.add("starts",inputStarts);
                capture(output,"collector_"+chunk.x()+"_"+chunk.z()+"_"+reverse,Beardifier.forStructuresInChunk(new Manager(starts),chunk),x,z,input);
            }
        }
    }
}
