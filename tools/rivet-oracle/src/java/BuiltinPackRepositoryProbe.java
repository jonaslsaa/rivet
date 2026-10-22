import com.google.gson.*;
import com.mojang.serialization.Dynamic;
import java.io.*;
import java.util.*;
import net.minecraft.nbt.*;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.*;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.storage.LevelStorageSource;

/** Actual pinned builtin discovery, resource inventory and pack finalization. */
public final class BuiltinPackRepositoryProbe {
 static JsonArray flags(net.minecraft.world.flag.FeatureFlagSet set){return new Gson().toJsonTree(FeatureFlags.REGISTRY.toNames(set).stream().map(Object::toString).sorted().toList()).getAsJsonArray();}
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  try(var output=new PrintStream(args[0])){
   var catalogRepo=ServerPacksSource.createVanillaTrustedRepository();catalogRepo.reload(true);var catalog=new JsonObject();catalog.addProperty("kind","catalog");var packs=new JsonArray();
   for(var pack:catalogRepo.getAvailablePacks()){
    var row=new JsonObject();row.addProperty("id",pack.getId());row.addProperty("required",pack.isRequired());row.addProperty("fixed",pack.isFixedPosition());row.addProperty("position",pack.getDefaultPosition().name());row.addProperty("auto",pack.getPackSource().shouldAddAutomatically());row.addProperty("feature_source",pack.getPackSource()==PackSource.FEATURE);row.add("features",flags(pack.getRequestedFeatures()));
    if(!pack.getId().equals("vanilla"))try(var jar=new java.util.jar.JarFile(new File(ServerPacksSource.class.getProtectionDomain().getCodeSource().getLocation().toURI()))){
     // listResources rejects an empty path prefix. Inspect all actual archive
     // payloads instead of accidentally reporting an empty resource inventory.
     var prefix="data/minecraft/datapacks/"+pack.getId()+"/data/";var paths=new TreeSet<String>();
     jar.stream().filter(entry->!entry.isDirectory()&&entry.getName().startsWith(prefix)).forEach(entry->{var relative=entry.getName().substring(prefix.length());var slash=relative.indexOf('/');paths.add(relative.substring(0,slash)+":"+relative.substring(slash+1));});row.add("resources",new Gson().toJsonTree(paths));
     var synchronizedRoots=new HashSet<String>();for(var registry:net.minecraft.resources.RegistryDataLoader.SYNCHRONIZED_REGISTRIES)synchronizedRoots.add(registry.key().identifier().getPath());
     var payloads=new JsonObject();for(var id:paths){var relative=id.substring(id.indexOf(':')+1);var slash=relative.indexOf('/');if(slash>0&&synchronizedRoots.contains(relative.substring(0,slash)))try(var reader=new InputStreamReader(jar.getInputStream(jar.getJarEntry(prefix+id.replace(':','/'))),java.nio.charset.StandardCharsets.UTF_8)){payloads.add(id,JsonParser.parseReader(reader));}}
     row.add("synchronized_payloads",payloads);
    }
    packs.add(row);
   }
   catalog.add("packs",packs);output.println(catalog);
   String[][] inputs={{"default","{}"},{"empty_features","{enabled_features:[]}"},{"selected_minecart","{DataPacks:{Enabled:[\"vanilla\",\"minecart_improvements\"],Disabled:[]}}"},{"forced_empty_overlay_features","{enabled_features:[\"vanilla\",\"minecart_improvements\",\"redstone_experiments\"]}"},{"forced_trade","{enabled_features:[\"vanilla\",\"trade_rebalance\"]}"},{"required_paper_disabled","{DataPacks:{Enabled:[\"vanilla\"],Disabled:[\"paper\"]}}"},{"empty_selected","{DataPacks:{Enabled:[],Disabled:[\"vanilla\"]}}"}};
   for(var test:inputs)for(boolean init:new boolean[]{false,true})for(boolean safe:new boolean[]{false,true}){
    var input=TagParser.parseCompoundFully(test[1]);var initial=LevelStorageSource.readDataConfig(new Dynamic<>(NbtOps.INSTANCE,input));var repo=ServerPacksSource.createVanillaTrustedRepository();var result=MinecraftServer.configurePackRepository(repo,initial,init,safe);
    var row=new JsonObject();row.addProperty("kind","selection");row.addProperty("case",test[0]);row.addProperty("input",input.toString());row.addProperty("init_mode",init);row.addProperty("safe_mode",safe);row.add("features",flags(result.enabledFeatures()));row.add("enabled",new Gson().toJsonTree(result.dataPacks().getEnabled()));row.add("disabled",new Gson().toJsonTree(result.dataPacks().getDisabled()));output.println(row);
   }
  }
 }
}
