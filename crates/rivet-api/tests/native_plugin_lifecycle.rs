//! Integration & controlled-negative tests for the native plugin lifecycle boundary (#506).

use std::cell::RefCell;
use std::rc::Rc;

use rivet_api::native::{
    ApiVersion, Capability, CapabilitySet, Handle, HandleError, LifecycleError, LifecyclePhase,
    LoadingMode, MinimalLifecyclePlugin, NativePlugin, NativePluginHost, NegotiationError,
    PluginContext, PluginDescriptor, PluginState, ResourceHandle,
};

struct RecordingPlugin {
    descriptor: PluginDescriptor,
    shared_handle: Rc<RefCell<Option<ResourceHandle>>>,
    other_handle: Option<ResourceHandle>,
    fail_in: Option<LifecyclePhase>,
    panic_in: Option<LifecyclePhase>,
    leak_resource_on_enable: bool,
}

impl RecordingPlugin {
    fn new(name: &str) -> Self {
        Self {
            descriptor: PluginDescriptor::new(name, "1.0.0"),
            shared_handle: Rc::new(RefCell::new(None)),
            other_handle: None,
            fail_in: None,
            panic_in: None,
            leak_resource_on_enable: false,
        }
    }
}

impl NativePlugin for RecordingPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        self.descriptor.clone()
    }

    fn on_load(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        let h = ctx.alloc_resource("load-res").map_err(|e| e.to_string())?;
        *self.shared_handle.borrow_mut() = Some(h);
        if self.panic_in == Some(LifecyclePhase::Load) {
            panic!("boom during load");
        }
        if self.fail_in == Some(LifecyclePhase::Load) {
            return Err("load failed".to_owned());
        }
        Ok(())
    }

    fn on_enable(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        if let Some(other) = self.other_handle {
            ctx.read_resource(other).map_err(|e| e.to_string())?;
        }
        if self.leak_resource_on_enable {
            let h = ctx
                .alloc_resource("enable-res")
                .map_err(|e| e.to_string())?;
            *self.shared_handle.borrow_mut() = Some(h);
        }
        if self.panic_in == Some(LifecyclePhase::Enable) {
            panic!("boom during enable");
        }
        if self.fail_in == Some(LifecyclePhase::Enable) {
            return Err("enable failed".to_owned());
        }
        Ok(())
    }

    fn on_disable(&mut self, _ctx: &mut PluginContext<'_>) -> Result<(), String> {
        if self.panic_in == Some(LifecyclePhase::Disable) {
            panic!("boom during disable");
        }
        if self.fail_in == Some(LifecyclePhase::Disable) {
            return Err("disable failed".to_owned());
        }
        Ok(())
    }
}

#[test]
fn minimal_lifecycle_fixture_round_trip() {
    let mut host = NativePluginHost::synthetic_default();
    let handle = host
        .register(Box::new(MinimalLifecyclePlugin::new("minimal-fixture")))
        .expect("register succeeds");
    assert_eq!(host.state(handle).unwrap(), PluginState::Registered);
    assert_eq!(host.live_resource_count(), 0);

    host.load(handle).expect("load succeeds");
    assert_eq!(host.state(handle).unwrap(), PluginState::Loaded);

    host.enable(handle).expect("enable succeeds");
    assert_eq!(host.state(handle).unwrap(), PluginState::Enabled);
    assert_eq!(host.live_resource_count(), 1);
    assert_eq!(host.live_resource_count_for(handle).unwrap(), 1);

    host.disable(handle).expect("disable succeeds");
    assert_eq!(host.state(handle).unwrap(), PluginState::Disabled);
    assert_eq!(host.live_resource_count(), 0);

    host.unload(handle).expect("unload succeeds");
    assert_eq!(
        host.logs(),
        &[
            "[minimal-fixture] loaded",
            "[minimal-fixture] enabled",
            "[minimal-fixture] disabled",
        ]
    );

    // Retained plugin handle is now stale.
    let err = host.state(handle).unwrap_err();
    assert!(matches!(
        err,
        LifecycleError::Handle(HandleError::Stale { .. })
    ));
}

#[test]
fn api_version_and_capability_negotiation() {
    let mut host_caps = CapabilitySet::synthetic_default();
    host_caps.insert(Capability::WorldRead);
    let mut host = NativePluginHost::new(ApiVersion::new(1, 2, 3), host_caps);

    // Compatible lower minor + optional capability partially granted.
    let mut good = RecordingPlugin::new("good-plugin");
    good.descriptor.target_api = ApiVersion::new(1, 1, 0);
    good.descriptor.required_capabilities = CapabilitySet::from_iter([Capability::LifecycleHooks]);
    good.descriptor.optional_capabilities =
        CapabilitySet::from_iter([Capability::WorldRead, Capability::WorldWrite]);
    let handle = host.register(Box::new(good)).expect("compatible plugin");
    let granted = host.granted_capabilities(handle).unwrap();
    assert!(granted.contains(Capability::LifecycleHooks));
    assert!(granted.contains(Capability::WorldRead));
    assert!(!granted.contains(Capability::WorldWrite));

    // Controlled negative: incompatible major version.
    let mut bad_major = RecordingPlugin::new("bad-major");
    bad_major.descriptor.target_api = ApiVersion::new(2, 0, 0);
    assert!(matches!(
        host.register(Box::new(bad_major)),
        Err(LifecycleError::Negotiation(
            NegotiationError::IncompatibleApiVersion { .. }
        ))
    ));

    // Controlled negative: plugin requests newer minor than host.
    let mut too_new_minor = RecordingPlugin::new("too-new-minor");
    too_new_minor.descriptor.target_api = ApiVersion::new(1, 3, 0);
    assert!(matches!(
        host.register(Box::new(too_new_minor)),
        Err(LifecycleError::Negotiation(
            NegotiationError::IncompatibleApiVersion { .. }
        ))
    ));

    // Controlled negative: missing required M3-gated capability.
    let mut missing_cap = RecordingPlugin::new("missing-cap");
    missing_cap.descriptor.target_api = ApiVersion::new(1, 2, 0);
    missing_cap.descriptor.required_capabilities =
        CapabilitySet::from_iter([Capability::EntityWrite]);
    assert_eq!(
        host.register(Box::new(missing_cap)),
        Err(LifecycleError::Negotiation(
            NegotiationError::MissingCapabilities(vec![Capability::EntityWrite])
        ))
    );

    // Pre-1.0 strict minor check.
    assert!(ApiVersion::new(0, 1, 0).is_compatible_with_host(ApiVersion::new(0, 1, 2)));
    assert!(!ApiVersion::new(0, 1, 3).is_compatible_with_host(ApiVersion::new(0, 1, 2)));
    assert!(!ApiVersion::new(0, 2, 0).is_compatible_with_host(ApiVersion::new(0, 1, 2)));
}

#[test]
fn explicit_dynamic_loading_mode_rejected_fail_closed() {
    let mut host = NativePluginHost::synthetic_default();
    let mut plugin = RecordingPlugin::new("dynamic-plugin");
    plugin.descriptor.loading_mode = LoadingMode::DynamicCdylib {
        abi_version: 1,
        symbol: "rivet_plugin_entry".to_owned(),
    };
    assert!(matches!(
        host.register(Box::new(plugin)),
        Err(LifecycleError::Negotiation(
            NegotiationError::UnsupportedLoadingMode(LoadingMode::DynamicCdylib { .. })
        ))
    ));
}

#[test]
fn generational_handles_reject_stale_zero_and_cross_plugin_access() {
    let mut host = NativePluginHost::synthetic_default();

    let mut plugin_a = RecordingPlugin::new("plugin-a");
    plugin_a.leak_resource_on_enable = true;
    let handle_a_slot = Rc::clone(&plugin_a.shared_handle);
    let pa = host.register(Box::new(plugin_a)).unwrap();
    host.load(pa).unwrap();
    host.enable(pa).unwrap();

    let res_a = handle_a_slot.borrow().expect("plugin-a allocated resource");
    assert_eq!(host.read_resource_as(pa, res_a).unwrap(), "enable-res");
    assert_eq!(ResourceHandle::from_raw_u64(res_a.to_raw_u64()), res_a);

    // Zero raw handle is always invalid.
    let zero_handle: ResourceHandle = Handle::from_raw_u64(0);
    assert!(matches!(
        host.read_resource_as(pa, zero_handle),
        Err(LifecycleError::Handle(HandleError::Invalid {
            index: 0,
            generation: 0
        }))
    ));

    // Cross-plugin ownership isolation: plugin-b cannot access plugin-a's live resource.
    let mut plugin_b = RecordingPlugin::new("plugin-b");
    plugin_b.other_handle = Some(res_a);
    let pb = host.register(Box::new(plugin_b)).unwrap();
    assert!(matches!(
        host.read_resource_as(pb, res_a),
        Err(LifecycleError::Handle(
            HandleError::UnauthorizedOwner { .. }
        ))
    ));
    host.load(pb).unwrap();
    let enable_err = host.enable(pb).unwrap_err();
    assert!(matches!(
        enable_err,
        LifecycleError::PluginReturnedError {
            phase: LifecyclePhase::Enable,
            ..
        }
    ));
    assert_eq!(host.state(pb).unwrap(), PluginState::Failed);
    // plugin-b's load-time resource was automatically cleaned up on enable failure.
    assert_eq!(host.live_resource_count_for(pb).unwrap(), 0);
    // plugin-a was untouched.
    assert_eq!(host.state(pa).unwrap(), PluginState::Enabled);
    assert_eq!(host.live_resource_count_for(pa).unwrap(), 2);

    // Disabling plugin-a revokes its leaked resources and invalidates res_a even after slot reuse.
    host.disable(pa).unwrap();
    assert_eq!(host.live_resource_count(), 0);
    assert!(matches!(
        host.read_resource_as(pa, res_a),
        Err(LifecycleError::Handle(HandleError::Stale { .. }))
    ));

    // Re-enabling plugin-a reuses freed slots with bumped generations; old res_a stays stale.
    host.enable(pa).unwrap();
    let res_a_new = handle_a_slot.borrow().unwrap();
    assert_ne!(res_a_new.generation(), res_a.generation());
    assert!(matches!(
        host.read_resource_as(pa, res_a),
        Err(LifecycleError::Handle(HandleError::Stale { .. }))
    ));
    assert_eq!(host.read_resource_as(pa, res_a_new).unwrap(), "enable-res");
}

#[test]
fn panic_containment_and_cleanup_across_all_lifecycle_phases() {
    let mut host = NativePluginHost::synthetic_default();

    // Healthy sibling plugin must survive panics in other plugins.
    let healthy = host
        .register(Box::new(MinimalLifecyclePlugin::new("healthy")))
        .unwrap();
    host.load(healthy).unwrap();
    host.enable(healthy).unwrap();

    // Panic in on_load: contained, state=Failed, load-time resource revoked.
    let mut panic_load = RecordingPlugin::new("panic-load");
    panic_load.panic_in = Some(LifecyclePhase::Load);
    let h_load_res = Rc::clone(&panic_load.shared_handle);
    let p_load = host.register(Box::new(panic_load)).unwrap();
    let err = host.load(p_load).unwrap_err();
    assert!(matches!(
        err,
        LifecycleError::PluginPanicked {
            phase: LifecyclePhase::Load,
            ..
        }
    ));
    assert_eq!(host.state(p_load).unwrap(), PluginState::Failed);
    assert_eq!(host.live_resource_count_for(p_load).unwrap(), 0);
    let leaked_load_res = h_load_res.borrow().unwrap();
    assert!(matches!(
        host.read_resource_as(p_load, leaked_load_res),
        Err(LifecycleError::Handle(HandleError::Stale { .. }))
    ));

    // Panic in on_enable: contained, state=Failed, all resources revoked.
    let mut panic_enable = RecordingPlugin::new("panic-enable");
    panic_enable.leak_resource_on_enable = true;
    panic_enable.panic_in = Some(LifecyclePhase::Enable);
    let p_enable = host.register(Box::new(panic_enable)).unwrap();
    host.load(p_enable).unwrap();
    assert_eq!(host.live_resource_count_for(p_enable).unwrap(), 1);
    let err = host.enable(p_enable).unwrap_err();
    assert!(matches!(
        err,
        LifecycleError::PluginPanicked {
            phase: LifecyclePhase::Enable,
            ..
        }
    ));
    assert_eq!(host.state(p_enable).unwrap(), PluginState::Failed);
    assert_eq!(host.live_resource_count_for(p_enable).unwrap(), 0);

    // Panic in on_disable: contained, state=Failed, all resources still revoked.
    let mut panic_disable = RecordingPlugin::new("panic-disable");
    panic_disable.leak_resource_on_enable = true;
    panic_disable.panic_in = Some(LifecyclePhase::Disable);
    let p_disable = host.register(Box::new(panic_disable)).unwrap();
    host.load(p_disable).unwrap();
    host.enable(p_disable).unwrap();
    assert_eq!(host.live_resource_count_for(p_disable).unwrap(), 2);
    let err = host.disable(p_disable).unwrap_err();
    assert!(matches!(
        err,
        LifecycleError::PluginPanicked {
            phase: LifecyclePhase::Disable,
            ..
        }
    ));
    assert_eq!(host.state(p_disable).unwrap(), PluginState::Failed);
    assert_eq!(host.live_resource_count_for(p_disable).unwrap(), 0);

    // Healthy sibling plugin remained completely intact.
    assert_eq!(host.state(healthy).unwrap(), PluginState::Enabled);
    assert_eq!(host.live_resource_count(), 1);
}

#[test]
fn invalid_names_duplicates_and_state_transitions_fail_closed() {
    let mut host = NativePluginHost::synthetic_default();

    assert!(matches!(
        host.register(Box::new(MinimalLifecyclePlugin::new(""))),
        Err(LifecycleError::Negotiation(
            NegotiationError::InvalidPluginName(_)
        ))
    ));
    assert!(matches!(
        host.register(Box::new(MinimalLifecyclePlugin::new("bad name!"))),
        Err(LifecycleError::Negotiation(
            NegotiationError::InvalidPluginName(_)
        ))
    ));

    let p = host
        .register(Box::new(MinimalLifecyclePlugin::new("dup-check")))
        .unwrap();
    assert!(matches!(
        host.register(Box::new(MinimalLifecyclePlugin::new("dup-check"))),
        Err(LifecycleError::Negotiation(
            NegotiationError::DuplicatePluginName(_)
        ))
    ));

    // Cannot enable before load.
    assert!(matches!(
        host.enable(p),
        Err(LifecycleError::InvalidStateTransition {
            from: PluginState::Registered,
            phase: LifecyclePhase::Enable,
            ..
        })
    ));

    host.load(p).unwrap();
    host.enable(p).unwrap();

    // Cannot unload while enabled.
    assert!(matches!(
        host.unload(p),
        Err(LifecycleError::InvalidStateTransition {
            from: PluginState::Enabled,
            phase: LifecyclePhase::Unload,
            ..
        })
    ));

    host.disable(p).unwrap();
    host.unload(p).unwrap();

    // After unload, name can be registered again cleanly.
    let p2 = host
        .register(Box::new(MinimalLifecyclePlugin::new("dup-check")))
        .unwrap();
    assert_ne!(p, p2);
}
