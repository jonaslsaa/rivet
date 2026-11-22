//! Synchronous command registration and dispatch for native Rust plugins (#507).
//!
//! Mirrors Paper's `org.bukkit.command.SimpleCommandMap` normalization and
//! fallback-prefix routing (`plugin:label` + bare `label`), using value-only
//! sender descriptors, generational handles, per-plugin ownership, panic
//! containment, and automatic cleanup on plugin disable/unload.

use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::handle::{Handle, HandleError, HandleTable, PluginId};
use super::lifecycle::extract_panic_message_and_drop;

/// Marker type for handles referring to registered native commands.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct CommandMarker;

/// Generational handle to a registered native command.
pub type CommandHandle = Handle<CommandMarker>;

/// Value-only command sender descriptor (never exposes arena pointers or borrows).
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CommandSender {
    Console,
    Player { uuid: u128, name: String },
    Synthetic { name: String },
}

impl CommandSender {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Console => "CONSOLE",
            Self::Player { name, .. } | Self::Synthetic { name } => name,
        }
    }
}

/// Static metadata for a command registered by a native plugin.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct CommandSpec {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub usage: String,
}

impl CommandSpec {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let usage = format!("/{name}");
        Self {
            name,
            aliases: Vec::new(),
            description: String::new(),
            usage,
        }
    }

    #[must_use]
    pub fn with_aliases(mut self, aliases: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.aliases = aliases.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

/// Outcome of dispatching a command line through [`CommandRegistry::dispatch`].
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CommandDispatchOutcome {
    /// Command executed normally and returned `Ok(())`.
    Handled {
        resolved_label: String,
        owner: PluginId,
    },
    /// No command matched the sent label (or the command line was empty).
    UnknownCommand,
    /// Command returned a structured error.
    ReturnedError {
        resolved_label: String,
        owner: PluginId,
        message: String,
    },
    /// Command panicked; the panic was contained by the registry.
    Panicked {
        resolved_label: String,
        owner: PluginId,
        message: String,
    },
}

/// Errors produced when registering or unregistering commands.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum CommandError {
    InvalidLabel(String),
    InvalidFallbackPrefix(String),
    DuplicateNamespacedCommand(String),
    Handle(HandleError),
}

impl From<HandleError> for CommandError {
    fn from(value: HandleError) -> Self {
        Self::Handle(value)
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLabel(label) => write!(f, "invalid command label: {label:?}"),
            Self::InvalidFallbackPrefix(prefix) => {
                write!(f, "invalid command fallback prefix: {prefix:?}")
            }
            Self::DuplicateNamespacedCommand(key) => {
                write!(f, "namespaced command already registered: {key}")
            }
            Self::Handle(err) => write!(f, "command handle error: {err}"),
        }
    }
}

impl std::error::Error for CommandError {}

type CommandExecutor = Box<dyn FnMut(&CommandSender, &str, &[&str]) -> Result<(), String>>;

struct CommandEntry {
    spec: CommandSpec,
    primary_label: String,
    fallback_prefix: String,
    bound_keys: Vec<String>,
    executor: Option<CommandExecutor>,
}

/// Synchronous command registry mirroring Paper's `SimpleCommandMap` lookup rules.
#[derive(Default)]
pub struct CommandRegistry {
    commands: HandleTable<CommandMarker, CommandEntry>,
    /// Maps normalized command label (both `label` and `fallback_prefix:label`) -> handle.
    known_commands: BTreeMap<String, CommandHandle>,
}

impl CommandRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub const fn command_count(&self) -> usize {
        self.commands.live_count()
    }

    #[must_use]
    pub fn command_count_for(&self, owner: PluginId) -> usize {
        self.commands.live_count_for(owner)
    }

    /// Returns the registered handle for a normalized command label, if any.
    #[must_use]
    pub fn lookup(&self, label: &str) -> Option<CommandHandle> {
        let normalized = label.trim().to_ascii_lowercase();
        self.known_commands.get(&normalized).copied()
    }

    /// Registers a command owned by `owner` under `fallback_prefix` (typically the plugin name).
    ///
    /// Follows `SimpleCommandMap.register`:
    /// - Normalizes `name`, `aliases`, and `fallback_prefix` to ASCII lowercase.
    /// - Always binds `<fallback_prefix>:<label>` for the primary name and each alias.
    /// - Binds bare `<label>` if no primary command already owns it (aliases do not
    ///   overwrite existing commands or aliases).
    pub fn register<F>(
        &mut self,
        owner: PluginId,
        fallback_prefix: &str,
        spec: CommandSpec,
        executor: F,
    ) -> Result<CommandHandle, CommandError>
    where
        F: FnMut(&CommandSender, &str, &[&str]) -> Result<(), String> + 'static,
    {
        let prefix = normalize_token(fallback_prefix)
            .ok_or_else(|| CommandError::InvalidFallbackPrefix(fallback_prefix.to_owned()))?;
        let primary = normalize_token(&spec.name)
            .ok_or_else(|| CommandError::InvalidLabel(spec.name.clone()))?;

        let namespaced_primary = format!("{prefix}:{primary}");
        if self.known_commands.contains_key(&namespaced_primary) {
            return Err(CommandError::DuplicateNamespacedCommand(namespaced_primary));
        }

        let mut normalized_aliases = Vec::with_capacity(spec.aliases.len());
        for alias in &spec.aliases {
            let norm =
                normalize_token(alias).ok_or_else(|| CommandError::InvalidLabel(alias.clone()))?;
            if norm != primary && !normalized_aliases.contains(&norm) {
                let ns_alias = format!("{prefix}:{norm}");
                if self.known_commands.contains_key(&ns_alias) {
                    return Err(CommandError::DuplicateNamespacedCommand(ns_alias));
                }
                normalized_aliases.push(norm);
            }
        }

        let entry = CommandEntry {
            spec,
            primary_label: primary.clone(),
            fallback_prefix: prefix.clone(),
            bound_keys: Vec::new(),
            executor: Some(Box::new(executor)),
        };
        let handle = self.commands.insert(owner, entry)?;

        let mut bound_keys = Vec::new();
        // Always bind fallback_prefix:primary
        self.known_commands
            .insert(namespaced_primary.clone(), handle);
        bound_keys.push(namespaced_primary);

        // Bind bare primary if not conflicting with an existing primary command
        let bare_primary_conflict = self
            .known_commands
            .get(&primary)
            .and_then(|&existing| self.commands.get(existing, None).ok())
            .is_some_and(|existing_entry| existing_entry.primary_label == primary);
        if !bare_primary_conflict {
            self.known_commands.insert(primary.clone(), handle);
            bound_keys.push(primary);
        }

        // Bind aliases: always fallback_prefix:alias, bare alias only if unused
        for alias in normalized_aliases {
            let ns_alias = format!("{prefix}:{alias}");
            self.known_commands.insert(ns_alias.clone(), handle);
            bound_keys.push(ns_alias);

            if !self.known_commands.contains_key(&alias) {
                self.known_commands.insert(alias.clone(), handle);
                bound_keys.push(alias);
            }
        }

        if let Ok(slot) = self.commands.get_mut(handle, Some(owner)) {
            slot.bound_keys = bound_keys;
        }
        Ok(handle)
    }

    /// Unregisters a command owned by `owner`, removing all of its bound labels.
    pub fn unregister(
        &mut self,
        owner: PluginId,
        handle: CommandHandle,
    ) -> Result<(), CommandError> {
        let entry = self.commands.remove(handle, Some(owner))?;
        self.detach_keys(handle, &entry.bound_keys);
        drop_executor_contained(entry.executor);
        Ok(())
    }

    /// Unregisters and drops all commands owned by `owner`.
    pub fn unregister_all_for_plugin(&mut self, owner: PluginId) -> usize {
        let removed = self.commands.revoke_owned_by(owner);
        let count = removed.len();
        for entry in removed {
            for key in &entry.bound_keys {
                if self
                    .known_commands
                    .get(key)
                    .is_some_and(|h| self.commands.get(*h, None).is_err())
                {
                    self.known_commands.remove(key);
                }
            }
            drop_executor_contained(entry.executor);
        }
        count
    }

    /// Dispatches a raw command line (optional leading `'/'` is stripped),
    /// collapsing adjacent ASCII spaces just like Paper's `SimpleCommandMap.dispatch`.
    pub fn dispatch(
        &mut self,
        sender: &CommandSender,
        command_line: &str,
    ) -> CommandDispatchOutcome {
        let trimmed = command_line.strip_prefix('/').unwrap_or(command_line);
        let parts: Vec<&str> = trimmed.split(' ').filter(|part| !part.is_empty()).collect();
        let Some((&raw_label, args)) = parts.split_first() else {
            return CommandDispatchOutcome::UnknownCommand;
        };

        let sent_label = raw_label.to_ascii_lowercase();
        let Some(&handle) = self.known_commands.get(&sent_label) else {
            return CommandDispatchOutcome::UnknownCommand;
        };
        let Ok(owner) = self.commands.owner_of(handle) else {
            return CommandDispatchOutcome::UnknownCommand;
        };
        let Ok(entry) = self.commands.get_mut(handle, None) else {
            return CommandDispatchOutcome::UnknownCommand;
        };
        let Some(mut executor) = entry.executor.take() else {
            return CommandDispatchOutcome::ReturnedError {
                resolved_label: sent_label,
                owner,
                message: "reentrant invocation of currently executing command".to_owned(),
            };
        };

        let outcome = catch_unwind(AssertUnwindSafe(|| executor(sender, &sent_label, args)));

        if let Ok(slot) = self.commands.get_mut(handle, None) {
            slot.executor = Some(executor);
        } else {
            drop_executor_contained(Some(executor));
        }

        match outcome {
            Ok(Ok(())) => CommandDispatchOutcome::Handled {
                resolved_label: sent_label,
                owner,
            },
            Ok(Err(message)) => CommandDispatchOutcome::ReturnedError {
                resolved_label: sent_label,
                owner,
                message,
            },
            Err(payload) => CommandDispatchOutcome::Panicked {
                resolved_label: sent_label,
                owner,
                message: extract_panic_message_and_drop(payload),
            },
        }
    }

    /// Returns the registered metadata for a live command handle.
    pub fn spec(&self, handle: CommandHandle) -> Result<&CommandSpec, CommandError> {
        Ok(&self.commands.get(handle, None)?.spec)
    }

    /// Returns the fallback prefix for a live command handle.
    pub fn fallback_prefix(&self, handle: CommandHandle) -> Result<&str, CommandError> {
        Ok(&self.commands.get(handle, None)?.fallback_prefix)
    }

    /// Unregisters and drops every command during shutdown.
    pub fn clear(&mut self) -> usize {
        let handles: Vec<CommandHandle> = self
            .known_commands
            .values()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        self.known_commands.clear();
        let mut count = 0usize;
        for handle in handles {
            if let Ok(entry) = self.commands.remove(handle, None) {
                drop_executor_contained(entry.executor);
                count += 1;
            }
        }
        count
    }

    fn detach_keys(&mut self, handle: CommandHandle, keys: &[String]) {
        for key in keys {
            if self.known_commands.get(key).copied() == Some(handle) {
                self.known_commands.remove(key);
            }
        }
    }
}

fn normalize_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn drop_executor_contained(executor: Option<CommandExecutor>) {
    if let Some(exec) = executor
        && let Err(payload) = catch_unwind(AssertUnwindSafe(move || drop(exec)))
    {
        let _ = extract_panic_message_and_drop(payload);
    }
}
