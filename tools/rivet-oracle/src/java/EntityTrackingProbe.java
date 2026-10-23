import com.google.gson.*;
import java.io.*;
import java.util.*;
import io.netty.buffer.Unpooled;
import net.minecraft.core.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.*;
import net.minecraft.server.level.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.level.*;
import net.minecraft.network.*;
import net.minecraft.network.protocol.*;
import net.minecraft.network.protocol.game.*;

/** Actual ServerEntity pairing/change algorithms. The level supplies genuine
 * registries and getter-only context; no observer selection/plugins are mocked
 * as successful. A detached player supplies only the identity used by pairing. */
public final class EntityTrackingProbe {
 static final Gson G=new Gson();
 @SuppressWarnings({"unchecked","rawtypes"})
 static JsonObject packet(Packet<?> packet){
  var row=new JsonObject();row.addProperty("type",packet.type().id().toString());
  var buffer=new RegistryFriendlyByteBuf(Unpooled.buffer(),GenerationSheepProbe.access);
  var codec=packet instanceof ClientboundAddEntityPacket?ClientboundAddEntityPacket.STREAM_CODEC:
   packet instanceof ClientboundSetEntityDataPacket?ClientboundSetEntityDataPacket.STREAM_CODEC:
   packet instanceof ClientboundUpdateAttributesPacket?ClientboundUpdateAttributesPacket.STREAM_CODEC:
   packet instanceof ClientboundEntityPositionSyncPacket?ClientboundEntityPositionSyncPacket.STREAM_CODEC:
   packet instanceof ClientboundMoveEntityPacket.Pos?ClientboundMoveEntityPacket.Pos.STREAM_CODEC:
   packet instanceof ClientboundMoveEntityPacket.PosRot?ClientboundMoveEntityPacket.PosRot.STREAM_CODEC:
   packet instanceof ClientboundMoveEntityPacket.Rot?ClientboundMoveEntityPacket.Rot.STREAM_CODEC:
   packet instanceof ClientboundRotateHeadPacket?ClientboundRotateHeadPacket.STREAM_CODEC:
   packet instanceof ClientboundSetEntityMotionPacket?ClientboundSetEntityMotionPacket.STREAM_CODEC:null;
  if(codec==null)throw new AssertionError(packet.getClass());
  ((net.minecraft.network.codec.StreamCodec)codec).encode(buffer,packet);
  byte[] bytes=new byte[buffer.readableBytes()];buffer.readBytes(bytes);row.addProperty("body",HexFormat.of().formatHex(bytes));buffer.release();return row;
 }
 static void run(PrintStream out,String family,String nbt)throws Exception{
  var level=GenerationSheepProbe.level();Entity.SHARED_RANDOM=new ca.spottedleaf.moonrise.common.util.ThreadUnsafeRandom(42);
  EntityType<?> type=switch(family){case "sheep"->EntityTypes.SHEEP;case "cow"->EntityTypes.COW;case "pig"->EntityTypes.PIG;case "chicken"->EntityTypes.CHICKEN;default->EntityTypes.CHEST_MINECART;};
  var entity=type.create(level,EntitySpawnReason.LOAD);
  if(!nbt.isEmpty())entity.load(net.minecraft.world.level.storage.TagValueInput.create(net.minecraft.util.ProblemReporter.DISCARDING,GenerationSheepProbe.access,net.minecraft.nbt.TagParser.parseCompoundFully(nbt)));
  entity.setId(101);entity.setUUID(new UUID(1,2));entity.snapTo(8.5,64,8.5,123,12);
  var player=(ServerPlayer)ChestMinecartProbe.UNSAFE.allocateInstance(ServerPlayer.class);player.setId(999);
  var changes=new JsonArray();
  var sync=(ServerEntity.Synchronizer)java.lang.reflect.Proxy.newProxyInstance(ServerEntity.class.getClassLoader(),new Class[]{ServerEntity.Synchronizer.class},(p,m,args)->{changes.add(packet((Packet<?>)args[0]));return null;});
  var tracker=new ServerEntity(level,entity,3,true,sync,new HashSet<>());
  var pairing=new JsonArray();tracker.sendPairingData(player,p->pairing.add(packet(p)));
  // Actual synchronized data in accessor order, including defaults, records
  // both schema and constructor/default identity for native projection tests.
  var all=new JsonArray();for(var value:entity.getEntityData().packAll()){var d=new JsonObject();d.addProperty("id",value.id());d.addProperty("serializer",net.minecraft.network.syncher.EntityDataSerializers.getSerializedId(value.serializer()));d.addProperty("value",value.value() instanceof Holder<?> holder?holder.unwrapKey().orElseThrow().identifier().toString():String.valueOf(value.value()));all.add(d);}
  tracker.sendChanges();entity.snapTo(9,64,8.5,124,12);for(int i=0;i<3;i++)tracker.sendChanges();tracker.onPlayerAdd();tracker.sendChanges();
  var row=new JsonObject();row.addProperty("family",family);row.addProperty("input",nbt);row.add("all",all);row.add("pairing",pairing);row.add("changes",changes);
  if(family.equals("sheep")&&nbt.isEmpty()){
   var vectors=new JsonArray();for(double[] xyz:new double[][]{{0,0,0},{0.000005,-0.000005,0},{3.051944088384301E-5,0,0},{1,-1,0.5},{4.1,-12.2,0},{1.7179869183E10,-1.8E10,0}}){var vector=new net.minecraft.world.phys.Vec3(xyz[0],xyz[1],xyz[2]);var b=Unpooled.buffer();LpVec3.write(b,vector);byte[] bytes=new byte[b.readableBytes()];b.getBytes(0,bytes);var decoded=LpVec3.read(b);var v=new JsonObject();v.add("input",G.toJsonTree(xyz));v.addProperty("body",HexFormat.of().formatHex(bytes));v.add("decoded",G.toJsonTree(new double[]{decoded.x,decoded.y,decoded.z}));vectors.add(v);b.release();}row.add("lp_vectors",vectors);
   var invalid=new JsonArray();for(int typeId:new int[]{-1,9999}){var b=new RegistryFriendlyByteBuf(Unpooled.buffer(),GenerationSheepProbe.access);b.writeVarInt(101);b.writeUUID(new UUID(1,2));b.writeVarInt(typeId);b.writeDouble(0);b.writeDouble(0);b.writeDouble(0);b.writeByte(0);b.writeByte(0);b.writeByte(0);b.writeByte(0);b.writeVarInt(0);var decoded=ClientboundAddEntityPacket.STREAM_CODEC.decode(b);var v=new JsonObject();v.addProperty("wire_id",typeId);v.addProperty("resolved_id",BuiltInRegistries.ENTITY_TYPE.getId(decoded.getType()));invalid.add(v);b.release();}row.add("unknown_types",invalid);
  }out.println(row);
 }
 public static void main(String[] args)throws Exception{
  net.minecraft.SharedConstants.tryDetectVersion();net.minecraft.server.Bootstrap.bootStrap();var packs=net.minecraft.server.packs.repository.ServerPacksSource.createVanillaTrustedRepository();net.minecraft.server.MinecraftServer.configurePackRepository(packs,WorldDataConfiguration.DEFAULT,true,false);
  // Getter-only sanitizer context: these supported packets contain no items.
  // The source serializer still executes its real session scope and codecs.
  var config=new io.papermc.paper.configuration.GlobalConfiguration();config.anticheat=config.new Anticheat();config.anticheat.obfuscation=config.anticheat.new Obfuscation();config.anticheat.obfuscation.items=(io.papermc.paper.configuration.GlobalConfiguration.Anticheat.Obfuscation.Items)ChestMinecartProbe.UNSAFE.allocateInstance(io.papermc.paper.configuration.GlobalConfiguration.Anticheat.Obfuscation.Items.class);var binding=(io.papermc.paper.util.sanitizer.ItemObfuscationBinding)ChestMinecartProbe.UNSAFE.allocateInstance(io.papermc.paper.util.sanitizer.ItemObfuscationBinding.class);var bindingLevel=binding.getClass().getDeclaredField("level");bindingLevel.setAccessible(true);bindingLevel.set(binding,io.papermc.paper.util.sanitizer.ItemObfuscationSession.ObfuscationLevel.OVERSIZED);config.anticheat.obfuscation.items.binding=binding;var global=io.papermc.paper.configuration.GlobalConfiguration.class.getDeclaredField("instance");global.setAccessible(true);global.set(null,config);
  try(var resources=new net.minecraft.server.packs.resources.MultiPackResourceManager(net.minecraft.server.packs.PackType.SERVER_DATA,packs.openAllSelected());var out=new PrintStream(args[0])){
   var layers=net.minecraft.server.RegistryLayer.createRegistryAccess();var lookups=net.minecraft.tags.TagLoader.buildUpdatedLookups(layers.getAccessForLoading(net.minecraft.server.RegistryLayer.WORLDGEN),net.minecraft.tags.TagLoader.loadTagsForExistingRegistries(resources,layers.getLayer(net.minecraft.server.RegistryLayer.STATIC)));var worldgen=RegistryDataLoader.load(resources,lookups,RegistryDataLoader.WORLDGEN_REGISTRIES,Runnable::run).join();GenerationSheepProbe.access=layers.replaceFrom(net.minecraft.server.RegistryLayer.WORLDGEN,worldgen).compositeAccess().freeze();
   for(String family:List.of("sheep","cow","pig","chicken","chest_minecart")){run(out,family,"");run(out,family,"{Age:-200,Health:2f,Silent:1b,NoGravity:1b,Invisible:1b,Glowing:1b,Air:12s,TicksFrozen:17,Color:14b,Sheared:1b,variant:'minecraft:cold',sound_variant:'minecraft:classic'}");}
  }
 }
}
