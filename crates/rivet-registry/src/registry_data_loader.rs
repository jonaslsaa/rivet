//! Port of `net.minecraft.resources.RegistryDataLoader` + the load-task units
//! (MC 26.2) — the pack-loading driver built on the #126 holder codecs.
//!
//! PROVENANCE: `RegistryDataLoader.java` (335 lines), `RegistryLoadTask.java`
//! (148 lines), `RegistryValidator.java` (25 lines),
//! `ResourceManagerRegistryLoadTask.java` (78 lines),
//! `NetworkRegistryLoadTask.java` (88 lines), and `FileToIdConverter.java`
//! (39 lines) — all leaves of the `mc.resources` manifest unit.
//!
//! This is the **RegistryDataLoader + load-task half** of issue #126. The
//! holder/codec foundations it consumes (`Holder`/`HolderSet`/`HolderLookup`,
//! `RegistryOps`, `RegistryFileCodec`/`RegistryFixedCodec`/`HolderSetCodec`)
//! are already merged (PR #165); the protocol stream codecs are merged (PR
//! #193). What remains — and what this module delivers — is the driver that
//! turns datapack JSON resources into a frozen `RegistryAccess`, mirroring
//! Java's `ResourceManagerRegistryLoadTask` and the `RegistryDataLoader.load`
//! pipeline:
//!
//! ```java
//! // RegistryDataLoader.java — the load pipeline
//! List<RegistryLoadTask<?>> loadTasks = registriesToLoad.stream()
//!     .map(r -> loaderFactory.create(r, loadingErrors)).toList();
//! RegistryOps.RegistryInfoLookup context = createContext(contextRegistries, loadTasks);
//! // ... loadTasks[i].load(context, executor) ...
//! // ... freezeRegistry(loadingErrors), validateRegistry(loadingErrors) ...
//! return new RegistryAccess.ImmutableRegistryAccess(registries).freeze();
//! ```
//!
//! Binding-model deviations (documented, PORTING.md drift checklist):
//! - **Ops-parametric codecs.** Java's `Codec<T>` is ops-polymorphic, so a
//!   `RegistryDataLoader.RegistryData<T>` carries one codec used under any
//!   `RegistryOps`, and the load pipeline is written once. The Rust
//!   `Codec<A, Ops>` pins the ops at the type, so an element codec used under
//!   the loader's `RegistryOps` must be built for exactly that ops
//!   (`RegistryOps<serde_json::Value, JsonOps>` — the `JsonRegistryOps` alias
//!   below, the same ops the `registry_file_codec` tests drive the codecs
//!   through). The `RegistryData<T>` record carries that ops-pinned codec.
//! - **Only the file/JSON path is ported.** `ResourceManagerRegistryLoadTask`
//!   decodes datapack JSON element resources. `NetworkRegistryLoadTask`
//!   (client-side registry sync) decodes NBT (`NbtOps`) and needs the
//!   `RegistrySynchronization`/`TagNetworkSerialization` wire values — but
//!   rivet-nbt depends on rivet-registry (`blocks` feature), so referencing
//!   `rivet_nbt::Tag`/`NbtOps` here would create a crate cycle, and the network
//!   sync types live in `rivet-protocol` (which rivet-registry must not depend
//!   on). RivetTodo(#126): `NetworkRegistryLoadTask` + the
//!   `NetworkedRegistryData` record + `TagLoader.loadTagsFromNetwork` are
//!   deferred with the `net.minecraft.tags` / network-sync units. The
//!   `loadFromNetwork`/network error surfaces are therefore absent rather than
//!   stubbed.
//! - **No `ResourceManager`.** Java's `ResourceManagerRegistryLoadTask` lists
//!   resources through `ResourceManager` (`net.minecraft.server.packs` — a
//!   different crate/unit not yet ported). This module keeps the loader
//!   testable and dependency-clean by taking the *already-listed* resources as
//!   a `HashMap<Identifier, String>` (id → JSON contents), which is exactly
//!   what `FileToIdConverter::listMatchingResources` returns. A future
//!   `ResourceManager` port calls that seam. `sourcePackId()` has no carrier,
//!   so the `"Failed to parse <id> from pack <pack>"` message renders
//!   `"<unknown>"` for the pack slot.
//! - **No async executor.** Java's load pipeline runs on a
//!   `CompletableFuture`/`Executor` (and `ParallelMapTransform` for the element
//!   decodes). The port keeps the same *synchronous control flow* — context
//!   first, then load all, then freeze all, then validate all — and the same
//!   observable error surfaces, without the concurrency layer. The caller owns
//!   threading.
//! - **Tags are caller-supplied.** Java's `ResourceManagerRegistryLoadTask`
//!   loads tags through `TagLoader` (a JSON `{ "replace", "values" }` loader in
//!   `net.minecraft.tags`, not yet ported) after the elements register, then
//!   `bindTags`. This module keeps the `bind_tags` phase but takes the
//!   pre-resolved `(TagKey<T>, Vec<HolderId>)` bindings (the exact
//!   `RegistryBuilder::bind_tags` shape) from the caller; a future `TagLoader`
//!   port supplies them.
//! - **`KnownPack`/Paper reg-mod API are omitted.** The `RegistrationInfo`
//!   known-pack slot is an opaque `()` placeholder (see `registration_info.rs`);
//!   the Paper `PaperRegistryAccess`/`PaperRegistryListenerManager`/
//!   `Conversions` hooks are Paper plugin-API machinery out of scope for this
//!   foundation slice. Because the placeholder carrier can only ever be absent,
//!   every datapack element registers with
//!   `RegistrationInfo(None, Lifecycle.experimental())` — Java's
//!   `REGISTRATION_INFO_CACHE` maps an absent `KnownPack` to
//!   `orElse(Lifecycle.experimental())`. The task's initial `Lifecycle.stable()`
//!   accumulates the element lifecycles (`Lifecycle.add`: experimental wins),
//!   so a datapack-loaded registry freezes experimental, matching Paper.
//! - **Same-batch cross-references resolve as empty.** Java's
//!   `ConcurrentHolderGetter` creates placeholder holders for not-yet-registered
//!   keys (holder identity: repeated `get(key)` returns the same holder, bound
//!   at `register`). The Rust `Holder<T>` is a value (`(RegistryId, id)`) with
//!   no placeholder form, so during a registry's own load its elements resolve
//!   it as absent (`"Failed to get element ..."`), exactly like Java's decode
//!   phase does for a key with no placeholder. Elements referencing **context**
//!   registries (already-loaded layers) resolve normally. RivetTodo(#126): the
//!   placeholder-holder binding for same-batch cyclic references is deferred
//!   with the holder-model work.
//!
//! Error surfaces (Java `RegistryLoadTask`):
//! - `loadFromResource`: `"Failed to parse <id> from pack <pack>"` (pack is
//!   `"<unknown>"` here); the JSON parse error or the element-codec decode
//!   error is appended after a colon for diagnosability (Java chains it as the
//!   exception's cause instead).
//! - `findAndLoadFromResource`: `"Failed to find resource <resourceId> for
//!   element <id>"`.
//! - `freezeRegistry`: a failed freeze records the registry key in the error
//!   map (`"Failed to freeze <key>"`; Java's `MappedRegistry.freeze()` throws
//!   with its own message, so the text differs but the shape — one error keyed
//!   by the registry key — matches).
//! - `validateRegistry`: `RegistryValidator` errors, e.g. `"Registry must be
//!   non-empty: <id>"`.
//!
//! The driver does **not** hard-code the concrete registry lists
//! (`WORLDGEN_REGISTRIES` etc.) — those need the ~40 element codecs of the
//! worldgen/entity/data leaves, which are not yet ported. Callers pass their
//! own load tasks.

use crate::access::RegistryAccess;
use crate::builder::RegistryBuilder;
use crate::holder::HolderId;
use crate::identifier::Identifier;
use crate::registry::RegistryKey;
use crate::registry_ops::{RegistryInfo, RegistryInfoLookup, RegistryOps};
use crate::root::AnyBox;
use crate::{ResourceKey, TagKey};

use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::lifecycle::Lifecycle;

use std::collections::HashMap;
use std::sync::Arc;

/// The `RegistryOps` the loader's element codecs run under — a `RegistryOps`
/// over `JsonOps` carrying the load context (`RegistryInfoLookup`). Element
/// codecs for datapack-loaded registries are built for this ops (the same ops
/// the `registry_file_codec` tests drive the holder codecs through).
pub type JsonRegistryOps = RegistryOps<serde_json::Value, JsonOps>;

/// Erase an element `ResourceKey<T>` to `ResourceKey<()>` preserving both
/// identifiers (the error-map key Java stores is the full element key).
fn erase_key<T>(key: &ResourceKey<T>) -> ResourceKey<()> {
    let registry_key: RegistryKey<()> = ResourceKey::create_registry_key(key.registry().clone());
    ResourceKey::create(&registry_key, key.identifier().clone())
}

/// Erase a registry key to the wildcard registry-key form
/// `ResourceKey<Registry<()>>` — the key an access/`RegistryInfoLookup` stores
/// (Java's `ResourceKey<? extends Registry<?>>`). Used for `registry_key()`,
/// the context map, and `from_pairs`.
fn erase_registry_key<E>(key: &RegistryKey<E>) -> RegistryKey<()> {
    ResourceKey::create_registry_key(key.identifier().clone())
}

// ---------------------------------------------------------------------------
// FileToIdConverter
// ---------------------------------------------------------------------------

/// `net.minecraft.resources.FileToIdConverter` — the resource-id
/// `(prefix, extension)` mapping between a datapack file path and its element
/// identifier.
///
/// Java:
/// ```java
/// public record FileToIdConverter(String prefix, String extension) {
///     public static FileToIdConverter json(String prefix) { return new FileToIdConverter(prefix, ".json"); }
///     public static FileToIdConverter registry(ResourceKey<? extends Registry<?>> registry) {
///         return json(Registries.elementsDirPath(registry));
///     }
///     public Identifier idToFile(Identifier id) { return id.withPath(this.prefix + "/" + id.getPath() + this.extension); }
///     public Identifier fileToId(Identifier file) {
///         String path = file.getPath();
///         return file.withPath(path.substring(this.prefix.length() + 1, path.length() - this.extension.length()));
///     }
/// }
/// ```
///
/// `file_to_id` recreates Java's `substring` on the identifier's path and
/// panics on the same out-of-bounds (a hostile caller passing a file whose
/// path does not start with the prefix or end with the extension).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileToIdConverter {
    /// `FileToIdConverter.prefix`.
    pub prefix: String,
    /// `FileToIdConverter.extension`.
    pub extension: String,
}

impl FileToIdConverter {
    /// `FileToIdConverter.json(String)`.
    pub fn json(prefix: String) -> Self {
        FileToIdConverter {
            prefix,
            extension: ".json".to_string(),
        }
    }

    /// `FileToIdConverter.registry(ResourceKey<? extends Registry<?>>)` —
    /// `json(Registries.elementsDirPath(registry))` = the registry key's path.
    pub fn registry(registry_key: &RegistryKey<()>) -> Self {
        FileToIdConverter::json(registry_key.identifier().path().to_string())
    }

    /// `FileToIdConverter.idToFile(Identifier)` — `prefix + "/" + path + extension`.
    pub fn id_to_file(&self, id: &Identifier) -> Identifier {
        id.with_path(&format!("{}/{}{}", self.prefix, id.path(), self.extension))
    }

    /// `FileToIdConverter.fileToId(Identifier)` — the inverse of `id_to_file`
    /// (substring by prefix + extension).
    pub fn file_to_id(&self, file: &Identifier) -> Identifier {
        let path = file.path();
        let start = self.prefix.len() + 1;
        let end = path.len() - self.extension.len();
        file.with_path(&path[start..end])
    }

    /// `FileToIdConverter.extensionMatches(Identifier)`.
    pub fn extension_matches(&self, id: &Identifier) -> bool {
        id.path().ends_with(&self.extension)
    }

    /// `FileToIdConverter.listMatchingResources(ResourceManager)` — the seam a
    /// future `ResourceManager` port calls. `resources` is the already-listed
    /// `(Identifier, contents)` map (see module docs). Java's
    /// `manager.listResources(prefix, this::extensionMatches)` lists only
    /// resources under the prefix directory, so a file must both start with
    /// `prefix + "/"` AND end with the extension to match. The prefix filter is
    /// what keeps `file_to_id`'s substring from ever seeing an out-of-prefix
    /// path during a load (Java's `substring` throws on such input; the load
    /// path simply never hands it one).
    pub fn list_matching_resources<'a>(
        &self,
        resources: &'a HashMap<Identifier, String>,
    ) -> HashMap<Identifier, &'a String> {
        let prefix_path = format!("{}/", self.prefix);
        resources
            .iter()
            .filter(|(id, _)| id.path().starts_with(&prefix_path) && self.extension_matches(id))
            .map(|(id, contents)| (id.clone(), contents))
            .collect()
    }
}

// ---------------------------------------------------------------------------
// RegistryValidator
// ---------------------------------------------------------------------------

/// `net.minecraft.resources.RegistryValidator<T>` — the post-load validation
/// hook (`RegistryDataLoader.RegistryData.validator`).
///
/// Java:
/// ```java
/// public interface RegistryValidator<T> {
///     RegistryValidator<?> NONE = (var0, var1) -> {};
///     RegistryValidator<?> NON_EMPTY = (registry, loadingErrors) -> {
///         if (registry.size() == 0) {
///             loadingErrors.put(registry.key(), new IllegalStateException(
///                 "Registry must be non-empty: " + registry.key().identifier()));
///         }
///     };
/// }
/// ```
///
/// The validator writes into the shared error map by registry key (erased to
/// `ResourceKey<()>`).
pub trait RegistryValidator<T> {
    /// `RegistryValidator.validate(Registry<T>, Map<ResourceKey<?>, Exception>)`.
    fn validate(
        &self,
        registry: &crate::Registry<T>,
        loading_errors: &mut HashMap<ResourceKey<()>, String>,
    );
}

/// `RegistryValidator.NONE` — the no-op validator.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoOpValidator;

impl<T> RegistryValidator<T> for NoOpValidator {
    fn validate(
        &self,
        _registry: &crate::Registry<T>,
        _loading_errors: &mut HashMap<ResourceKey<()>, String>,
    ) {
    }
}

/// `RegistryValidator.NON_EMPTY` — `"Registry must be non-empty: <id>"`.
#[derive(Debug, Clone, Copy, Default)]
pub struct NonEmptyValidator;

impl<T> RegistryValidator<T> for NonEmptyValidator {
    fn validate(
        &self,
        registry: &crate::Registry<T>,
        loading_errors: &mut HashMap<ResourceKey<()>, String>,
    ) {
        if registry.size() == 0 {
            loading_errors.insert(
                erase_key(registry.key()),
                format!(
                    "Registry must be non-empty: {}",
                    registry.key().identifier()
                ),
            );
        }
    }
}

/// `RegistryValidator.none()` — the erased no-op (Java's static `NONE`).
pub fn validator_none<T>() -> Box<dyn RegistryValidator<T>> {
    Box::new(NoOpValidator)
}

/// `RegistryValidator.nonEmpty()` — the erased non-empty (Java's static
/// `NON_EMPTY`).
pub fn validator_non_empty<T>() -> Box<dyn RegistryValidator<T>> {
    Box::new(NonEmptyValidator)
}

// ---------------------------------------------------------------------------
// RegistryData
// ---------------------------------------------------------------------------

/// `RegistryDataLoader.RegistryData<T>` — `(key, elementCodec, validator)`.
///
/// Java's element codec is ops-polymorphic; the Rust `Codec<T, Ops>` pins the
/// ops, so the codec is built for the loader's `JsonRegistryOps` (the ops the
/// datapack JSON decode runs under). The `validator` slot mirrors Java's third
/// `RegistryData` constructor argument (`RegistryValidator.none()` default).
pub struct RegistryData<T> {
    /// `RegistryData.key` — `ResourceKey<? extends Registry<T>>`.
    pub key: RegistryKey<T>,
    /// `RegistryData.elementCodec`.
    pub element_codec: Arc<dyn Codec<T, JsonRegistryOps>>,
    /// `RegistryData.validator`.
    pub validator: Box<dyn RegistryValidator<T>>,
}

impl<T> RegistryData<T> {
    /// `RegistryData(ResourceKey, Codec<T>)` — `RegistryValidator.none()`.
    pub fn new(key: &RegistryKey<T>, element_codec: Arc<dyn Codec<T, JsonRegistryOps>>) -> Self {
        RegistryData {
            key: key.clone(),
            element_codec,
            validator: validator_none(),
        }
    }

    /// `RegistryData(ResourceKey, Codec<T>, RegistryValidator<T>)`.
    pub fn with_validator(
        key: &RegistryKey<T>,
        element_codec: Arc<dyn Codec<T, JsonRegistryOps>>,
        validator: Box<dyn RegistryValidator<T>>,
    ) -> Self {
        RegistryData {
            key: key.clone(),
            element_codec,
            validator,
        }
    }
}

// ---------------------------------------------------------------------------
// PendingRegistration — the load helpers
// ---------------------------------------------------------------------------

/// A pending element registration — `(key, Either<T, error>, registrationInfo)`.
///
/// The value is a `Result<T, String>` (Java's `Either<T, Exception>`; the
/// `RegistrationInfo` known-pack slot is the opaque `()` placeholder).
struct PendingRegistration<T> {
    key: ResourceKey<T>,
    value: Result<T, String>,
    registration_info: crate::RegistrationInfo,
}

impl<T> PendingRegistration<T> {
    /// `RegistryLoadTask.PendingRegistration.loadFromResource` — parse the
    /// datapack JSON `contents` into the ops' element type, then decode it with
    /// the element codec under `ops`.
    ///
    /// Java:
    /// ```java
    /// static <T> Either<T, Exception> loadFromResource(Decoder<T> elementDecoder,
    ///         RegistryOps<JsonElement> ops, ResourceKey<T> elementKey, Resource thunk) {
    ///     try (Reader reader = thunk.openAsReader()) {
    ///         JsonElement json = StrictJsonParser.parse(reader);
    ///         return Either.left(elementDecoder.parse(ops, json).getOrThrow());
    ///     } catch (Exception e) {
    ///         return Either.right(new IllegalStateException(String.format(
    ///             "Failed to parse %s from pack %s", elementKey.identifier(), thunk.sourcePackId()), e));
    ///     }
    /// }
    /// ```
    ///
    /// The JSON parse error / decode error is appended after a colon (Java
    /// chains it as the cause); the pack slot renders `"<unknown>"` (no
    /// `ResourceManager`/`KnownPack` carrier — see module docs).
    fn load_from_resource(
        element_codec: &dyn Codec<T, JsonRegistryOps>,
        ops: &JsonRegistryOps,
        element_key: &ResourceKey<T>,
        contents: &str,
    ) -> Result<T, String> {
        let json: serde_json::Value = match serde_json::from_str(contents) {
            Ok(value) => value,
            Err(e) => {
                return Err(format!(
                    "Failed to parse {} from pack <unknown>: {}",
                    element_key.identifier(),
                    e
                ));
            }
        };
        let decoded = element_codec.decode(ops, &json);
        if let Some(error) = decoded.error_ref() {
            return Err(format!(
                "Failed to parse {} from pack <unknown>: {}",
                element_key.identifier(),
                error.message()
            ));
        }
        match decoded.result_or_partial_silent() {
            Some((value, _)) => Ok(value),
            // An error with no partial is caught by `error_ref` above; this arm
            // keeps the decode's partial from being silently dropped.
            None => Err(format!(
                "Failed to parse {} from pack <unknown>",
                element_key.identifier()
            )),
        }
    }

    /// `RegistryLoadTask.PendingRegistration.findAndLoadFromResource` — resolve
    /// `converter.idToFile(id)` against a resource provider, then
    /// `loadFromResource`.
    ///
    /// Java: `"Failed to find resource <resourceId> for element <id>"` when the
    /// file is absent.
    ///
    /// The driver's parallel path (`load`) maps over the already-listed
    /// resources and calls `load_from_resource` directly, so this singular
    /// find-then-load form is exercised only by the hostile test below
    /// (`find_and_load_resolves_a_missing_resource_as_an_error`) — the seam a
    /// single-resource caller (e.g. a future registry-sync task) uses.
    #[allow(dead_code)] // exercised by the cfg(test) module only
    fn find_and_load_from_resource(
        element_codec: &dyn Codec<T, JsonRegistryOps>,
        ops: &JsonRegistryOps,
        element_key: &ResourceKey<T>,
        converter: &FileToIdConverter,
        resource_provider: &HashMap<Identifier, String>,
    ) -> Result<T, String> {
        let resource_id = converter.id_to_file(element_key.identifier());
        match resource_provider.get(&resource_id) {
            Some(contents) => Self::load_from_resource(element_codec, ops, element_key, contents),
            None => Err(format!(
                "Failed to find resource {} for element {}",
                resource_id,
                element_key.identifier()
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// DynLoadTask — the erased load-task surface
// ---------------------------------------------------------------------------

/// The erased load-task surface — Java's `RegistryLoadTask<?>` heterogeneous
/// list. The driver holds `Vec<Box<dyn DynLoadTask>>` and drives the shared
/// lifecycle (context info, load, freeze, validate); the concrete
/// `ResourceManagerRegistryLoadTask` carries the element type.
///
/// Java's `registryWriteLock` (`synchronized` blocks) becomes ownership: the
/// concrete task owns its `RegistryBuilder<T>` (the exclusive pre-freeze
/// registry), and `load`/`freeze_registry`/`validate_registry` run in sequence
/// on the tick thread. The driver passes the shared error map by `&mut` through
/// each phase (Java shares one `Map<ResourceKey<?>, Exception>` across all
/// tasks).
pub trait DynLoadTask {
    /// `RegistryLoadTask.registryKey()` — erased to the wildcard registry-key
    /// form `ResourceKey<Registry<()>>` (what the context map and the built
    /// access store).
    fn registry_key(&self) -> RegistryKey<()>;

    /// `RegistryLoadTask.createRegistryInfo()` — the `RegistryInfo` the load
    /// context resolves this registry with (`owner`, `elementsLifecycle`, and
    /// the getter's owning access).
    fn create_registry_info(&self, context_access: &RegistryAccess) -> RegistryInfo<()>;

    /// `RegistryLoadTask.load(RegistryOps, Executor)` — the concrete element/tag
    /// load (synchronous here; the caller owns threading). The driver passes the
    /// shared `RegistryOps` (over the load context) so the per-element decode
    /// runs under the same ops across all tasks, like Java's shared context.
    fn load(
        &mut self,
        ops: &JsonRegistryOps,
        loading_errors: &mut HashMap<ResourceKey<()>, String>,
    );

    /// `RegistryLoadTask.freezeRegistry(Map)` — freeze the registry; `false`
    /// (with the registry key recorded) on a freeze failure.
    fn freeze_registry(&mut self, loading_errors: &mut HashMap<ResourceKey<()>, String>) -> bool;

    /// `RegistryLoadTask.validateRegistry(Map)` — run the data's validator;
    /// `Some(erased)` when the registry validates, else `None` with the errors
    /// merged into the shared map.
    fn validate_registry(
        &mut self,
        loading_errors: &mut HashMap<ResourceKey<()>, String>,
    ) -> Option<AnyBox>;
}

// ---------------------------------------------------------------------------
// ResourceManagerRegistryLoadTask
// ---------------------------------------------------------------------------

/// `net.minecraft.resources.ResourceManagerRegistryLoadTask<T>` — the datapack
/// JSON load task.
///
/// Java:
/// ```java
/// public class ResourceManagerRegistryLoadTask<T> extends RegistryLoadTask<T> {
///     public CompletableFuture<?> load(RegistryOps.RegistryInfoLookup context, Executor executor) {
///         FileToIdConverter lister = FileToIdConverter.registry(this.registryKey());
///         return CompletableFuture.supplyAsync(() -> lister.listMatchingResources(this.resourceManager), executor)
///             .thenCompose(registryResources -> {
///                 RegistryOps<JsonElement> ops = RegistryOps.create(JsonOps.INSTANCE, context);
///                 return ParallelMapTransform.schedule(registryResources, (resourceId, thunk) ->
///                     new PendingRegistration<>(ResourceKey.create(this.registryKey(), lister.fileToId(resourceId)),
///                         PendingRegistration.loadFromResource(this.data.elementCodec(), ops, elementKey, thunk),
///                         REGISTRATION_INFO_CACHE.apply(thunk.knownPackInfo())), executor);
///             })
///             .thenAcceptAsync(loadedEntries -> {
///                 this.registerElements(loadedEntries.entrySet().stream()
///                     .sorted(Entry.comparingByKey()).map(Entry::getValue), conversions);
///                 // ... Paper reg-mod listeners ...
///                 Map<TagKey<T>, List<Holder<T>>> pendingTags = TagLoader.loadTagsForRegistry(...);
///                 this.registerTags(pendingTags);
///             }, executor);
///     }
/// }
/// ```
///
/// The `ResourceManager` listing and the `TagLoader`/Paper hooks are deferred
/// (see module docs); the task takes the already-listed resources and the
/// pre-resolved tag bindings. Registration order is the **resource-id sorted
/// order** (Java's `sorted(Entry.comparingByKey())`), which fixes the holder
/// ids (element id == insertion index).
pub struct ResourceManagerRegistryLoadTask<'a, T> {
    /// `RegistryLoadTask.data`.
    data: RegistryData<T>,
    /// The pre-freeze registry (`MappedRegistry(key, lifecycle)`), held in an
    /// `Option` so `freeze_registry` can move it out (Java's `registry` field
    /// is consumed by `freeze()`; the `Option` is `Some` from construction
    /// through `load` and `None` after freeze).
    builder: Option<RegistryBuilder<T>>,
    /// `RegistryLoadTask` constructor lifecycle — `Lifecycle.stable()` from the
    /// driver; the element registration lifecycles accumulate into the frozen
    /// registry's lifecycle.
    lifecycle: Lifecycle,
    /// The already-listed datapack resources (`FileToIdConverter.registry(key)`
    /// is built internally).
    resources: &'a HashMap<Identifier, String>,
    /// The pre-resolved tag bindings (Java's `TagLoader.loadTagsForRegistry`
    /// result).
    tag_bindings: Vec<(TagKey<T>, Vec<HolderId>)>,
    /// The frozen registry, set by `freeze_registry` and consumed by
    /// `validate_registry`.
    frozen: Option<crate::Registry<T>>,
}

impl<'a, T: Send + Sync + 'static> ResourceManagerRegistryLoadTask<'a, T> {
    /// Build the task from a `RegistryData<T>` and the already-listed
    /// resources. The task owns the `RegistryData` (Java's `this.data = data`),
    /// moving the validator in.
    pub fn from_data(
        data: RegistryData<T>,
        lifecycle: Lifecycle,
        resources: &'a HashMap<Identifier, String>,
    ) -> Self {
        ResourceManagerRegistryLoadTask {
            builder: Some(RegistryBuilder::new(&data.key)),
            lifecycle,
            data,
            resources,
            tag_bindings: Vec::new(),
            frozen: None,
        }
    }

    /// The default driver lifecycle (`Lifecycle.stable()`, as Java's
    /// `RegistryDataLoader.load` passes to the load-task constructor).
    pub fn with_stable_lifecycle(
        data: RegistryData<T>,
        resources: &'a HashMap<Identifier, String>,
    ) -> Self {
        Self::from_data(data, Lifecycle::Stable, resources)
    }

    /// Supply the tag bindings (`TagLoader.loadTagsForRegistry` result) bound
    /// before freeze, after the elements register.
    pub fn with_tag_bindings(mut self, tag_bindings: Vec<(TagKey<T>, Vec<HolderId>)>) -> Self {
        self.tag_bindings = tag_bindings;
        self
    }

    /// `RegistryLoadTask.registerElements(Stream<PendingRegistration>, ...)` —
    /// register each successfully-decoded element and record decode failures in
    /// the shared error map keyed by the element key (Java's
    /// `loadingErrors.put(element.key, error)`).
    fn register_elements(
        &mut self,
        elements: Vec<PendingRegistration<T>>,
        loading_errors: &mut HashMap<ResourceKey<()>, String>,
    ) {
        let builder = self
            .builder
            .as_mut()
            .expect("register_elements runs before freeze");
        for element in elements {
            match element.value {
                Ok(value) => {
                    builder.register(&element.key, Arc::new(value), element.registration_info);
                }
                Err(message) => {
                    loading_errors.insert(erase_key(&element.key), message);
                }
            }
        }
    }

    /// `RegistryLoadTask.registerTags(Map)` — `bindTags` on the builder. The
    /// bindings are moved out (the task is linearly consumed; they are never
    /// needed after the load phase).
    fn register_tags(&mut self) {
        let builder = self
            .builder
            .as_mut()
            .expect("register_tags runs before freeze");
        builder.bind_tags(std::mem::take(&mut self.tag_bindings));
    }
}

impl<T: Send + Sync + 'static> DynLoadTask for ResourceManagerRegistryLoadTask<'_, T> {
    fn registry_key(&self) -> RegistryKey<()> {
        erase_registry_key(&self.data.key)
    }

    fn create_registry_info(&self, context_access: &RegistryAccess) -> RegistryInfo<()> {
        // Java `new RegistryInfo<>(registry, concurrentRegistrationGetter,
        // registry.registryLifecycle())` — the owner id and lifecycle come from
        // the pre-freeze registry; the getter's owning access is the context
        // access, which during this registry's own load does NOT contain it
        // (same-batch references resolve as empty — see module docs).
        let builder = self
            .builder
            .as_ref()
            .expect("create_registry_info runs before freeze");
        RegistryInfo::new(
            self.lifecycle,
            builder.registry_id(),
            context_access.clone(),
        )
    }

    fn load(
        &mut self,
        ops: &JsonRegistryOps,
        loading_errors: &mut HashMap<ResourceKey<()>, String>,
    ) {
        let converter = FileToIdConverter::registry(&self.registry_key());
        // `lister.listMatchingResources` then `sorted(Entry.comparingByKey())`:
        // only prefix + extension-matching resources, registered in resource-id
        // order (which fixes the holder ids). Identifier `Ord` is
        // path-then-namespace, matching Java's `Identifier.compareTo`.
        let mut resource_ids: Vec<Identifier> = converter
            .list_matching_resources(self.resources)
            .keys()
            .cloned()
            .collect();
        resource_ids.sort();

        let mut registrations: Vec<PendingRegistration<T>> = Vec::new();
        for resource_id in resource_ids {
            let element_key =
                ResourceKey::create(&self.data.key, converter.file_to_id(&resource_id));
            let value = PendingRegistration::<T>::load_from_resource(
                self.data.element_codec.as_ref(),
                ops,
                &element_key,
                &self.resources[&resource_id],
            );
            registrations.push(PendingRegistration {
                key: element_key,
                value,
                // `REGISTRATION_INFO_CACHE.apply(thunk.knownPackInfo())` with an
                // absent known-pack: `RegistrationInfo(None, experimental())`
                // (the `()` placeholder carrier cannot be present).
                registration_info: crate::RegistrationInfo::new(None, Lifecycle::Experimental),
            });
        }

        self.register_elements(registrations, loading_errors);
        self.register_tags();
    }

    fn freeze_registry(&mut self, loading_errors: &mut HashMap<ResourceKey<()>, String>) -> bool {
        // Java `loadingErrors.put(registry.key(), e)` — the error-map key is the
        // registry key in the wildcard element form `(root, registry-id)`.
        let error_key = erase_key(&self.data.key);
        // Java: `try { registry.freeze(); } catch (Exception e) {
        // loadingErrors.put(registry.key(), e); return false; }`. The builder's
        // `freeze()` panics (unbound values / leftover intrusive holders); the
        // load task always registers real values, so this is the exceptional
        // path, mirrored with catch_unwind. A panic consumes the builder (it
        // was moved into the closure), so the `Option` is taken first.
        let builder = match self.builder.take() {
            Some(builder) => builder,
            None => return false, // already frozen (load → freeze is a linear phase)
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.freeze())) {
            Ok(registry) => {
                self.frozen = Some(registry);
                true
            }
            Err(_) => {
                loading_errors.insert(error_key, format!("Failed to freeze {}", self.data.key));
                false
            }
        }
    }

    fn validate_registry(
        &mut self,
        loading_errors: &mut HashMap<ResourceKey<()>, String>,
    ) -> Option<AnyBox> {
        let frozen = self.frozen.as_ref()?;
        // Java validates into a LOCAL map, then merges into the shared one — so
        // this registry's validation errors do not leak into the next
        // registry's emptiness check.
        let mut registry_errors: HashMap<ResourceKey<()>, String> = HashMap::new();
        self.data.validator.validate(frozen, &mut registry_errors);
        if registry_errors.is_empty() {
            self.frozen
                .take()
                .map(|registry| Box::new(registry) as AnyBox)
        } else {
            loading_errors.extend(registry_errors);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// RegistryDataLoader — the load driver
// ---------------------------------------------------------------------------

/// `RegistryDataLoader.createContext(List<RegistryLookup>, List<RegistryLoadTask>)`
/// — the `RegistryOps.RegistryInfoLookup` that resolves both the context
/// registries (already-loaded layers) and the empty registries about to be
/// filled by the load tasks.
///
/// Java builds a `HashMap<ResourceKey, RegistryInfo>` from the context lookups
/// (`createInfoForContextRegistry`) and the load tasks (`createRegistryInfo`),
/// then narrows `lookup(key)` into it.
fn create_context<'a>(
    context_registries: &RegistryAccess,
    new_registries: &[Box<dyn DynLoadTask + 'a>],
) -> Box<dyn RegistryInfoLookup> {
    let mut by_key: HashMap<RegistryKey<()>, RegistryInfo<()>> = HashMap::new();
    for registry_key in context_registries.list_registry_keys() {
        // `createInfoForContextRegistry`: `new RegistryInfo<>(lookup, lookup,
        // lookup.registryLifecycle())`.
        if let Some(registry) = context_registries.lookup_erased(&registry_key) {
            by_key.insert(
                registry_key.clone(),
                RegistryInfo::new(
                    registry.registry_lifecycle(),
                    registry.registry_id(),
                    context_registries.clone(),
                ),
            );
        }
    }
    for task in new_registries {
        by_key.insert(
            task.registry_key(),
            task.create_registry_info(context_registries),
        );
    }
    Box::new(MapLookup { by_key })
}

/// The concrete `RegistryInfoLookup` over the built key → info map (Java's
/// `Optional.ofNullable(result.get(key))`).
#[derive(Debug)]
struct MapLookup {
    by_key: HashMap<RegistryKey<()>, RegistryInfo<()>>,
}

impl RegistryInfoLookup for MapLookup {
    fn lookup_erased(&self, registry_key: &RegistryKey<()>) -> Option<RegistryInfo<()>> {
        self.by_key.get(registry_key).cloned()
    }

    fn clone_box(&self) -> Box<dyn RegistryInfoLookup> {
        Box::new(MapLookup {
            by_key: self.by_key.clone(),
        })
    }
}

/// The shared loading-errors map — `Map<ResourceKey<?>, Exception>` keyed by
/// element or registry key (Java's `loadingErrors`), values rendered as the
/// message strings.
pub type RegistryLoadErrors = HashMap<ResourceKey<()>, String>;

/// `RegistryDataLoader.load(ResourceManager, contextRegistries, registriesToLoad,
/// Executor)` — the load pipeline.
///
/// Synchronous (the caller owns threading) and taking the already-listed
/// resources via the concrete load tasks:
/// ```java
/// // control flow
/// createContext(contextRegistries, loadTasks);        // context info first
/// loadTasks.forEach(t -> t.load(context, executor));  // load all
/// // freeze all, then validate all (Java checks errors between the phases)
/// return new RegistryAccess.ImmutableRegistryAccess(registries).freeze();
/// ```
///
/// Returns `Err(RegistryLoadErrors)` when any phase recorded errors — the
/// synchronous form of Java's `throw logErrors(loadingErrors)` (the reported
/// exception wrapping the errors map).
pub fn load<'a>(
    context_registries: &RegistryAccess,
    mut registries_to_load: Vec<Box<dyn DynLoadTask + 'a>>,
) -> Result<RegistryAccess, RegistryLoadErrors> {
    let mut loading_errors: HashMap<ResourceKey<()>, String> = HashMap::new();
    let context = create_context(context_registries, &registries_to_load);
    // One ops over the load context shared across all tasks (Java passes the
    // same `RegistryOps.RegistryInfoLookup context` to every task's load).
    let ops = RegistryOps::create(&JsonOps::INSTANCE, context);

    for task in &mut registries_to_load {
        task.load(&ops, &mut loading_errors);
    }

    // `frozenRegistries = loadTasks.stream().filter(t -> t.freezeRegistry(errors))`.
    // Paper runs the freeze phase before checking the shared error map, so a
    // decode failure in one registry still surfaces a freeze failure in another
    // alongside it (no early return here).
    let mut to_validate: Vec<&mut Box<dyn DynLoadTask + 'a>> = Vec::new();
    for task in &mut registries_to_load {
        if task.freeze_registry(&mut loading_errors) {
            to_validate.push(task);
        }
    }
    if !loading_errors.is_empty() {
        return Err(loading_errors);
    }

    // `frozenRegistries.stream().flatMap(t -> t.validateRegistry(errors).stream())`
    let mut frozen: Vec<(RegistryKey<()>, AnyBox)> = Vec::new();
    for task in to_validate {
        if let Some(registry) = task.validate_registry(&mut loading_errors) {
            frozen.push((task.registry_key(), registry));
        }
    }
    if !loading_errors.is_empty() {
        return Err(loading_errors);
    }

    Ok(RegistryAccess::from_pairs(frozen))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holder::{Holder, RegistryId};
    use crate::holder_lookup::HolderGetter;
    use crate::registration_info::RegistrationInfo;
    use crate::registry_file_codec::RegistryFileCodec;

    use rivet_serialization::codec::xmap;

    // -----------------------------------------------------------------------
    // Test element types + fixture helpers
    // -----------------------------------------------------------------------

    /// A context-registry element (biomes) — resolves holder references through
    /// the loader's context access.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestBiome(String);

    fn biome_registry_key() -> RegistryKey<TestBiome> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test_biome"))
    }

    fn biome_element_key(id: &str) -> ResourceKey<TestBiome> {
        ResourceKey::create(
            &biome_registry_key(),
            Identifier::with_default_namespace(id),
        )
    }

    /// A `Codec<TestBiome, JsonRegistryOps>` decoding a bare identifier to
    /// `TestBiome` (the inline fallback of `RegistryFileCodec`; the primary
    /// path is the identifier-resolved reference).
    fn biome_codec() -> Arc<dyn Codec<TestBiome, JsonRegistryOps>> {
        xmap(
            crate::identifier::identifier_codec::<JsonRegistryOps>(),
            Arc::new(|id: &Identifier| TestBiome(id.path().to_string())),
            Arc::new(|b: &TestBiome| Identifier::with_default_namespace(&b.0)),
        )
    }

    /// The loading registry's element codec — a `RegistryFileCodec` over the
    /// CONTEXT biome registry. Elements decode as `Holder<TestBiome>` references
    /// resolved through the loader's context (the `RegistryFileCodec` decode
    /// path exercises the loader-created `RegistryOps`).
    fn biome_holder_codec() -> Arc<dyn Codec<Holder<TestBiome>, JsonRegistryOps>> {
        // The concrete codec is `Send + Sync`; the allow matches the crate's
        // established `registry_file_codec` test pattern for this construction.
        #[allow(clippy::arc_with_non_send_sync)]
        Arc::new(RegistryFileCodec::create(
            &biome_registry_key(),
            biome_codec(),
        ))
    }

    /// A context access with a frozen biome registry `{ "plains", "desert" }`.
    fn context_access() -> (RegistryAccess, RegistryId) {
        let mut builder = RegistryBuilder::new(&biome_registry_key());
        builder.register(
            &biome_element_key("plains"),
            Arc::new(TestBiome("plains".to_string())),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &biome_element_key("desert"),
            Arc::new(TestBiome("desert".to_string())),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let id = registry.registry_id();
        (
            RegistryAccess::from_single_registry(biome_registry_key(), registry),
            id,
        )
    }

    fn feature_registry_key() -> RegistryKey<Holder<TestBiome>> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test_feature"))
    }

    fn feature_element_key(id: &str) -> ResourceKey<Holder<TestBiome>> {
        ResourceKey::create(
            &feature_registry_key(),
            Identifier::with_default_namespace(id),
        )
    }

    /// The resource map for a registry — `Identifier` paths under the
    /// `registry/elementsDirPath` prefix + the `.json` extension, matching
    /// `FileToIdConverter::id_to_file` output.
    fn resource_map(prefix: &str, entries: &[(&str, &str)]) -> HashMap<Identifier, String> {
        entries
            .iter()
            .map(|(id, json)| {
                (
                    Identifier::with_default_namespace(&format!("{}/{}.json", prefix, id)),
                    json.to_string(),
                )
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // FileToIdConverter
    // -----------------------------------------------------------------------

    #[test]
    fn file_to_id_converter_roundtrips() {
        let converter = FileToIdConverter::json("worldgen/feature".to_string());
        let id = Identifier::with_default_namespace("oak_tree");
        let file = converter.id_to_file(&id);
        assert_eq!(
            file,
            Identifier::with_default_namespace("worldgen/feature/oak_tree.json")
        );
        assert!(converter.extension_matches(&file));
        assert_eq!(converter.file_to_id(&file), id);
        // A non-matching extension is filtered by listMatchingResources.
        assert!(!converter.extension_matches(&file.with_path("x.txt")));
    }

    #[test]
    fn file_to_id_converter_registry_uses_the_key_path() {
        let converter = FileToIdConverter::registry(&erase_registry_key(&biome_registry_key()));
        assert_eq!(converter.prefix, "test_biome");
        assert_eq!(converter.extension, ".json");
    }

    #[test]
    fn file_to_id_converter_list_filters_by_extension_and_prefix() {
        let converter = FileToIdConverter::json("worldgen/feature".to_string());
        let mut resources = HashMap::new();
        resources.insert(
            Identifier::with_default_namespace("worldgen/feature/a.json"),
            "\"x\"".to_string(),
        );
        resources.insert(
            Identifier::with_default_namespace("worldgen/feature/b.json"),
            "\"y\"".to_string(),
        );
        resources.insert(
            Identifier::with_default_namespace("worldgen/feature/ignored.txt"),
            "\"z\"".to_string(),
        );
        // Java's `listResources(prefix, ...)` never lists an out-of-prefix file
        // even when it ends with the extension — without this filter the
        // resource would reach `fileToId`'s substring and panic.
        resources.insert(
            Identifier::with_default_namespace("other/prefix/oak.json"),
            "\"w\"".to_string(),
        );
        let matching = converter.list_matching_resources(&resources);
        assert_eq!(matching.len(), 2);
        assert!(!matching.contains_key(&Identifier::with_default_namespace(
            "worldgen/feature/ignored.txt"
        )));
        assert!(
            !matching.contains_key(&Identifier::with_default_namespace("other/prefix/oak.json"))
        );
    }

    #[test]
    #[should_panic]
    fn file_to_id_converter_hostile_file_without_prefix_panics() {
        // Java's `substring(prefix.length() + 1, ...)` throws
        // StringIndexOutOfBoundsException for a path that does not start with
        // the prefix; the Rust slice panics on the same bounds.
        let converter = FileToIdConverter::json("worldgen/feature".to_string());
        let _ = converter.file_to_id(&Identifier::with_default_namespace("no_prefix/a.json"));
    }

    // -----------------------------------------------------------------------
    // RegistryValidator
    // -----------------------------------------------------------------------

    #[test]
    fn non_empty_validator_errors_on_an_empty_registry() {
        let key = feature_registry_key();
        let empty: crate::Registry<Holder<TestBiome>> = RegistryBuilder::new(&key).freeze();
        let mut errors = HashMap::new();
        NonEmptyValidator.validate(&empty, &mut errors);
        assert_eq!(
            errors.get(&erase_key(&key)).map(String::as_str),
            Some("Registry must be non-empty: minecraft:test_feature")
        );
    }

    #[test]
    fn non_empty_validator_passes_a_populated_registry() {
        let access = context_access().0;
        let registry = access
            .lookup(&biome_registry_key())
            .expect("frozen biome registry");
        let mut errors = HashMap::new();
        NonEmptyValidator.validate(registry, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn noop_validator_never_errors() {
        let key = feature_registry_key();
        let empty: crate::Registry<Holder<TestBiome>> = RegistryBuilder::new(&key).freeze();
        let mut errors = HashMap::new();
        NoOpValidator.validate(&empty, &mut errors);
        assert!(errors.is_empty());
    }

    // -----------------------------------------------------------------------
    // RegistryDataLoader — end-to-end
    // -----------------------------------------------------------------------

    #[test]
    fn load_builds_a_frozen_access_from_json_resources() {
        let (context, biome_id) = context_access();
        let data = RegistryData::new(&feature_registry_key(), biome_holder_codec());
        let resources = resource_map(
            "test_feature",
            &[
                ("oak", "\"minecraft:plains\""),
                ("oak_beehive_005", "\"minecraft:desert\""),
            ],
        );
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources);
        let access = load(&context, vec![Box::new(task)]).expect("load succeeds");

        let feature_registry = access
            .lookup(&feature_registry_key())
            .expect("loaded feature registry");
        let feature_id = feature_registry.registry_id();
        // Registration order is the resource-id sorted order:
        // "oak" < "oak_beehive_005" (path-first Identifier Ord).
        assert_eq!(feature_registry.size(), 2);
        let oak_holder = feature_registry
            .get(&feature_element_key("oak"))
            .expect("registered element");
        // The holder id in the FEATURE registry is the sorted insertion index.
        assert_eq!(
            oak_holder,
            Holder::<Holder<TestBiome>>::reference(feature_id, 0)
        );
        // The element VALUE is the biome reference decoded through the loader's
        // context RegistryOps (element id == holder id == insertion index).
        assert_eq!(
            oak_holder.value(feature_registry),
            &Holder::reference(biome_id, 0)
        );
        let beehive_holder = feature_registry
            .get(&feature_element_key("oak_beehive_005"))
            .expect("registered element");
        assert_eq!(
            beehive_holder,
            Holder::<Holder<TestBiome>>::reference(feature_id, 1)
        );
        assert_eq!(
            beehive_holder.value(feature_registry),
            &Holder::reference(biome_id, 1)
        );
    }

    #[test]
    fn load_sorts_resources_regardless_of_map_order() {
        let (context, _) = context_access();
        // Insert in reverse: map order is irrelevant; the loader sorts by id.
        let data = RegistryData::new(&feature_registry_key(), biome_holder_codec());
        let mut resources = HashMap::new();
        resources.insert(
            Identifier::with_default_namespace("test_feature/z.json"),
            "\"minecraft:desert\"".to_string(),
        );
        resources.insert(
            Identifier::with_default_namespace("test_feature/a.json"),
            "\"minecraft:plains\"".to_string(),
        );
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources);
        let access = load(&context, vec![Box::new(task)]).expect("load succeeds");
        let feature_registry = access.lookup(&feature_registry_key()).unwrap();
        assert_eq!(
            feature_registry.registry_key_set(),
            vec![feature_element_key("a"), feature_element_key("z")]
        );
    }

    #[test]
    fn load_reports_invalid_json_as_a_parse_error() {
        let (context, _) = context_access();
        let data = RegistryData::new(&feature_registry_key(), biome_holder_codec());
        let resources = resource_map("test_feature", &[("bad", "not json {")]);
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources);
        let result = load(&context, vec![Box::new(task)]);
        let errors = result.expect_err("invalid JSON is a load error");
        let message = errors.get(&erase_key(&feature_element_key("bad")));
        let message = message.expect("error keyed by the element key");
        assert!(
            message.starts_with("Failed to parse minecraft:bad from pack"),
            "unexpected message: {}",
            message
        );
    }

    #[test]
    fn load_reports_an_element_decode_failure_as_a_parse_error() {
        let (context, _) = context_access();
        let data = RegistryData::new(&feature_registry_key(), biome_holder_codec());
        // A number: the RegistryFileCodec identifier decode fails, and the
        // inline element codec (identifier -> TestBiome) fails too.
        let resources = resource_map("test_feature", &[("num", "123")]);
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources);
        let result = load(&context, vec![Box::new(task)]);
        let errors = result.expect_err("decode failure is a load error");
        let message = errors
            .get(&erase_key(&feature_element_key("num")))
            .expect("error keyed by the element key");
        assert!(
            message.starts_with("Failed to parse minecraft:num from pack"),
            "unexpected message: {}",
            message
        );
    }

    #[test]
    fn load_reports_an_unknown_element_reference() {
        let (context, _) = context_access();
        let data = RegistryData::new(&feature_registry_key(), biome_holder_codec());
        // "minecraft:nope" is not in the context biome registry: the
        // RegistryFileCodec decode fails with "Failed to get element ...".
        let resources = resource_map("test_feature", &[("nope", "\"minecraft:nope\"")]);
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources);
        let result = load(&context, vec![Box::new(task)]);
        let errors = result.expect_err("unknown element is a load error");
        let message = errors
            .get(&erase_key(&feature_element_key("nope")))
            .expect("error keyed by the element key");
        assert!(
            message.starts_with("Failed to parse minecraft:nope from pack"),
            "unexpected message: {}",
            message
        );
    }

    #[test]
    fn load_errors_when_a_non_empty_registry_loads_nothing() {
        let (context, _) = context_access();
        let data = RegistryData::with_validator(
            &feature_registry_key(),
            biome_holder_codec(),
            validator_non_empty(),
        );
        let resources = resource_map("test_feature", &[]);
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources);
        let result = load(&context, vec![Box::new(task)]);
        let errors = result.expect_err("empty registry fails the non-empty validator");
        assert_eq!(
            errors
                .get(&erase_key(&feature_registry_key()))
                .map(String::as_str),
            Some("Registry must be non-empty: minecraft:test_feature")
        );
    }

    #[test]
    fn load_freezes_a_datapack_registry_as_experimental() {
        // `REGISTRATION_INFO_CACHE` maps an absent KnownPack to
        // `Lifecycle.experimental()`; the element lifecycles accumulate into
        // the frozen registry (experimental wins over the task's stable).
        let (context, _) = context_access();
        let data = RegistryData::new(&feature_registry_key(), biome_holder_codec());
        let resources = resource_map("test_feature", &[("oak", "\"minecraft:plains\"")]);
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources);
        let access = load(&context, vec![Box::new(task)]).expect("load succeeds");
        let feature_registry = access.lookup(&feature_registry_key()).unwrap();
        assert_eq!(
            feature_registry.registry_lifecycle(),
            Lifecycle::Experimental
        );
    }

    #[test]
    fn load_binds_tags_before_freeze() {
        let (context, _) = context_access();
        let data = RegistryData::new(&feature_registry_key(), biome_holder_codec());
        let resources = resource_map("test_feature", &[("oak", "\"minecraft:plains\"")]);
        // The caller-supplied tag binding (TagLoader result): "trees" = the
        // registered element id 0 (sorted order).
        let tag = TagKey::create(
            &feature_registry_key(),
            Identifier::with_default_namespace("trees"),
        );
        let task = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data, &resources)
            .with_tag_bindings(vec![(tag.clone(), vec![HolderId(0)])]);
        let access = load(&context, vec![Box::new(task)]).expect("load succeeds");
        let feature_registry = access.lookup(&feature_registry_key()).unwrap();
        assert_eq!(feature_registry.get_tag(&tag), Some(&[HolderId(0)][..]));
    }

    #[test]
    fn load_multiple_registries_share_the_context() {
        // Two loading registries, both resolving biome holders through the same
        // context; each lands in the access.
        let (context, biome_id) = context_access();
        let a_key = ResourceKey::create_registry_key(Identifier::with_default_namespace("test_a"));
        let b_key = ResourceKey::create_registry_key(Identifier::with_default_namespace("test_b"));
        let data_a = RegistryData::new(&a_key, biome_holder_codec());
        let data_b = RegistryData::new(&b_key, biome_holder_codec());
        let resources_a = resource_map("test_a", &[("one", "\"minecraft:plains\"")]);
        let resources_b = resource_map("test_b", &[("two", "\"minecraft:desert\"")]);
        let task_a = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data_a, &resources_a);
        let task_b = ResourceManagerRegistryLoadTask::with_stable_lifecycle(data_b, &resources_b);
        let access =
            load(&context, vec![Box::new(task_a), Box::new(task_b)]).expect("load succeeds");

        let a = access.lookup(&a_key).unwrap();
        let b = access.lookup(&b_key).unwrap();
        let one = a
            .get(&ResourceKey::create(
                &a_key,
                Identifier::with_default_namespace("one"),
            ))
            .expect("registered element");
        let two = b
            .get(&ResourceKey::create(
                &b_key,
                Identifier::with_default_namespace("two"),
            ))
            .expect("registered element");
        assert_eq!(one.value(a), &Holder::reference(biome_id, 0));
        assert_eq!(two.value(b), &Holder::reference(biome_id, 1));
    }

    // -----------------------------------------------------------------------
    // findAndLoadFromResource
    // -----------------------------------------------------------------------

    #[test]
    fn find_and_load_resolves_a_missing_resource_as_an_error() {
        let ops = RegistryOps::create(
            &JsonOps::INSTANCE,
            Box::new(MapLookup {
                by_key: HashMap::new(),
            }),
        );
        let converter = FileToIdConverter::json("test_feature".to_string());
        let element_key = feature_element_key("gone");
        let result = PendingRegistration::<Holder<TestBiome>>::find_and_load_from_resource(
            biome_holder_codec().as_ref(),
            &ops,
            &element_key,
            &converter,
            &HashMap::new(),
        );
        assert_eq!(
            result.err().unwrap(),
            "Failed to find resource minecraft:test_feature/gone.json for element minecraft:gone"
        );
    }
}
