import com.google.gson.*;
import java.io.PrintStream;
import java.lang.reflect.*;
import java.util.*;
import java.util.concurrent.atomic.*;
import java.util.logging.*;
import org.bukkit.*;
import org.bukkit.event.*;
import org.bukkit.event.world.AsyncStructureSpawnEvent;
import org.bukkit.plugin.*;
import com.destroystokyo.paper.event.server.ServerExceptionEvent;

/** Actual PaperEventManager, HandlerList and RegisteredListener execution.
 * Server/plugin proxies supply only lifecycle/thread/logger getters. World and
 * structure payloads are deliberately unused/null: registry payload authority
 * is tested by the native dispatcher, not fabricated in this dispatch probe. */
public final class StructureEventDispatchProbe {
    static final Thread PRIMARY=Thread.currentThread();
    static final List<String> TRACE=new ArrayList<>(),LOGS=new ArrayList<>();
    static Object manager;static Method call,register;
    static final Logger LOGGER=Logger.getLogger("structure-event-oracle");
    static Object proxy(Class<?> type,InvocationHandler handler) {return Proxy.newProxyInstance(type.getClassLoader(),new Class[]{type},handler);}
    static Object objectMethod(Object self,Method method,Object[] args) {
        return switch(method.getName()) {case "equals"->self==args[0];case "hashCode"->System.identityHashCode(self);case "toString"->"event-oracle-proxy";default->throw new AssertionError("unexpected getter "+method);};
    }
    static Plugin plugin(String name,AtomicBoolean enabled) {
        var naggable=new AtomicBoolean(true);
        var metadata=proxy(io.papermc.paper.plugin.configuration.PluginMeta.class,(self,method,args)->switch(method.getName()) {
            case "getDisplayName","getName"->name;case "getAuthors"->List.of("author");default->objectMethod(self,method,args);
        });
        return (Plugin)proxy(Plugin.class,(self,method,args)->switch(method.getName()) {
            case "getPluginMeta"->metadata;case "isEnabled"->enabled.get();case "isNaggable"->naggable.get();case "setNaggable"->{naggable.set((boolean)args[0]);yield null;}case "getLogger"->LOGGER;default->objectMethod(self,method,args);
        });
    }
    static void add(Class<? extends Event> type,Plugin plugin,EventPriority priority,boolean ignore,EventExecutor executor)throws Exception {
        register.invoke(manager,type,new Listener(){},priority,executor,plugin,ignore);
    }
    static void dispatch(Event event)throws Throwable {try{call.invoke(manager,event);}catch(InvocationTargetException wrapped){throw wrapped.getCause();}}
    static AsyncStructureSpawnEvent event() {return new AsyncStructureSpawnEvent(null,null,new org.bukkit.util.BoundingBox(-12,60,-20,3,80,-5),-1,-2);}
    static void worker(Event event)throws Exception {
        var failure=new AtomicReference<Throwable>();
        var thread=new Thread(()->{try{dispatch(event);}catch(Throwable error){failure.set(error);}},"actual-paper-event-oracle");thread.start();thread.join();
        if(failure.get()!=null)throw new AssertionError("worker failed",failure.get());
    }
    public static void main(String[] args)throws Exception {
        try(var output=new PrintStream(args[0])) {
            net.minecraft.SharedConstants.tryDetectVersion();
            LOGGER.setUseParentHandlers(false);LOGGER.addHandler(new Handler(){public void publish(LogRecord record){var m=record.getMessage();if(m.startsWith("Nag author"))LOGS.add("nag");else if(m.startsWith("Could not pass event"))LOGS.add(m.contains("ServerExceptionEvent")?"error:exception":"error:spawn");}public void flush(){}public void close(){}});
            var server=(Server)proxy(Server.class,(self,method,values)->switch(method.getName()) {
                case "isPrimaryThread"->Thread.currentThread()==PRIMARY;case "getLogger"->LOGGER;case "getName","getVersion","getBukkitVersion"->"event-oracle";default->objectMethod(self,method,values);
            });
            Bukkit.setServer(server);
            var type=Class.forName("io.papermc.paper.plugin.manager.PaperEventManager");var constructor=type.getDeclaredConstructor(Server.class);constructor.setAccessible(true);manager=constructor.newInstance(server);
            call=type.getDeclaredMethod("callEvent",Event.class);call.setAccessible(true);
            register=type.getDeclaredMethod("registerEvent",Class.class,Listener.class,EventPriority.class,EventExecutor.class,Plugin.class,boolean.class);register.setAccessible(true);
            HandlerList.unregisterAll();String guard;
            try{dispatch(event());throw new AssertionError("primary thread accepted empty asynchronous event");}catch(IllegalStateException expected){guard=expected.getMessage();}catch(Throwable error){throw new AssertionError(error);}
            var empty=event();worker(empty);
            var enabled=new AtomicBoolean(true);var plugin=plugin("registered",enabled);var disabled=new AtomicBoolean(true);var inactive=plugin("disabled",disabled);
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.MONITOR,false,(listener,event)->{TRACE.add("monitor");var spawn=(AsyncStructureSpawnEvent)event;if(spawn.getChunkX()!=-1||spawn.getChunkZ()!=-2)throw new AssertionError("chunk payload");var copy=spawn.getBoundingBox();copy.shift(100,0,0);if(spawn.getBoundingBox().getMinX()!=-12)throw new AssertionError("mutable bounds escaped");spawn.setCancelled(true);});
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.LOWEST,false,(listener,event)->{TRACE.add("cancel");((AsyncStructureSpawnEvent)event).setCancelled(true);});
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.LOW,true,(listener,event)->TRACE.add("ignored"));
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.NORMAL,false,(listener,event)->{TRACE.add("uncancel");((AsyncStructureSpawnEvent)event).setCancelled(false);});
            add(AsyncStructureSpawnEvent.class,inactive,EventPriority.HIGH,false,(listener,event)->TRACE.add("disabled"));disabled.set(false);
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.HIGHEST,false,(listener,event)->{TRACE.add("failure");throw new IllegalStateException("recoverable");});
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.HIGHEST,true,(listener,event)->TRACE.add("after-exception"));
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.HIGHEST,false,(listener,event)->{TRACE.add("nag-one");throw new AuthorNagException("first");});
            add(AsyncStructureSpawnEvent.class,plugin,EventPriority.HIGHEST,false,(listener,event)->{TRACE.add("nag-two");throw new AuthorNagException("second");});
            add(ServerExceptionEvent.class,plugin,EventPriority.NORMAL,false,(listener,event)->{TRACE.add("exception");var original=((com.destroystokyo.paper.exception.ServerEventException)((ServerExceptionEvent)event).getException()).getEvent();((AsyncStructureSpawnEvent)original).setCancelled(true);throw new IllegalArgumentException("exception handler failed");});
            var first=event();worker(first);var firstTrace=List.copyOf(TRACE);TRACE.clear();var firstLogs=List.copyOf(LOGS);LOGS.clear();
            var second=event();worker(second);
            var row=new JsonObject();row.addProperty("empty_primary_error",guard);row.addProperty("empty_worker_cancelled",empty.isCancelled());row.addProperty("first_cancelled",first.isCancelled());row.addProperty("second_cancelled",second.isCancelled());
            row.add("first_trace",new Gson().toJsonTree(firstTrace));row.add("second_trace",new Gson().toJsonTree(TRACE));row.add("first_logs",new Gson().toJsonTree(firstLogs));row.add("second_logs",new Gson().toJsonTree(LOGS));output.println(row);
            HandlerList.unregisterAll();
        }
    }
}
