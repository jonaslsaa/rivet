import com.google.gson.*;
import com.mojang.serialization.Dynamic;
import java.io.*;
import java.util.*;
import net.minecraft.nbt.*;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.level.storage.LevelStorageSource;

/** Actual LevelStorageSource.readDataConfig, not an imitation field parser.
 * This probes initial saved settings only, not pack repository availability. */
public final class GeneratedBootSettingsProbe {
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();
  String[][] cases={
   {"absent","{}"},
   {"explicit_empty","{enabled_features:[]}"},
   {"experimental_without_vanilla","{enabled_features:[\"minecraft:minecart_improvements\"]}"},
   {"unqualified_names","{enabled_features:[\"vanilla\",\"minecart_improvements\"]}"},
   {"unknown_flag","{enabled_features:[\"minecraft:not_real\"]}"},
   {"partial_known_and_unknown","{enabled_features:[\"minecraft:minecart_improvements\",\"minecraft:not_real\"]}"},
   {"wrong_feature_container","{enabled_features:\"minecraft:minecart_improvements\"}"},
   {"wrong_feature_element","{enabled_features:[1]}"},
   {"custom_pack_retained_not_resolved","{DataPacks:{Enabled:[\"vanilla\",\"file/custom.zip\"],Disabled:[\"paper\"]},enabled_features:[]}"},
   {"empty_enabled_packs","{DataPacks:{Enabled:[],Disabled:[\"vanilla\"]}}"},
   {"missing_required_pack_field","{DataPacks:{Enabled:[\"file/custom.zip\"]}}"},
   {"wrong_pack_field_type","{DataPacks:{Enabled:[1],Disabled:[]}}"},
   {"pack_order_duplicates","{DataPacks:{Enabled:[\"file/z.zip\",\"vanilla\",\"file/z.zip\"],Disabled:[\"paper\",\"file/a.zip\"]},enabled_features:[\"trade_rebalance\",\"vanilla\"]}"},
   {"wrong_pack_container","{DataPacks:7,enabled_features:[\"redstone_experiments\"]}"}
  };
  try(var output=new PrintStream(args[0])){
   for(var test:cases){
    var input=TagParser.parseCompoundFully(test[1]);
    var settings=LevelStorageSource.readDataConfig(new Dynamic<>(NbtOps.INSTANCE,input));
    var row=new JsonObject();row.addProperty("case",test[0]);row.addProperty("input",input.toString());
    var flags=FeatureFlags.REGISTRY.toNames(settings.enabledFeatures()).stream().map(Object::toString).sorted().toList();
    row.add("enabled_features",new Gson().toJsonTree(flags));row.add("enabled_packs",new Gson().toJsonTree(settings.dataPacks().getEnabled()));row.add("disabled_packs",new Gson().toJsonTree(settings.dataPacks().getDisabled()));
    output.println(row);
   }
  }
 }
}
