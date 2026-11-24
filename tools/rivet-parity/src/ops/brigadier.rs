//! `brigadier.dispatch`: the Rust twin of the reference oracle's op of that
//! name (`BrigadierDispatchOp.java`).
//!
//! A request carries a declarative dispatcher spec (JSON), one command string,
//! a suggestion cursor and the facets wanted. Both sides build the same
//! dispatcher from the spec, with Brigadier's built-in argument types only, and
//! answer: the parse result (context nodes, parsed arguments and their ranges,
//! the reader cursor, the per-node errors), the execute result or exception,
//! the result-consumer calls, the completion suggestions, `getAllUsage` and
//! `getSmartUsage`, and `findAmbiguities`. Brigadier's own exceptions are data
//! inside the result (type, message, raw message, cursor, input); the op never
//! answers "rejected".
//!
//! Everything here calls the ported `rivet-brigadier`; the handler only mirrors
//! the op contract around it (spec decoding, result JSON).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rivet_brigadier::builder::argument_builder::ArgumentBuilderBehavior;
use rivet_brigadier::builder::literal_argument_builder::LiteralArgumentBuilder;
use rivet_brigadier::builder::required_argument_builder::RequiredArgumentBuilder;
use rivet_brigadier::context::CommandContext;
use rivet_brigadier::context::CommandContextBuilder;
use rivet_brigadier::exceptions::{
    BuiltInExceptionProvider, CommandExceptionType, CommandSyntaxException,
    SimpleCommandExceptionType, exception_type_eq,
};
use rivet_brigadier::tree::CommandNode;
use rivet_brigadier::{
    AmbiguityConsumer, BoolArgumentType, Command, CommandDispatcher, DoubleArgumentType,
    FloatArgumentType, ImmutableStringReader, IntegerArgumentType, LiteralMessage,
    LongArgumentType, Message, RedirectModifier, ResultConsumer, SingleRedirectModifier,
    StringArgumentType, StringRange, SuggestionProvider, Suggestions, SuggestionsBuilder,
};
use serde_json::{Map, Value, json};

use super::compare::Outcome;

/// The command source: Paper's `CommandSourceStack` is not needed, only a
/// value that is not a function or command-block source.
const SOURCE: &str = "src";

type Node = Arc<dyn CommandNode<String>>;

/// A spec or request the op cannot interpret: infrastructure fault, never a verdict.
type Fault = String;

fn fault<T>(message: impl Into<String>) -> Result<T, Fault> {
    Err(message.into())
}

// ---- spec -> dispatcher ----------------------------------------------------

struct Built {
    dispatcher: CommandDispatcher<String>,
}

struct Context<'a> {
    root: Node,
    roots: &'a HashMap<String, Node>,
}

/// `new SimpleCommandExceptionType(new LiteralMessage(message))`, one `'static`
/// instance per message (a thrown exception outlives the spec's builders).
fn failure_type(message: &str) -> &'static SimpleCommandExceptionType {
    static TYPES: OnceLock<Mutex<HashMap<String, &'static SimpleCommandExceptionType>>> =
        OnceLock::new();
    let mut types = TYPES
        .get_or_init(Default::default)
        .lock()
        .expect("failure type cache");
    types.entry(message.to_owned()).or_insert_with(|| {
        Box::leak(Box::new(SimpleCommandExceptionType::new(
            LiteralMessage::new(message),
        )))
    })
}

struct FixedCommand(i32);

impl Command<String> for FixedCommand {
    fn run(&self, _: &CommandContext<String>) -> Result<i32, CommandSyntaxException<'static>> {
        Ok(self.0)
    }
}

struct ThrowingCommand(String);

impl Command<String> for ThrowingCommand {
    fn run(&self, _: &CommandContext<String>) -> Result<i32, CommandSyntaxException<'static>> {
        Err(failure_type(&self.0).create())
    }
}

/// A fixed suggestion provider (`suggests` in the spec): text entries are
/// suggested when they start with the lower-cased remaining input (the vanilla
/// filter), integer entries always.
struct FixedSuggestions(Vec<(Value, Option<String>)>);

impl SuggestionProvider<String> for FixedSuggestions {
    fn get_suggestions(
        &self,
        _: &CommandContext<String>,
        builder: &mut SuggestionsBuilder,
    ) -> Result<Suggestions, CommandSyntaxException<'static>> {
        for (value, tooltip) in &self.0 {
            let tooltip: Option<Arc<dyn Message>> = tooltip
                .as_ref()
                .map(|text| Arc::new(LiteralMessage::new(text.clone())) as Arc<dyn Message>);
            match value {
                Value::Number(n) => {
                    let n = n
                        .as_i64()
                        .and_then(|v| i32::try_from(v).ok())
                        .expect("checked at build");
                    match tooltip {
                        Some(tooltip) => builder.suggest_int_with_tooltip(n, tooltip),
                        None => builder.suggest_int(n),
                    };
                }
                Value::String(text) => {
                    if text
                        .to_lowercase()
                        .starts_with(builder.get_remaining_lower_case())
                    {
                        match tooltip {
                            Some(tooltip) => builder.suggest_with_tooltip(text, tooltip),
                            None => builder.suggest(text),
                        };
                    }
                }
                _ => unreachable!("checked at build"),
            }
        }
        Ok(builder.build())
    }
}

fn provider(entries: &[Value]) -> Result<Arc<dyn SuggestionProvider<String>>, Fault> {
    let mut parsed = Vec::new();
    for entry in entries {
        let (value, tooltip) = match entry {
            Value::Object(object) => {
                let value = object
                    .get("text")
                    .or_else(|| object.get("value"))
                    .ok_or("a suggestion object needs `text` or `value`")?;
                let tooltip = match object.get("tooltip") {
                    None => None,
                    Some(t) => Some(t.as_str().ok_or("`tooltip` must be a string")?.to_owned()),
                };
                (value.clone(), tooltip)
            }
            other => (other.clone(), None),
        };
        match &value {
            Value::String(_) => {}
            Value::Number(n) if n.as_i64().is_some_and(|v| i32::try_from(v).is_ok()) => {}
            _ => return fault("a suggestion is a string or an int"),
        }
        parsed.push((value, tooltip));
    }
    Ok(Arc::new(FixedSuggestions(parsed)))
}

/// The fork modifier: `copies` copies of the source, or a thrown exception.
struct CopiesModifier {
    copies: i32,
    throws: Option<String>,
}

impl RedirectModifier<String> for CopiesModifier {
    fn apply(
        &self,
        context: &CommandContext<String>,
    ) -> Result<Vec<String>, CommandSyntaxException<'static>> {
        if let Some(message) = &self.throws {
            return Err(failure_type(message).create());
        }
        Ok((0..self.copies)
            .map(|_| context.get_source().clone())
            .collect())
    }
}

struct SameSource;

impl SingleRedirectModifier<String> for SameSource {
    fn apply(
        &self,
        context: &CommandContext<String>,
    ) -> Result<String, CommandSyntaxException<'static>> {
        Ok(context.get_source().clone())
    }
}

fn get<'a>(json: &'a Map<String, Value>, key: &str) -> Result<&'a Value, Fault> {
    json.get(key)
        .ok_or_else(|| format!("spec node lacks `{key}`"))
}

fn get_str<'a>(json: &'a Map<String, Value>, key: &str) -> Result<&'a str, Fault> {
    get(json, key)?
        .as_str()
        .ok_or_else(|| format!("spec `{key}` must be a string"))
}

fn get_object<'a>(
    json: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a Map<String, Value>, Fault> {
    get(json, key)?
        .as_object()
        .ok_or_else(|| format!("spec `{key}` must be an object"))
}

fn get_array<'a>(json: &'a Map<String, Value>, key: &str) -> Result<&'a Vec<Value>, Fault> {
    get(json, key)?
        .as_array()
        .ok_or_else(|| format!("spec `{key}` must be an array"))
}

fn bound<T>(
    json: &Map<String, Value>,
    key: &str,
    default: T,
    parse: impl Fn(&serde_json::Number) -> Option<T>,
) -> Result<T, Fault> {
    match json.get(key) {
        None => Ok(default),
        Some(Value::Number(n)) => parse(n).ok_or_else(|| format!("bound `{key}` out of range")),
        Some(_) => fault(format!("bound `{key}` must be a number")),
    }
}

/// Applies the spec fields of a node to its builder. `shallow` is a root
/// literal's first registration: only what a later merge cannot carry (the
/// requirement and, for a redirecting literal, the command and the redirect).
fn configure<B: ArgumentBuilderBehavior<String>>(
    builder: &mut B,
    json: &Map<String, Value>,
    context: &Context,
    shallow: bool,
) -> Result<(), Fault> {
    if let Some(requires) = json.get("requires") {
        let allowed = requires.as_bool().ok_or("`requires` must be a boolean")?;
        builder.requires(Arc::new(move |_: &String| allowed));
    }
    if shallow && !json.contains_key("redirect") {
        return Ok(());
    }
    if let Some(executes) = json.get("executes") {
        let command: Arc<dyn Command<String>> = match executes {
            Value::Object(failure) => {
                Arc::new(ThrowingCommand(get_str(failure, "throws")?.to_owned()))
            }
            Value::Number(n) => Arc::new(FixedCommand(
                n.as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or("`executes` must be an int")?,
            )),
            _ => return fault("`executes` must be an int or {throws}"),
        };
        builder.executes(Some(command));
    }
    if let Some(children) = json.get("children").filter(|_| !shallow) {
        for child in children.as_array().ok_or("`children` must be an array")? {
            let child = child.as_object().ok_or("a child must be an object")?;
            builder.then_node(node(child, context)?);
        }
    }
    if let Some(redirect) = json.get("redirect") {
        let redirect = redirect.as_object().ok_or("`redirect` must be an object")?;
        apply_redirect(builder, redirect, context)?;
    }
    Ok(())
}

fn apply_redirect<B: ArgumentBuilderBehavior<String>>(
    builder: &mut B,
    json: &Map<String, Value>,
    context: &Context,
) -> Result<(), Fault> {
    let to = get_str(json, "to")?;
    let target: Node = if to == "root" {
        Arc::clone(&context.root)
    } else {
        context
            .roots
            .get(to)
            .cloned()
            .ok_or_else(|| format!("redirect target {to} is not a root literal"))?
    };
    let copies = match json.get("copies") {
        None => 2,
        Some(n) => n
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or("`copies` must be an int")?,
    };
    let throws = match json.get("throws") {
        None => None,
        Some(message) => Some(
            message
                .as_str()
                .ok_or("`throws` must be a string")?
                .to_owned(),
        ),
    };
    let modifier = || -> Arc<dyn RedirectModifier<String>> {
        Arc::new(CopiesModifier {
            copies,
            throws: throws.clone(),
        })
    };
    match get_str(json, "kind")? {
        "plain" => builder.redirect(target),
        "single" => builder.redirect_with_modifier(target, Some(Arc::new(SameSource))),
        "fork" => builder.fork(target, modifier()),
        "forward" => {
            let fork = get(json, "fork")?
                .as_bool()
                .ok_or("`fork` must be a boolean")?;
            builder.forward(Some(target), Some(modifier()), fork)
        }
        other => return fault(format!("unknown redirect kind {other}")),
    };
    Ok(())
}

fn node(json: &Map<String, Value>, context: &Context) -> Result<Node, Fault> {
    if let Some(name) = json.get("literal") {
        let mut builder = LiteralArgumentBuilder::<String>::literal(
            name.as_str().ok_or("`literal` must be a string")?,
        );
        configure(&mut builder, json, context, false)?;
        return Ok(Arc::from(builder.build()));
    }
    let name = get_str(json, "argument")?;
    let kind = get_object(json, "type")?;
    macro_rules! argument {
        ($ty:expr) => {{
            let mut builder = RequiredArgumentBuilder::<String, _>::argument(name, $ty);
            if let Some(entries) = json.get("suggests") {
                let entries = entries.as_array().ok_or("`suggests` must be an array")?;
                builder.suggests(Some(provider(entries)?));
            }
            configure(&mut builder, json, context, false)?;
            Ok(Arc::from(builder.build()))
        }};
    }
    match get_str(kind, "kind")? {
        "bool" => argument!(BoolArgumentType::bool()),
        "integer" => argument!(IntegerArgumentType::integer_with_bounds(
            bound(kind, "min", i32::MIN, |n| n
                .as_i64()
                .and_then(|v| i32::try_from(v).ok()))?,
            bound(kind, "max", i32::MAX, |n| n
                .as_i64()
                .and_then(|v| i32::try_from(v).ok()))?,
        )),
        "long" => argument!(LongArgumentType::long_arg_with_bounds(
            bound(kind, "min", i64::MIN, serde_json::Number::as_i64)?,
            bound(kind, "max", i64::MAX, serde_json::Number::as_i64)?,
        )),
        "float" => argument!(FloatArgumentType::float_arg_with_bounds(
            bound(kind, "min", -f32::MAX, |n| n.to_string().parse().ok())?,
            bound(kind, "max", f32::MAX, |n| n.to_string().parse().ok())?,
        )),
        "double" => argument!(DoubleArgumentType::double_arg_with_bounds(
            bound(kind, "min", -f64::MAX, |n| n.to_string().parse().ok())?,
            bound(kind, "max", f64::MAX, |n| n.to_string().parse().ok())?,
        )),
        "string" => match get_str(kind, "mode")? {
            "word" => argument!(StringArgumentType::word()),
            "quotable" => argument!(StringArgumentType::string()),
            "greedy" => argument!(StringArgumentType::greedy_string()),
            other => fault(format!("unknown string mode {other}")),
        },
        other => fault(format!("unknown argument type kind {other}")),
    }
}

/// Registers every root literal shallowly first (the requirement and, for a
/// redirecting literal, its command and redirect, which a later merge cannot
/// carry; so a redirect can target any earlier root literal, and a literal's
/// children can target the literal itself, the way vanilla's `execute` does),
/// then registers the full literals, which merge into the first nodes.
fn build(spec: &Map<String, Value>) -> Result<Built, Fault> {
    let mut dispatcher = CommandDispatcher::<String>::new();
    let root: Node = Arc::clone(dispatcher.get_root()) as Node;
    let commands = get_array(spec, "commands")?;
    let mut roots: HashMap<String, Node> = HashMap::new();
    for command in commands {
        let command = command.as_object().ok_or("a command must be an object")?;
        let name = get_str(command, "literal")?;
        let mut builder = LiteralArgumentBuilder::<String>::literal(name);
        let context = Context {
            root: Arc::clone(&root),
            roots: &roots,
        };
        configure(&mut builder, command, &context, true)?;
        let registered = dispatcher.register(&mut builder);
        roots.insert(name.to_owned(), registered as Node);
    }
    let context = Context {
        root,
        roots: &roots,
    };
    for command in commands {
        let command = command.as_object().ok_or("a command must be an object")?;
        if command.contains_key("redirect") {
            continue;
        }
        let mut builder = LiteralArgumentBuilder::<String>::literal(get_str(command, "literal")?);
        configure(&mut builder, command, &context, false)?;
        dispatcher.register(&mut builder);
    }
    Ok(Built { dispatcher })
}

// ---- results -----------------------------------------------------------------

fn range(range: StringRange) -> Value {
    json!([range.get_start(), range.get_end()])
}

fn path(dispatcher: &CommandDispatcher<String>, node: &Node) -> Value {
    json!(dispatcher.get_path(node))
}

/// `BuiltInExceptionProvider` method name of an exception type (by identity),
/// `other` for any type that is not a built-in one.
fn type_name(exception_type: &dyn CommandExceptionType) -> &'static str {
    let built_in = CommandSyntaxException::built_in_exceptions();
    let table: [(&str, &dyn CommandExceptionType); 27] = [
        ("doubleTooLow", built_in.double_too_low()),
        ("doubleTooHigh", built_in.double_too_high()),
        ("floatTooLow", built_in.float_too_low()),
        ("floatTooHigh", built_in.float_too_high()),
        ("integerTooLow", built_in.integer_too_low()),
        ("integerTooHigh", built_in.integer_too_high()),
        ("longTooLow", built_in.long_too_low()),
        ("longTooHigh", built_in.long_too_high()),
        ("literalIncorrect", built_in.literal_incorrect()),
        (
            "readerExpectedStartOfQuote",
            built_in.reader_expected_start_of_quote(),
        ),
        (
            "readerExpectedEndOfQuote",
            built_in.reader_expected_end_of_quote(),
        ),
        ("readerInvalidEscape", built_in.reader_invalid_escape()),
        ("readerInvalidBool", built_in.reader_invalid_bool()),
        ("readerInvalidInt", built_in.reader_invalid_int()),
        ("readerExpectedInt", built_in.reader_expected_int()),
        ("readerInvalidLong", built_in.reader_invalid_long()),
        ("readerExpectedLong", built_in.reader_expected_long()),
        ("readerInvalidDouble", built_in.reader_invalid_double()),
        ("readerExpectedDouble", built_in.reader_expected_double()),
        ("readerInvalidFloat", built_in.reader_invalid_float()),
        ("readerExpectedFloat", built_in.reader_expected_float()),
        ("readerExpectedBool", built_in.reader_expected_bool()),
        ("readerExpectedSymbol", built_in.reader_expected_symbol()),
        (
            "dispatcherUnknownCommand",
            built_in.dispatcher_unknown_command(),
        ),
        (
            "dispatcherUnknownArgument",
            built_in.dispatcher_unknown_argument(),
        ),
        (
            "dispatcherExpectedArgumentSeparator",
            built_in.dispatcher_expected_argument_separator(),
        ),
        (
            "dispatcherParseException",
            built_in.dispatcher_parse_exception(),
        ),
    ];
    table
        .iter()
        .find(|(_, candidate)| exception_type_eq(*candidate, exception_type))
        .map_or("other", |(name, _)| name)
}

fn exception(error: &CommandSyntaxException<'_>) -> Value {
    let mut json = Map::new();
    json.insert("type".into(), json!(type_name(error.get_type())));
    json.insert("message".into(), json!(error.get_message()));
    json.insert("raw".into(), json!(error.get_raw_message().get_string()));
    json.insert("cursor".into(), json!(error.get_cursor()));
    // An absent value is an absent key: the oracle transport drops JSON nulls.
    if let Some(input) = error.get_input() {
        json.insert("input".into(), json!(input));
    }
    Value::Object(json)
}

fn argument_value(result: &dyn std::any::Any) -> Result<Value, Fault> {
    if let Some(value) = result.downcast_ref::<bool>() {
        Ok(json!({"type": "bool", "value": value}))
    } else if let Some(value) = result.downcast_ref::<i32>() {
        Ok(json!({"type": "integer", "value": value}))
    } else if let Some(value) = result.downcast_ref::<i64>() {
        Ok(json!({"type": "long", "value": value}))
    } else if let Some(value) = result.downcast_ref::<f32>() {
        Ok(json!({"type": "float", "bits": format!("0x{:08x}", canonical_f32(*value))}))
    } else if let Some(value) = result.downcast_ref::<f64>() {
        Ok(json!({"type": "double", "bits": format!("0x{:016x}", canonical_f64(*value))}))
    } else if let Some(value) = result.downcast_ref::<String>() {
        Ok(json!({"type": "string", "value": value}))
    } else {
        fault("unexpected parsed argument value")
    }
}

/// `Float.floatToIntBits`: every NaN is the canonical quiet NaN.
fn canonical_f32(value: f32) -> u32 {
    if value.is_nan() {
        0x7fc0_0000
    } else {
        value.to_bits()
    }
}

/// `Double.doubleToLongBits`: every NaN is the canonical quiet NaN.
fn canonical_f64(value: f64) -> u64 {
    if value.is_nan() {
        0x7ff8_0000_0000_0000
    } else {
        value.to_bits()
    }
}

fn context_json(
    dispatcher: &CommandDispatcher<String>,
    context: &CommandContextBuilder<String>,
) -> Result<Value, Fault> {
    let nodes: Vec<Value> = context
        .get_nodes()
        .iter()
        .map(|parsed| {
            json!({
                "path": path(dispatcher, &parsed.get_node()),
                "range": range(parsed.get_range()),
            })
        })
        .collect();
    let mut names: Vec<&String> = context.get_arguments().keys().collect();
    names.sort();
    let mut arguments = Vec::new();
    for name in names {
        let parsed = &context.get_arguments()[name];
        arguments.push(json!({
            "name": name,
            "range": range(parsed.get_range()),
            "value": argument_value(parsed.result_as_any())?,
        }));
    }
    let mut json = Map::new();
    json.insert("range".into(), range(context.get_range()));
    json.insert("has_command".into(), json!(context.get_command().is_some()));
    json.insert("nodes".into(), Value::Array(nodes));
    json.insert("arguments".into(), Value::Array(arguments));
    if let Some(child) = context.get_child() {
        json.insert("child".into(), context_json(dispatcher, child)?);
    }
    Ok(Value::Object(json))
}

struct RecordingConsumer(Arc<Mutex<Vec<Value>>>);

impl ResultConsumer<String> for RecordingConsumer {
    fn on_command_complete(&self, _: &CommandContext<String>, success: bool, result: i32) {
        self.0
            .lock()
            .expect("consumer calls")
            .push(json!({"success": success, "result": result}));
    }
}

fn run_facet(built: &mut Built, input: &str) -> Result<Value, Fault> {
    let consumed = Arc::new(Mutex::new(Vec::new()));
    built
        .dispatcher
        .set_consumer(Arc::new(RecordingConsumer(Arc::clone(&consumed))));
    let dispatcher = &built.dispatcher;
    let parse = dispatcher.parse_string(input, SOURCE.to_owned());
    let errors: Vec<Value> = parse
        .get_exceptions()
        .iter()
        .map(|(node, error)| json!({"node": path(dispatcher, node), "exception": exception(error)}))
        .collect();
    let parsed = json!({
        "cursor": parse.get_reader().get_cursor(),
        "remaining": parse.get_reader().get_remaining(),
        "context": context_json(dispatcher, parse.get_context())?,
        "errors": errors,
    });
    let executed = match dispatcher.execute_parse(parse) {
        Ok(result) => json!({"result": result}),
        Err(error) => json!({"exception": exception(&error)}),
    };
    let consumer = consumed.lock().expect("consumer calls").clone();
    Ok(json!({"parse": parsed, "execute": executed, "consumer": consumer}))
}

fn suggest_facet(built: &Built, input: &str, cursor: i32) -> Value {
    let dispatcher = &built.dispatcher;
    let parse = dispatcher.parse_string(input, SOURCE.to_owned());
    let suggestions = dispatcher.get_completion_suggestions_with_cursor(&parse, cursor);
    let list: Vec<Value> = suggestions
        .get_list()
        .iter()
        .map(|suggestion| {
            let mut entry = Map::new();
            entry.insert("text".into(), json!(suggestion.get_text()));
            entry.insert("range".into(), range(suggestion.get_range()));
            if let Some(tooltip) = suggestion.get_tooltip() {
                entry.insert("tooltip".into(), json!(tooltip.get_string()));
            }
            Value::Object(entry)
        })
        .collect();
    json!({"range": range(suggestions.get_range()), "list": list})
}

fn usage_facet(built: &Built, usage_path: &[Value]) -> Result<Value, Fault> {
    let dispatcher = &built.dispatcher;
    let names: Vec<String> = usage_path
        .iter()
        .map(|name| {
            name.as_str()
                .map(str::to_owned)
                .ok_or("usage_path names are strings")
        })
        .collect::<Result<_, _>>()?;
    let Some(node) = dispatcher.find_node(&names) else {
        return fault("usage_path names no node");
    };
    let source = SOURCE.to_owned();
    let smart: Vec<Value> = dispatcher
        .get_smart_usage(&*node, &source)
        .iter()
        .map(|(child, usage)| json!({"node": child.get_name(), "usage": usage}))
        .collect();
    Ok(json!({
        "all": dispatcher.get_all_usage(&*node, &source, false),
        "all_restricted": dispatcher.get_all_usage(&*node, &source, true),
        "smart": smart,
    }))
}

struct CollectingAmbiguities(Mutex<Vec<Value>>);

impl AmbiguityConsumer<String> for CollectingAmbiguities {
    fn ambiguous(
        &self,
        parent: &dyn CommandNode<String>,
        child: &dyn CommandNode<String>,
        sibling: &dyn CommandNode<String>,
        inputs: &[String],
    ) {
        self.0.lock().expect("ambiguities").push(json!({
            "parent": parent.get_name(),
            "child": child.get_name(),
            "sibling": sibling.get_name(),
            "inputs": inputs,
        }));
    }
}

fn ambiguity_facet(built: &Built) -> Value {
    let consumer = CollectingAmbiguities(Mutex::new(Vec::new()));
    built.dispatcher.find_ambiguities(&consumer);
    Value::Array(consumer.0.into_inner().expect("ambiguities"))
}

fn dispatch(request: &Map<String, Value>) -> Result<Map<String, Value>, Fault> {
    let mut built = build(get_object(request, "spec")?)?;
    let input = get_str(request, "input")?;
    let mut result = Map::new();
    for want in get_array(request, "want")? {
        match want.as_str() {
            Some("run") => {
                result.insert("run".into(), run_facet(&mut built, input)?);
            }
            Some("suggestions") => {
                let cursor = get(request, "cursor")?
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .ok_or("`cursor` must be an int")?;
                result.insert("suggestions".into(), suggest_facet(&built, input, cursor));
            }
            Some("usage") => {
                result.insert(
                    "usage".into(),
                    usage_facet(&built, get_array(request, "usage_path")?)?,
                );
            }
            Some("ambiguities") => {
                result.insert("ambiguities".into(), ambiguity_facet(&built));
            }
            _ => return fault("unknown facet"),
        }
    }
    // The declared result-key order of the Java op.
    let mut ordered = Map::new();
    for key in ["run", "suggestions", "usage", "ambiguities"] {
        if let Some(value) = result.shift_remove(key) {
            ordered.insert(key.into(), value);
        }
    }
    Ok(ordered)
}

/// `brigadier.dispatch`.
pub fn brigadier_dispatch(request: &Map<String, Value>) -> Outcome {
    match dispatch(request) {
        Ok(result) => Outcome::Accepted(result),
        Err(message) => Outcome::Unavailable(message),
    }
}

/// The scoreboard subject of one input: the facet group it exercises.
pub fn subject_of(request: &Map<String, Value>) -> Option<String> {
    let spec = request.get("spec")?.get("name")?.as_str()?;
    let want = request.get("want")?.as_array()?;
    let facets: Vec<&str> = want.iter().filter_map(Value::as_str).collect();
    Some(format!("{spec}/{}", facets.join("+")))
}

#[cfg(test)]
mod tests {
    use super::super::{CorpusEntry, OpFixture, compare, load_fixtures, op_source, ops_dir};
    use super::*;

    fn fixture() -> OpFixture {
        load_fixtures(&ops_dir(), &op_source())
            .expect("committed recorded ops load and close")
            .into_iter()
            .find(|f| f.op == "brigadier.dispatch")
            .expect("brigadier.dispatch is recorded")
    }

    fn wants(entry: &CorpusEntry) -> Vec<&str> {
        entry.request["want"]
            .as_array()
            .expect("want is an array")
            .iter()
            .filter_map(Value::as_str)
            .collect()
    }

    fn input(entry: &CorpusEntry) -> &str {
        entry.request["input"].as_str().expect("input is a string")
    }

    /// The corpus is large, covers every facet and every spec, and holds the
    /// invalid input classes of the task: out-of-range numbers, unclosed quotes,
    /// escapes, trailing whitespace, unknown literals, empty input and partial
    /// input for suggestions.
    #[test]
    fn corpus_covers_every_facet_spec_and_invalid_class() {
        let fixture = fixture();
        assert!(
            fixture.corpus.len() >= 500,
            "{} inputs",
            fixture.corpus.len()
        );
        for facet in ["run", "suggestions", "usage", "ambiguities"] {
            let n = fixture
                .corpus
                .iter()
                .filter(|e| wants(e).contains(&facet))
                .count();
            assert!(n >= 5, "facet {facet}: {n} inputs");
        }
        let specs: std::collections::BTreeSet<&str> = fixture
            .corpus
            .iter()
            .map(|e| e.request["spec"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            specs.into_iter().collect::<Vec<_>>(),
            ["ambiguous", "basic", "forks", "suggest", "usage", "vanilla"]
        );
        let run_inputs: Vec<&str> = fixture
            .corpus
            .iter()
            .filter(|e| wants(e).contains(&"run"))
            .map(input)
            .collect();
        let has = |what: &str, test: &dyn Fn(&str) -> bool| {
            assert!(run_inputs.iter().any(|i| test(i)), "no {what} input");
        };
        has("empty", &|i| i.is_empty());
        has("unclosed double quote", &|i| i.contains("\"unclosed"));
        has("unclosed single quote", &|i| i.contains("'unclosed"));
        has("escape", &|i| i.contains("\\\""));
        has("invalid escape", &|i| i.contains("\\n"));
        has("trailing whitespace", &|i| i.ends_with(' '));
        has("unknown literal", &|i| i.starts_with("unknown"));
        has("int out of range", &|i| i.contains("2147483648"));
        has("long out of range", &|i| i.contains("9223372036854775808"));
        has("below a bound", &|i| i.contains("intb -6"));
        has("float overflow", &|i| {
            i.contains("1000000000000000000000000000000000000000")
        });
        let partial = fixture
            .corpus
            .iter()
            .filter(|e| wants(e).contains(&"suggestions"))
            .filter(|e| {
                let cursor = e.request["cursor"].as_i64().unwrap();
                cursor < input(e).encode_utf16().count() as i64
            })
            .count();
        assert!(partial >= 50, "{partial} mid-input suggestion cursors");
        // Paper's recorded answers hold the exception classes the corpus is
        // meant to reach.
        let mut types = std::collections::BTreeSet::new();
        for (_, outcome) in &fixture.recorded {
            if let Outcome::Accepted(result) = outcome {
                let text = Value::Object(result.clone()).to_string();
                for t in [
                    "dispatcherUnknownCommand",
                    "dispatcherUnknownArgument",
                    "dispatcherExpectedArgumentSeparator",
                    "readerExpectedInt",
                    "readerInvalidInt",
                    "integerTooLow",
                    "integerTooHigh",
                    "longTooHigh",
                    "floatTooHigh",
                    "doubleTooLow",
                    "readerInvalidBool",
                    "readerExpectedEndOfQuote",
                    "readerInvalidEscape",
                ] {
                    if text.contains(&format!("\"type\":\"{t}\"")) {
                        types.insert(t);
                    }
                }
            }
        }
        assert_eq!(types.len(), 13, "exception classes reached: {types:?}");
    }

    /// The tamper negatives of the nested comparator: one leaf of each facet's
    /// recorded answer is corrupted, and the comparator must name the input and
    /// the exact field path. (The generic per-op negative only corrupts the first
    /// top-level key.)
    #[test]
    fn nested_tampers_are_detected_and_named_per_facet() {
        let fixture = fixture();
        let exclusions = compare::load_exclusions(&ops_dir().join("exclusions.tsv")).unwrap();
        let cases: [(&str, &str, &str); 5] = [
            ("run", "/run/execute/result", "result.run.execute.result"),
            ("run", "/run/parse/cursor", "result.run.parse.cursor"),
            (
                "suggestions",
                "/suggestions/list/0/text",
                "result.suggestions.list[0].text",
            ),
            ("usage", "/usage/all/0", "result.usage.all[0]"),
            (
                "ambiguities",
                "/ambiguities/0/inputs/0",
                "result.ambiguities[0].inputs[0]",
            ),
        ];
        for (facet, pointer, expected_path) in cases {
            let (entry, (id, recorded)) = fixture
                .corpus
                .iter()
                .zip(&fixture.recorded)
                .find(|(entry, (id, outcome))| {
                    !compare::input_excluded("brigadier.dispatch", id, &exclusions)
                        && wants(entry).contains(&facet)
                        && matches!(outcome, Outcome::Accepted(r)
                            if Value::Object(r.clone()).pointer(pointer).is_some())
                })
                .unwrap_or_else(|| panic!("no recorded {facet} answer has {pointer}"));
            let actual = brigadier_dispatch(&entry.request);
            assert_eq!(
                &actual, recorded,
                "{id}: Rivet matches Paper before tampering"
            );
            let Outcome::Accepted(result) = recorded else {
                unreachable!()
            };
            let mut value = Value::Object(result.clone());
            let leaf = value.pointer_mut(pointer).unwrap();
            *leaf = match &*leaf {
                Value::Number(n) => json!(n.as_i64().unwrap() + 1),
                Value::String(s) => json!(format!("{s}x")),
                other => panic!("unexpected leaf {other}"),
            };
            let Value::Object(tampered) = value else {
                unreachable!()
            };
            let comparison = compare::compare(
                "brigadier.dispatch",
                &[(id.clone(), Outcome::Accepted(tampered))],
                &[(id.clone(), actual)],
                &[],
            );
            assert_eq!(comparison.mismatched_ids, vec![id.clone()], "{facet}");
            let first = comparison.first.expect("a divergence is reported");
            assert_eq!(first.id, *id);
            assert!(
                first.fields.iter().any(|f| f.path == expected_path),
                "{facet}: {:?} does not name {expected_path}",
                first.fields
            );
        }
    }

    /// A hand-edited recorded answer no longer matches the manifest pin, and the
    /// loader names the file.
    #[test]
    fn edited_recorded_golden_is_rejected() {
        let scratch = tempfile::tempdir().unwrap();
        rivet_harness_common::negative::copy_dir_recursive(&ops_dir(), scratch.path()).unwrap();
        let target = scratch.path().join("brigadier.dispatch.jsonl");
        let text = std::fs::read_to_string(&target).unwrap();
        let edited = text.replacen("\"cursor\":0", "\"cursor\":1", 1);
        assert_ne!(edited, text);
        std::fs::write(&target, edited).unwrap();
        match load_fixtures(scratch.path(), &op_source()) {
            Err(super::super::LoadError::Invalid(m)) => {
                assert!(m.contains("brigadier.dispatch.jsonl"), "{m}")
            }
            other => panic!(
                "expected an invalid-fixture error, got {:?}",
                other.map(|f| f.len())
            ),
        }
    }

    /// A request the op cannot interpret is `unavailable` evidence, never a verdict.
    #[test]
    fn malformed_requests_are_unavailable_not_rejected() {
        let request = |text: &str| -> Map<String, Value> {
            serde_json::from_str(text).expect("test request is JSON")
        };
        for text in [
            r#"{"input":"x","want":["run"]}"#,
            r#"{"spec":{"commands":[{"literal":"a"}]},"input":"x","want":["nope"]}"#,
            r#"{"spec":{"commands":[{"literal":"a","redirect":{"to":"missing","kind":"plain"}}]},"input":"x","want":["run"]}"#,
            r#"{"spec":{"commands":[{"argument":"a","type":{"kind":"integer"}}]},"input":"x","want":["run"]}"#,
            r#"{"spec":{"commands":[{"literal":"a"}]},"input":"x","want":["usage"],"usage_path":["b"]}"#,
            r#"{"spec":{"commands":[{"literal":"a"}]},"input":"x","want":["suggestions"]}"#,
        ] {
            assert!(
                matches!(brigadier_dispatch(&request(text)), Outcome::Unavailable(_)),
                "{text}"
            );
        }
    }
}
