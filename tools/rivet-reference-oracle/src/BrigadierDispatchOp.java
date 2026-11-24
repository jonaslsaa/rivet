package dev.rivet.oracle;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.mojang.brigadier.AmbiguityConsumer;
import com.mojang.brigadier.LiteralMessage;
import com.mojang.brigadier.Message;
import com.mojang.brigadier.ParseResults;
import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.RedirectModifier;
import com.mojang.brigadier.arguments.ArgumentType;
import com.mojang.brigadier.arguments.BoolArgumentType;
import com.mojang.brigadier.arguments.DoubleArgumentType;
import com.mojang.brigadier.arguments.FloatArgumentType;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import com.mojang.brigadier.arguments.LongArgumentType;
import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.builder.ArgumentBuilder;
import com.mojang.brigadier.builder.LiteralArgumentBuilder;
import com.mojang.brigadier.builder.RequiredArgumentBuilder;
import com.mojang.brigadier.context.CommandContextBuilder;
import com.mojang.brigadier.context.ParsedArgument;
import com.mojang.brigadier.context.ParsedCommandNode;
import com.mojang.brigadier.context.StringRange;
import com.mojang.brigadier.exceptions.BuiltInExceptionProvider;
import com.mojang.brigadier.exceptions.CommandExceptionType;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import com.mojang.brigadier.exceptions.SimpleCommandExceptionType;
import com.mojang.brigadier.suggestion.Suggestion;
import com.mojang.brigadier.suggestion.SuggestionProvider;
import com.mojang.brigadier.suggestion.Suggestions;
import com.mojang.brigadier.tree.CommandNode;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;

/**
 * The {@code brigadier.dispatch} op (VERIFICATION §5 item 10): build a
 * {@link CommandDispatcher} from a declarative JSON spec, then answer one facet
 * of Mojang Brigadier (as bundled with Paper) for one command string.
 *
 * <p>Request: {@code spec} (the dispatcher), {@code input} (the command string),
 * {@code cursor} (the suggestion cursor, in UTF-16 units) and {@code want}, a list of
 * facets: {@code run} (parse result, execute result or exception, result-consumer
 * calls), {@code suggestions}, {@code usage} (needs {@code usage_path}, the node names
 * from the root) and {@code ambiguities}.
 *
 * <p>Spec: {@code {"commands":[<root literal>...]}}. A node is
 * {@code {"literal":name}} or {@code {"argument":name,"type":{...}}} with optional
 * {@code requires} (a fixed boolean, default true), {@code executes} (an int result, or
 * {@code {"throws":message}}), {@code children}, {@code redirect}
 * ({@code {"to":"root"|<root literal>,"kind":"plain"|"single"|"fork"|"forward",
 * "copies":n,"fork":bool,"throws":message}}) and, on arguments, {@code suggests} (a
 * fixed provider: strings, integers or objects with a tooltip). A redirect names a
 * root literal or the dispatcher root. Every root literal is registered shallowly
 * first (its requirement, and its own redirect, which needs an earlier root literal),
 * so the tree can redirect to itself the way vanilla's {@code execute} does; the full
 * literals then merge into those nodes. The command source is the string {@code "src"}.
 */
final class BrigadierDispatchOp {
    static final List<String> RESULT_KEYS = List.of("run", "suggestions", "usage", "ambiguities");

    private static final String SOURCE = "src";

    private BrigadierDispatchOp() {
    }

    /** One exception type per {@code BuiltInExceptionProvider} method, by identity. */
    private static final Map<CommandExceptionType, String> TYPE_NAMES = typeNames();

    private static Map<CommandExceptionType, String> typeNames() {
        Map<CommandExceptionType, String> names = new IdentityHashMap<>();
        try {
            for (Method method : BuiltInExceptionProvider.class.getMethods()) {
                if (method.getParameterCount() == 0
                    && CommandExceptionType.class.isAssignableFrom(method.getReturnType())) {
                    names.put((CommandExceptionType) method.invoke(CommandSyntaxException.BUILT_IN_EXCEPTIONS), method.getName());
                }
            }
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException(error);
        }
        return names;
    }

    // ---- spec -> dispatcher -----------------------------------------------

    private record Built(CommandDispatcher<String> dispatcher, Map<String, CommandNode<String>> roots) {
    }

    private static Built build(final JsonObject spec) {
        CommandDispatcher<String> dispatcher = new CommandDispatcher<>();
        Map<String, CommandNode<String>> roots = new java.util.LinkedHashMap<>();
        JsonArray commands = spec.getAsJsonArray("commands");
        // Registering a literal that already exists merges only its children and
        // command, so the requirement and the redirect of a root literal must be
        // on the first, shallow registration.
        for (JsonElement command : commands) {
            JsonObject json = command.getAsJsonObject();
            String name = json.get("literal").getAsString();
            roots.put(name, dispatcher.register((LiteralArgumentBuilder<String>) configure(
                LiteralArgumentBuilder.<String>literal(name), json, dispatcher, roots, true)));
        }
        for (JsonElement command : commands) {
            JsonObject json = command.getAsJsonObject();
            if (!json.has("redirect")) {
                dispatcher.register((LiteralArgumentBuilder<String>) node(json, dispatcher, roots));
            }
        }
        return new Built(dispatcher, roots);
    }

    private static ArgumentBuilder<String, ?> node(
        final JsonObject json, final CommandDispatcher<String> dispatcher, final Map<String, CommandNode<String>> roots
    ) {
        ArgumentBuilder<String, ?> builder;
        if (json.has("literal")) {
            builder = LiteralArgumentBuilder.<String>literal(json.get("literal").getAsString());
        } else {
            RequiredArgumentBuilder<String, Object> argument = RequiredArgumentBuilder.<String, Object>argument(
                json.get("argument").getAsString(), argumentType(json.getAsJsonObject("type")));
            if (json.has("suggests")) {
                argument.suggests(provider(json.getAsJsonArray("suggests")));
            }
            builder = argument;
        }
        return configure(builder, json, dispatcher, roots, false);
    }

    /** Applies requires/executes/children/redirect; {@code shallow} keeps only what a first registration must carry. */
    private static ArgumentBuilder<String, ?> configure(
        final ArgumentBuilder<String, ?> builder, final JsonObject json,
        final CommandDispatcher<String> dispatcher, final Map<String, CommandNode<String>> roots, final boolean shallow
    ) {
        if (json.has("requires")) {
            final boolean allowed = json.get("requires").getAsBoolean();
            builder.requires(source -> allowed);
        }
        if (shallow && !json.has("redirect")) {
            return builder;
        }
        if (json.has("executes")) {
            JsonElement executes = json.get("executes");
            if (executes.isJsonObject()) {
                final CommandSyntaxException failure = new SimpleCommandExceptionType(
                    new LiteralMessage(executes.getAsJsonObject().get("throws").getAsString())).create();
                builder.executes(context -> {
                    throw failure;
                });
            } else {
                final int result = executes.getAsInt();
                builder.executes(context -> result);
            }
        }
        if (!shallow && json.has("children")) {
            for (JsonElement child : json.getAsJsonArray("children")) {
                builder.then(node(child.getAsJsonObject(), dispatcher, roots));
            }
        }
        if (json.has("redirect")) {
            redirect(builder, json.getAsJsonObject("redirect"), dispatcher, roots);
        }
        return builder;
    }

    private static void redirect(
        final ArgumentBuilder<String, ?> builder, final JsonObject json,
        final CommandDispatcher<String> dispatcher, final Map<String, CommandNode<String>> roots
    ) {
        String to = json.get("to").getAsString();
        CommandNode<String> target = to.equals("root") ? dispatcher.getRoot() : roots.get(to);
        if (target == null) {
            throw new IllegalArgumentException("redirect target " + to + " is not a root literal");
        }
        final int copies = json.has("copies") ? json.get("copies").getAsInt() : 2;
        final CommandSyntaxException failure = json.has("throws")
            ? new SimpleCommandExceptionType(new LiteralMessage(json.get("throws").getAsString())).create()
            : null;
        RedirectModifier<String> modifier = context -> {
            if (failure != null) {
                throw failure;
            }
            List<String> sources = new ArrayList<>();
            for (int i = 0; i < copies; i++) {
                sources.add(context.getSource());
            }
            return sources;
        };
        switch (json.get("kind").getAsString()) {
            case "plain" -> builder.redirect(target);
            case "single" -> builder.redirect(target, context -> context.getSource());
            case "fork" -> builder.fork(target, modifier);
            case "forward" -> builder.forward(target, modifier, json.get("fork").getAsBoolean());
            default -> throw new IllegalArgumentException("unknown redirect kind");
        }
    }

    /**
     * A fixed suggestion provider: text entries are suggested when they start with the
     * lower-cased remaining input (the vanilla filter), integer entries always; an entry
     * is a string, an integer or {@code {"text"|"value":..,"tooltip":..}}.
     */
    private static SuggestionProvider<String> provider(final JsonArray entries) {
        return (context, builder) -> {
            for (JsonElement element : entries) {
                JsonObject entry = element.isJsonObject() ? element.getAsJsonObject() : null;
                JsonElement value = entry == null ? element : (entry.has("text") ? entry.get("text") : entry.get("value"));
                Message tooltip = entry != null && entry.has("tooltip")
                    ? new LiteralMessage(entry.get("tooltip").getAsString()) : null;
                if (value.getAsJsonPrimitive().isNumber()) {
                    if (tooltip == null) {
                        builder.suggest(value.getAsInt());
                    } else {
                        builder.suggest(value.getAsInt(), tooltip);
                    }
                } else if (value.getAsString().toLowerCase(Locale.ROOT).startsWith(builder.getRemainingLowerCase())) {
                    if (tooltip == null) {
                        builder.suggest(value.getAsString());
                    } else {
                        builder.suggest(value.getAsString(), tooltip);
                    }
                }
            }
            return builder.buildFuture();
        };
    }

    @SuppressWarnings("unchecked")
    private static <T> ArgumentType<T> argumentType(final JsonObject type) {
        return (ArgumentType<T>) switch (type.get("kind").getAsString()) {
            case "bool" -> BoolArgumentType.bool();
            case "integer" -> IntegerArgumentType.integer(
                type.has("min") ? type.get("min").getAsInt() : Integer.MIN_VALUE,
                type.has("max") ? type.get("max").getAsInt() : Integer.MAX_VALUE);
            case "long" -> LongArgumentType.longArg(
                type.has("min") ? type.get("min").getAsLong() : Long.MIN_VALUE,
                type.has("max") ? type.get("max").getAsLong() : Long.MAX_VALUE);
            case "float" -> FloatArgumentType.floatArg(
                type.has("min") ? type.get("min").getAsFloat() : -Float.MAX_VALUE,
                type.has("max") ? type.get("max").getAsFloat() : Float.MAX_VALUE);
            case "double" -> DoubleArgumentType.doubleArg(
                type.has("min") ? type.get("min").getAsDouble() : -Double.MAX_VALUE,
                type.has("max") ? type.get("max").getAsDouble() : Double.MAX_VALUE);
            case "string" -> switch (type.get("mode").getAsString()) {
                case "word" -> StringArgumentType.word();
                case "quotable" -> StringArgumentType.string();
                case "greedy" -> StringArgumentType.greedyString();
                default -> throw new IllegalArgumentException("unknown string mode");
            };
            default -> throw new IllegalArgumentException("unknown argument type kind");
        };
    }

    // ---- facets -----------------------------------------------------------

    static JsonObject run(final JsonObject request) throws Exception {
        Built built = build(request.getAsJsonObject("spec"));
        String input = request.get("input").getAsString();
        JsonObject result = new JsonObject();
        for (JsonElement want : request.getAsJsonArray("want")) {
            switch (want.getAsString()) {
                case "run" -> result.add("run", runFacet(built, input));
                case "suggestions" -> result.add("suggestions", suggestFacet(built, input, request.get("cursor").getAsInt()));
                case "usage" -> result.add("usage", usageFacet(built, request.getAsJsonArray("usage_path")));
                case "ambiguities" -> result.add("ambiguities", ambiguityFacet(built));
                default -> throw new IllegalArgumentException("unknown facet " + want.getAsString());
            }
        }
        return result;
    }

    private static JsonObject runFacet(final Built built, final String input) {
        CommandDispatcher<String> dispatcher = built.dispatcher();
        JsonArray consumed = new JsonArray();
        dispatcher.setConsumer((context, success, code) -> {
            JsonObject call = new JsonObject();
            call.addProperty("success", success);
            call.addProperty("result", code);
            consumed.add(call);
        });
        ParseResults<String> parse = dispatcher.parse(input, SOURCE);
        JsonObject run = new JsonObject();
        run.add("parse", parseJson(dispatcher, parse));
        JsonObject executed = new JsonObject();
        try {
            executed.addProperty("result", dispatcher.execute(parse));
        } catch (CommandSyntaxException error) {
            executed.add("exception", exceptionJson(error));
        }
        run.add("execute", executed);
        run.add("consumer", consumed);
        return run;
    }

    private static JsonObject parseJson(final CommandDispatcher<String> dispatcher, final ParseResults<String> parse) {
        JsonObject json = new JsonObject();
        json.addProperty("cursor", parse.getReader().getCursor());
        json.addProperty("remaining", parse.getReader().getRemaining());
        json.add("context", contextJson(dispatcher, parse.getContext()));
        JsonArray errors = new JsonArray();
        for (Map.Entry<CommandNode<String>, CommandSyntaxException> entry : parse.getExceptions().entrySet()) {
            JsonObject error = new JsonObject();
            error.add("node", path(dispatcher, entry.getKey()));
            error.add("exception", exceptionJson(entry.getValue()));
            errors.add(error);
        }
        json.add("errors", errors);
        return json;
    }

    private static JsonObject contextJson(final CommandDispatcher<String> dispatcher, final CommandContextBuilder<String> context) {
        JsonObject json = new JsonObject();
        json.add("range", range(context.getRange()));
        json.addProperty("has_command", context.getCommand() != null);
        JsonArray nodes = new JsonArray();
        for (ParsedCommandNode<String> parsed : context.getNodes()) {
            JsonObject node = new JsonObject();
            node.add("path", path(dispatcher, parsed.getNode()));
            node.add("range", range(parsed.getRange()));
            nodes.add(node);
        }
        json.add("nodes", nodes);
        JsonArray arguments = new JsonArray();
        for (Map.Entry<String, ParsedArgument<String, ?>> entry : new TreeMap<>(context.getArguments()).entrySet()) {
            JsonObject argument = new JsonObject();
            argument.addProperty("name", entry.getKey());
            argument.add("range", range(entry.getValue().getRange()));
            argument.add("value", value(entry.getValue().getResult()));
            arguments.add(argument);
        }
        json.add("arguments", arguments);
        // An absent value is an absent key: the transport drops JSON nulls.
        if (context.getChild() != null) {
            json.add("child", contextJson(dispatcher, context.getChild()));
        }
        return json;
    }

    private static JsonObject value(final Object result) {
        JsonObject json = new JsonObject();
        if (result instanceof Boolean value) {
            json.addProperty("type", "bool");
            json.addProperty("value", value);
        } else if (result instanceof Integer value) {
            json.addProperty("type", "integer");
            json.addProperty("value", value);
        } else if (result instanceof Long value) {
            json.addProperty("type", "long");
            json.addProperty("value", value);
        } else if (result instanceof Float value) {
            json.addProperty("type", "float");
            json.addProperty("bits", RivetReferenceOracle.f32Bits(value));
        } else if (result instanceof Double value) {
            json.addProperty("type", "double");
            json.addProperty("bits", RivetReferenceOracle.f64Bits(value));
        } else if (result instanceof String value) {
            json.addProperty("type", "string");
            json.addProperty("value", value);
        } else {
            throw new IllegalStateException("unexpected argument value " + result.getClass());
        }
        return json;
    }

    private static JsonArray range(final StringRange range) {
        JsonArray json = new JsonArray();
        json.add(range.getStart());
        json.add(range.getEnd());
        return json;
    }

    private static JsonArray path(final CommandDispatcher<String> dispatcher, final CommandNode<String> node) {
        JsonArray json = new JsonArray();
        for (String name : dispatcher.getPath(node)) {
            json.add(name);
        }
        return json;
    }

    private static JsonObject exceptionJson(final CommandSyntaxException error) {
        JsonObject json = new JsonObject();
        json.addProperty("type", TYPE_NAMES.getOrDefault(error.getType(), "other"));
        json.addProperty("message", error.getMessage());
        json.addProperty("raw", error.getRawMessage().getString());
        json.addProperty("cursor", error.getCursor());
        if (error.getInput() != null) {
            json.addProperty("input", error.getInput());
        }
        return json;
    }

    private static JsonObject suggestFacet(final Built built, final String input, final int cursor) {
        CommandDispatcher<String> dispatcher = built.dispatcher();
        Suggestions suggestions = dispatcher.getCompletionSuggestions(dispatcher.parse(input, SOURCE), cursor).join();
        JsonObject json = new JsonObject();
        json.add("range", range(suggestions.getRange()));
        JsonArray list = new JsonArray();
        for (Suggestion suggestion : suggestions.getList()) {
            JsonObject entry = new JsonObject();
            entry.addProperty("text", suggestion.getText());
            entry.add("range", range(suggestion.getRange()));
            if (suggestion.getTooltip() != null) {
                entry.addProperty("tooltip", suggestion.getTooltip().getString());
            }
            list.add(entry);
        }
        json.add("list", list);
        return json;
    }

    private static JsonObject usageFacet(final Built built, final JsonArray usagePath) {
        CommandDispatcher<String> dispatcher = built.dispatcher();
        List<String> names = new ArrayList<>();
        for (JsonElement name : usagePath) {
            names.add(name.getAsString());
        }
        CommandNode<String> node = dispatcher.findNode(names);
        if (node == null) {
            throw new IllegalArgumentException("usage_path names no node");
        }
        JsonObject json = new JsonObject();
        json.add("all", strings(dispatcher.getAllUsage(node, SOURCE, false)));
        json.add("all_restricted", strings(dispatcher.getAllUsage(node, SOURCE, true)));
        JsonArray smart = new JsonArray();
        for (Map.Entry<CommandNode<String>, String> entry : dispatcher.getSmartUsage(node, SOURCE).entrySet()) {
            JsonObject line = new JsonObject();
            line.addProperty("node", entry.getKey().getName());
            line.addProperty("usage", entry.getValue());
            smart.add(line);
        }
        json.add("smart", smart);
        return json;
    }

    private static JsonArray strings(final String[] values) {
        JsonArray json = new JsonArray();
        for (String value : values) {
            json.add(value);
        }
        return json;
    }

    private static JsonArray ambiguityFacet(final Built built) {
        JsonArray found = new JsonArray();
        AmbiguityConsumer<String> consumer = (parent, child, sibling, inputs) -> {
            JsonObject entry = new JsonObject();
            // Node names, not paths: the Rust consumer is handed a `&dyn CommandNode` and
            // cannot recover the tree position of a node by identity.
            entry.addProperty("parent", parent.getName());
            entry.addProperty("child", child.getName());
            entry.addProperty("sibling", sibling.getName());
            JsonArray list = new JsonArray();
            for (String input : inputs) {
                list.add(input);
            }
            entry.add("inputs", list);
            found.add(entry);
        };
        built.dispatcher().findAmbiguities(consumer);
        return found;
    }

    /** True for no exception: every Brigadier answer, including its exceptions, is data. */
    static boolean isRejection(final Exception error) {
        return false;
    }
}
