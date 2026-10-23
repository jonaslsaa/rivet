import com.google.gson.*;
import java.io.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.resources.Identifier;
import io.papermc.paper.configuration.*;
import io.papermc.paper.configuration.type.number.IntOr;
import org.spongepowered.configurate.*;
import org.bukkit.configuration.file.YamlConfiguration;
import org.spigotmc.*;

public final class WorldRuntimeSettingsProbe {
 static ConfigurationNode node(String yaml)throws Exception {
  return ConfigurationLoaders.naturallySorted()
   .defaultOptions(o->o.serializers(s->s.register(IntOr.Default.class,IntOr.Default.SERIALIZER)))
   .source(()->new BufferedReader(new StringReader(yaml))).build().load();
 }
 static int integer(ConfigurationNode n,int fallback,String... path)throws Exception{return n.node((Object[])path).get(Integer.class,fallback);}
 static boolean bool(ConfigurationNode n,boolean fallback,String... path)throws Exception{return n.node((Object[])path).get(Boolean.class,fallback);}
 public static void main(String[] args)throws Exception {
  SharedConstants.tryDetectVersion();Bootstrap.bootStrap();
  var ctor=WorldConfiguration.class.getDeclaredConstructor(SpigotWorldConfig.class,Identifier.class);ctor.setAccessible(true);
  var spigotContext=(SpigotWorldConfig)ChestMinecartProbe.UNSAFE.allocateInstance(SpigotWorldConfig.class);
  var paper=ctor.newInstance(spigotContext,Identifier.parse("minecraft:overworld"));
  paper.environment=paper.new Environment();
  paper.anticheat=paper.new Anticheat();paper.anticheat.antiXray=paper.anticheat.new AntiXray();
  paper.entities=paper.new Entities();paper.entities.spawning=paper.entities.new Spawning();
  paper.entities.spawning.duplicateUuid=paper.entities.spawning.new DuplicateUUID();
  paper.entities.behavior=paper.entities.new Behavior();paper.tickRates=paper.new TickRates();
  paper.unsupportedSettings=paper.new UnsupportedSettings();paper.unsupportedSettings.ticking=paper.unsupportedSettings.new Ticking();
  String[] defaults={"", "tick-rates: {mob-spawner: 4}\nentities: {spawning: {duplicate-uuid: {mode: warn, safe-regen-delete-range: 17}}}\n", "entities: {behavior: {cooldown-failed-beehive-releases: false}}\nanticheat: {anti-xray: {enabled: true}}\n"};
  String[] worlds={"", "tick-rates: {mob-spawner: -1}\nentities: {spawning: {duplicate-uuid: {mode: delete}}}\n", "tick-rates: {mob-spawner: '7'}\nunsupported-settings: {ticking: {block-entities: 'false'}}\n", "entities: {spawning: {duplicate-uuid: {mode: safe-regen}}}\n", "tick-rates: {mob-spawner: 2.5}\n", "tick-rates: {mob-spawner: null}\n", "entities: {tracking-range-y: {enabled: true, animal: 12, other: default}}\n"};
  try(var out=new PrintWriter(args[0])) {
   var access=net.minecraft.server.RegistryLayer.createRegistryAccess().getLayer(net.minecraft.server.RegistryLayer.STATIC);
   var entityKeys=new io.papermc.paper.configuration.serializer.registry.RegistryValueSerializer<>(new io.leangen.geantyref.TypeToken<net.minecraft.world.entity.EntityType<?>>() {},access,net.minecraft.core.registries.Registries.ENTITY_TYPE,true);
   for(String key:List.of("cow","unknown:missing","invalid?")){
    var row=new JsonObject();row.addProperty("entity_key",key);
    try { row.addProperty("canonical",net.minecraft.core.registries.BuiltInRegistries.ENTITY_TYPE.getKey(entityKeys.deserialize((java.lang.reflect.AnnotatedType)null,key)).toString()); }
    catch(Exception e){row.addProperty("error",e.getClass().getName());}
    out.println(row);
   }
   var worldCases=new ArrayList<>(Arrays.asList(worlds));
   worldCases.add("entities: {tracking-range-y: {enabled: true, animal: 12.75, other: DEFAULT}}\n");
   worldCases.add("entities: {tracking-range-y: {enabled: true, animal: '12.75'}}\n");
   worldCases.add("environment: {max-block-ticks: 17, max-fluid-ticks: 23, water-over-lava-flow-speed: 9}\n");
   worldCases.add("environment: {max-block-ticks: -1, max-fluid-ticks: 12.75, water-over-lava-flow-speed: '7'}\n");
   worldCases.add("environment: {max-block-ticks: null, max-fluid-ticks: 'bad', water-over-lava-flow-speed: 3}\n");
   worldCases.add("anticheat: {anti-xray: {enabled: true}}\n");
   worldCases.add("anticheat: {anti-xray: {enabled: false}}\n");
   worldCases.add("anticheat: {anti-xray: {enabled: 'false'}}\n");
   worldCases.add("anticheat: {anti-xray: {enabled: invalid}}\n");
   for(String d:defaults)for(String w:worldCases){
    var row=new JsonObject();row.addProperty("defaults",d);row.addProperty("world",w);
    try {
     var n=node(w);n.mergeFrom(node(d));var result=new JsonObject();
     result.addProperty("spawner_rate",integer(n,paper.tickRates.mobSpawner,"tick-rates","mob-spawner"));
     result.addProperty("max_block_ticks",integer(n,paper.environment.maxBlockTicks,"environment","max-block-ticks"));
     result.addProperty("max_fluid_ticks",integer(n,paper.environment.maxFluidTicks,"environment","max-fluid-ticks"));
     result.addProperty("water_over_lava_flow_speed",integer(n,paper.environment.waterOverLavaFlowSpeed,"environment","water-over-lava-flow-speed"));
     result.addProperty("block_entities",bool(n,paper.unsupportedSettings.ticking.blockEntities,"unsupported-settings","ticking","block-entities"));
     result.addProperty("anti_xray_enabled",bool(n,paper.anticheat.antiXray.enabled,"anticheat","anti-xray","enabled"));
     result.addProperty("bee_cooldown",bool(n,paper.entities.behavior.cooldownFailedBeehiveReleases,"entities","behavior","cooldown-failed-beehive-releases"));
     result.addProperty("duplicate_mode",n.node("entities","spawning","duplicate-uuid","mode").get(WorldConfiguration.Entities.Spawning.DuplicateUUID.DuplicateUUIDMode.class,paper.entities.spawning.duplicateUuid.mode).name());
     result.addProperty("duplicate_range",integer(n,paper.entities.spawning.duplicateUuid.safeRegenDeleteRange,"entities","spawning","duplicate-uuid","safe-regen-delete-range"));
     boolean y=bool(n,false,"entities","tracking-range-y","enabled");
     result.addProperty("animal_y",y?n.node("entities","tracking-range-y","animal").get(IntOr.Default.class,IntOr.Default.USE_DEFAULT).or(-1):-1);
     result.addProperty("other_y",y?n.node("entities","tracking-range-y","other").get(IntOr.Default.class,IntOr.Default.USE_DEFAULT).or(-1):-1);
     row.add("result",result);
    }catch(Exception e){row.addProperty("error",e.getClass().getName());}
    out.println(row);
   }
   for(String yaml:new String[]{"", "world-settings: {default: {entity-tracking-range: {animals: 32}, nerf-spawner-mobs: true}, 'minecraft:overworld': {entity-tracking-range: {other: 15}}}", "world-settings: {default: {entity-tracking-range: {animals: '32', other: 12.75}}, 'minecraft:overworld': {nerf-spawner-mobs: 'true'}}"}) {
    var config=new YamlConfiguration();config.loadFromString(yaml);
    var row=new JsonObject();row.addProperty("spigot",yaml);
    // Invoke real Spigot getters with actual typed default fields without
    // initializing unrelated world settings or fabricating a Bukkit server.
    var s=(SpigotWorldConfig)ChestMinecartProbe.UNSAFE.allocateInstance(SpigotWorldConfig.class);
    for(var entry:Map.of("config",(Object)config,"worldName",(Object)"minecraft:overworld").entrySet()){var f=SpigotWorldConfig.class.getDeclaredField(entry.getKey());f.setAccessible(true);f.set(s,entry.getValue());}
    row.addProperty("animal",s.getInt("entity-tracking-range.animals",96));row.addProperty("other",s.getInt("entity-tracking-range.other",64));row.addProperty("nerf",s.getBoolean("nerf-spawner-mobs",false));out.println(row);
   }
  }
 }
}
