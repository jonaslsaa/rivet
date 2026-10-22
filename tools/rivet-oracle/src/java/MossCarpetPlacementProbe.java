import com.google.gson.*;
import java.lang.reflect.Proxy;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.WallSide;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;

/** Real MossyCarpetBlock.placeAt; the proxy supplies storage only. */
public final class MossCarpetPlacementProbe {
    public static void main(String[] args) {
        var output = System.out; var errors = System.err;
        try {
            SharedConstants.tryDetectVersion(); Bootstrap.bootStrap();
            var air = Blocks.AIR.defaultBlockState();
            var pos = new BlockPos(3,70,-5);
            for (int mask=0;mask<16;mask++) for (long seed : new long[]{0,1,42,-1})
                for (String condition : List.of("air","stone","base","empty_top","low_top","tall_top","grass","base_only","top_only")) {
                    var states = new HashMap<BlockPos,BlockState>();
                    states.put(pos.below(),Blocks.DIRT.defaultBlockState());
                    int bit=0;
                    for (Direction direction : Direction.Plane.HORIZONTAL) {
                        if ((mask & (1<<bit)) != 0) {
                            if (!condition.equals("top_only")) states.put(pos.relative(direction),Blocks.STONE.defaultBlockState());
                            if (!condition.equals("base_only")) states.put(pos.above().relative(direction),Blocks.STONE.defaultBlockState());
                        }
                        bit++;
                    }
                    var top = switch(condition) {
                        case "stone" -> Blocks.STONE.defaultBlockState();
                        case "base" -> Blocks.PALE_MOSS_CARPET.defaultBlockState();
                        case "empty_top","low_top","tall_top" -> {
                            var state=Blocks.PALE_MOSS_CARPET.defaultBlockState().setValue(MossyCarpetBlock.BASE,false);
                            var side=condition.equals("low_top") ? WallSide.LOW : condition.equals("tall_top") ? WallSide.TALL : WallSide.NONE;
                            for (var property : MossyCarpetBlock.PROPERTY_BY_DIRECTION.values()) state=state.setValue(property,side);
                            yield state;
                        }
                        case "grass" -> Blocks.SHORT_GRASS.defaultBlockState();
                        default -> air;
                    };
                    states.put(pos.above(),top);
                    var initial=new JsonArray();
                    states.entrySet().stream().sorted(Comparator.comparingInt((Map.Entry<BlockPos,BlockState> e)->e.getKey().getY())
                        .thenComparingInt(e->e.getKey().getZ()).thenComparingInt(e->e.getKey().getX()))
                        .forEach(e->initial.add(cell(e.getKey(),e.getValue())));
                    var writes=new JsonArray();
                    WorldGenLevel level=(WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},
                        (proxy,method,values)->switch(method.getName()) {
                            case "getBlockState" -> states.getOrDefault((BlockPos)values[0],air);
                            case "setBlock" -> {
                                var p=((BlockPos)values[0]).immutable();var state=(BlockState)values[1];
                                var write=cell(p,state);write.add((int)values[2]);writes.add(write);
                                var previous=states.put(p,state);yield !state.equals(previous);
                            }
                            default -> throw new UnsupportedOperationException(method.toString());
                        });
                    var random=new XoroshiroRandomSource(seed);
                    MossyCarpetBlock.placeAt(level,pos,random,2);
                    var row=new JsonObject();row.addProperty("mask",mask);row.addProperty("seed",seed);row.addProperty("condition",condition);
                    row.add("initial",initial);row.add("writes",writes);row.addProperty("next_long",random.nextLong());
                    output.println("MOSS_CARPET\t"+row);
                }
        } catch(Throwable error) {error.printStackTrace(errors);System.exit(1);}
    }
    private static JsonArray cell(BlockPos pos,BlockState state) {
        var cell=new JsonArray();cell.add(pos.getX());cell.add(pos.getY());cell.add(pos.getZ());cell.add(Block.getId(state));return cell;
    }
}
