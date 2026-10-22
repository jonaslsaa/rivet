import java.lang.reflect.Field;
import java.lang.reflect.Proxy;
import java.util.*;
import java.util.stream.Collectors;
import net.minecraft.core.BlockPos;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.feature.*;
import net.minecraft.world.level.levelgen.feature.configurations.*;
import net.minecraft.world.level.levelgen.feature.featuresize.TwoLayersFeatureSize;
import net.minecraft.world.level.levelgen.feature.foliageplacers.*;
import net.minecraft.world.level.levelgen.feature.trunkplacers.StraightTrunkPlacer;
import net.minecraft.world.level.levelgen.feature.stateproviders.BlockStateProvider;
import net.minecraft.util.RandomSource;
import net.minecraft.util.valueproviders.ConstantInt;

/** JDK25 HashSet actual iteration, collision treeification, splits/removal. */
public final class BlockPosSetProbe {
    private static final java.io.PrintStream OUT = System.out;
    private static Field mapField, tableField;
    private static String positions(Collection<BlockPos> positions) {
        if (positions.isEmpty()) return "-";
        return positions.stream().map(p -> p.getX()+","+p.getY()+","+p.getZ()).collect(Collectors.joining(";"));
    }
    private static List<BlockPos> line(int start, int end, int stride) {
        var positions = new ArrayList<BlockPos>();
        for (int i=start; i<end; i++) positions.add(new BlockPos(i*stride,0,0));
        return positions;
    }
    private static void snapshot(String name, String operation, String input, int changed, HashSet<BlockPos> set) throws Exception {
        Object[] table = (Object[]) tableField.get(mapField.get(set));
        int trees = 0;
        if (table != null) for (Object node : table) if (node != null && node.getClass().getSimpleName().equals("TreeNode")) trees++;
        OUT.println("BLOCK_POS_SET\t"+name+"\t"+operation+"\t"+input+"\t"+changed+"\t"+(table==null?0:table.length)+"\t"+trees+"\t"+positions(set));
    }
    private static void batch(String name, String op, List<BlockPos> input, HashSet<BlockPos> set) throws Exception {
        int changed=0;
        for (BlockPos p : input) if (op.equals("add") ? set.add(p) : set.remove(p)) changed++;
        snapshot(name,op,positions(input),changed,set);
    }
    private static void pop(String name, int count, HashSet<BlockPos> set) throws Exception {
        for (int i=0;i<count;i++) { var it=set.iterator(); it.next(); it.remove(); }
        snapshot(name,"pop",Integer.toString(count),count,set);
    }
    public static void main(String[] args) throws Exception {
        mapField=HashSet.class.getDeclaredField("map"); mapField.setAccessible(true);
        tableField=HashMap.class.getDeclaredField("table"); tableField.setAccessible(true);
        var comparable=HashMap.class.getDeclaredMethod("comparableClassFor",Object.class);
        comparable.setAccessible(true);
        if(comparable.invoke(null,new BlockPos(0,0,0))!=null) throw new AssertionError("revisit equal-hash BlockPos tree-bin ordering");
        var normal=new HashSet<BlockPos>();
        var mixed=new ArrayList<BlockPos>();
        for(int i=0;i<30;i++) mixed.add(new BlockPos((i*13)%17-8,i%5-2,(i*7)%19-9));
        batch("normal","add",mixed,normal);
        batch("normal","add",mixed.subList(0,5),normal);
        batch("normal","remove",mixed.subList(3,15),normal);
        batch("normal","add",mixed.subList(8,12),normal);
        var tree=new HashSet<BlockPos>();
        batch("tree","add",line(0,11,64),tree);
        batch("tree","add",line(11,20,64),tree);
        batch("tree","remove",List.of(new BlockPos(192,0,0),new BlockPos(448,0,0),new BlockPos(320,0,0)),tree);
        pop("tree",5,tree);
        batch("tree","add",line(20,23,64),tree);
        batch("tree","remove",line(0,23,64),tree);
        var split=new HashSet<BlockPos>();
        batch("split","add",line(0,20,64),split);
        batch("split","add",line(1,41,1),split);
        batch("split","add",line(41,91,1),split);
        pop("split",9,split);
        var signed=new HashSet<BlockPos>();
        var signedPositions=new ArrayList<BlockPos>();
        for(int i=-10;i<=10;i++) {int h=i*64;signedPositions.add(new BlockPos(h^(h>>>16),0,0));}
        batch("signed","add",signedPositions,signed);
        batch("signed","remove",List.of(signedPositions.get(10),signedPositions.get(9),signedPositions.get(11),signedPositions.get(2),signedPositions.get(18)),signed);
        pop("signed",7,signed);
        var iteratorTree=new HashSet<BlockPos>();
        batch("iterator_tree","add",line(0,11,64),iteratorTree);
        pop("iterator_tree",10,iteratorTree);
        batch("iterator_tree","add",line(11,14,64),iteratorTree);
        batch("iterator_tree","remove",line(0,14,64),iteratorTree);
        var equal=new HashSet<BlockPos>();
        var colliders=new ArrayList<BlockPos>();
        for(int i=0;i<10;i++) colliders.add(new BlockPos(-31*i,i,0));
        batch("equal","add",colliders,equal);
        var trigger=new BlockPos(-310,10,0);
        if(!equal.add(trigger)) throw new AssertionError();
        Object[] table=(Object[])tableField.get(mapField.get(equal));
        if(!table[0].getClass().getSimpleName().equals("TreeNode")) throw new AssertionError("expected actual tree bin");
        // Do not freeze JVM identity-dependent order into a deterministic fixture.
        OUT.println("BLOCK_POS_SET\tequal\tidentity\t-310,10,0\t0\t64\t1\t-");
        naturalTrees();
    }

    @SuppressWarnings("unchecked")
    private static void naturalTrees() throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();
        net.minecraft.server.MinecraftServer.configurePackRepository(packs,
            new net.minecraft.world.level.WorldDataConfiguration(
                new net.minecraft.world.level.DataPackConfig(List.of("minecraft","vanilla"),List.of()),
                net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS),true,false);
        try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected())) {
            var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();
            net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)).forEach(p->p.apply());
            var states=new HashMap<BlockPos,BlockState>();
            var writes=new ArrayList<BlockPos>();
            java.util.function.Function<BlockPos,BlockState> lookup=p->states.getOrDefault(p,p.getY()<64?Blocks.DIRT.defaultBlockState():Blocks.AIR.defaultBlockState());
            WorldGenLevel world=(WorldGenLevel)Proxy.newProxyInstance(WorldGenLevel.class.getClassLoader(),new Class<?>[]{WorldGenLevel.class},
                (proxy,method,a)->switch(method.getName()) {
                    case "getBlockState" -> lookup.apply((BlockPos)a[0]);
                    case "isStateAtPosition" -> ((java.util.function.Predicate<BlockState>)a[1]).test(lookup.apply((BlockPos)a[0]));
                    case "isFluidAtPosition" -> ((java.util.function.Predicate<net.minecraft.world.level.material.FluidState>)a[1]).test(lookup.apply((BlockPos)a[0]).getFluidState());
                    case "setBlock" -> {BlockPos p=((BlockPos)a[0]).immutable();states.put(p,(BlockState)a[1]);writes.add(p);yield true;}
                    case "getMinY" -> -64;
                    case "getHeight" -> 384;
                    case "getMaxY" -> 319;
                    default -> throw new UnsupportedOperationException(method.toString());
                });
            var config=new TreeConfiguration.TreeConfigurationBuilder(BlockStateProvider.simple(Blocks.OAK_LOG),new StraightTrunkPlacer(6,2,1),
                BlockStateProvider.simple(Blocks.OAK_LEAVES),new BlobFoliagePlacer(ConstantInt.of(2),ConstantInt.of(0),3),
                new TwoLayersFeatureSize(1,0,1),BlockStateProvider.simple(Blocks.DIRT)).build();
            var trunks=new ArrayList<BlockPos>();var foliage=new ArrayList<BlockPos>();var leafSet=new HashSet<BlockPos>();
            java.util.function.BiConsumer<BlockPos,BlockState> trunkSetter=(p,s)->{p=p.immutable();trunks.add(p);states.put(p,s);};
            java.util.function.BiConsumer<BlockPos,BlockState> rootSetter=(p,s)->{throw new AssertionError("rootless config");};
            var foliageSetter=new FoliagePlacer.FoliageSetter(){
                public void set(BlockPos p,BlockState s){p=p.immutable();foliage.add(p);leafSet.add(p);states.put(p,s);}
                public boolean isSet(BlockPos p){return leafSet.contains(p);}
            };
            var doPlace=TreeFeature.class.getDeclaredMethod("doPlace",WorldGenLevel.class,RandomSource.class,BlockPos.class,java.util.function.BiConsumer.class,java.util.function.BiConsumer.class,FoliagePlacer.FoliageSetter.class,TreeConfiguration.class);
            doPlace.setAccessible(true);
            if(!(boolean)doPlace.invoke(new TreeFeature(TreeConfiguration.CODEC),world,RandomSource.create(42),new BlockPos(0,64,0),rootSetter,trunkSetter,foliageSetter,config))throw new AssertionError("natural tree should place");
            batch("natural_trunks","add",trunks,new HashSet<>());
            batch("natural_foliage","add",foliage,new HashSet<>());
            states.clear();writes.clear();
            var fallenConfig=new FallenTreeConfiguration.FallenTreeConfigurationBuilder(BlockStateProvider.simple(Blocks.OAK_LOG),ConstantInt.of(9)).build();
            new FallenTreeFeature(FallenTreeConfiguration.CODEC).place(new FeaturePlaceContext<>(Optional.empty(),world,null,RandomSource.create(42),new BlockPos(0,64,0),fallenConfig));
            batch("natural_fallen_logs","add",writes.subList(1,writes.size()),new HashSet<>());
        }
    }
}
