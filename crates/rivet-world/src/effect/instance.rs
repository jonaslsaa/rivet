//! `net.minecraft.world.effect.MobEffectInstance` and `MobEffectInstance.BlendState`.
//!
//! Preserves exact Paper 26.2 semantics including:
//! - Amplifier clamping to `[0, 255]`
//! - Hidden-effect stacking chains in `update()`
//! - Paper's MC-259832 multi-level 0-duration promotion loop in `downgrade_to_hidden_effect()`
//! - `compareTo()` with the `32147` duration cutoff
//! - `equals()` / `hashCode()` ignoring `hidden_effect` and `blend_state`
//! - Recursive `Details` NBT persistence (`id`, `amplifier`, `duration`, `ambient`,
//!   `show_particles`, `show_icon`, `hidden_effect`).

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_util::mth;

use crate::effect::{EffectParticle, MobEffectDef, MobEffectId};

/// `MobEffectInstance.INFINITE_DURATION = -1`.
pub const INFINITE_DURATION: i32 = -1;
/// `MobEffectInstance.MIN_AMPLIFIER = 0`.
pub const MIN_AMPLIFIER: i32 = 0;
/// `MobEffectInstance.MAX_AMPLIFIER = 255`.
pub const MAX_AMPLIFIER: i32 = 255;

/// Client-side visual blend state (`MobEffectInstance.BlendState`, used by Nausea & Darkness).
#[derive(Debug, Clone, Copy, Default)]
pub struct BlendState {
    factor: f32,
    factor_previous_frame: f32,
}

impl BlendState {
    pub const fn new() -> Self {
        Self {
            factor: 0.0,
            factor_previous_frame: 0.0,
        }
    }

    pub fn set_immediate(&mut self, has_effect: bool) {
        self.factor = if has_effect { 1.0 } else { 0.0 };
        self.factor_previous_frame = self.factor;
    }

    pub fn copy_from(&mut self, other: &Self) {
        self.factor = other.factor;
        self.factor_previous_frame = other.factor_previous_frame;
    }

    pub fn tick(&mut self, effect: MobEffectId, has_effect: bool) {
        self.factor_previous_frame = self.factor;
        let target = if has_effect { 1.0_f32 } else { 0.0_f32 };
        if self.factor != target {
            let def = effect.def();
            let blend_duration = if has_effect {
                def.blend_in_duration_ticks
            } else {
                def.blend_out_duration_ticks
            };
            let step = if blend_duration == 0 {
                1.0_f32
            } else {
                1.0_f32 / (blend_duration as f32)
            };
            self.factor += (target - self.factor).clamp(-step, step);
        }
    }

    /// `BlendState.getFactor(LivingEntity, float partialTickTime)`.
    ///
    /// Pass `is_camera_entity_removed_or_dead` = `false` for normal living entities.
    pub fn get_factor(&self, partial_tick_time: f32) -> f32 {
        mth::lerp_f32(partial_tick_time, self.factor_previous_frame, self.factor)
    }
}

/// `net.minecraft.world.effect.MobEffectInstance`.
#[derive(Debug, Clone)]
pub struct MobEffectInstance {
    effect: MobEffectId,
    duration: i32,
    amplifier: i32,
    ambient: bool,
    visible: bool,
    show_icon: bool,
    hidden_effect: Option<Box<MobEffectInstance>>,
    blend_state: BlendState,
}

impl MobEffectInstance {
    /// `new MobEffectInstance(effect)` — duration 0, amplifier 0, ambient false, visible true, showIcon true.
    pub fn new(effect: MobEffectId) -> Self {
        Self::with_duration(effect, 0)
    }

    /// `new MobEffectInstance(effect, duration)`.
    pub fn with_duration(effect: MobEffectId, duration: i32) -> Self {
        Self::with_duration_and_amplifier(effect, duration, 0)
    }

    /// `new MobEffectInstance(effect, duration, amplifier)`.
    pub fn with_duration_and_amplifier(effect: MobEffectId, duration: i32, amplifier: i32) -> Self {
        Self::full(effect, duration, amplifier, false, true, true, None)
    }

    /// `new MobEffectInstance(effect, duration, amplifier, ambient, visible)`.
    pub fn with_flags(
        effect: MobEffectId,
        duration: i32,
        amplifier: i32,
        ambient: bool,
        visible: bool,
    ) -> Self {
        Self::full(effect, duration, amplifier, ambient, visible, visible, None)
    }

    /// Full constructor mirroring the 7-arg Java constructor.
    pub fn full(
        effect: MobEffectId,
        duration: i32,
        amplifier: i32,
        ambient: bool,
        visible: bool,
        show_icon: bool,
        hidden_effect: Option<Box<MobEffectInstance>>,
    ) -> Self {
        Self {
            effect,
            duration,
            amplifier: amplifier.clamp(MIN_AMPLIFIER, MAX_AMPLIFIER),
            ambient,
            visible,
            show_icon,
            hidden_effect,
            blend_state: BlendState::new(),
        }
    }

    /// Copy constructor mirroring `new MobEffectInstance(MobEffectInstance copy)`:
    /// copies details (effect, duration, amplifier, ambient, visible, show_icon)
    /// without copying `hidden_effect` or `blend_state`.
    pub fn copy_details(&self) -> Self {
        Self {
            effect: self.effect,
            duration: self.duration,
            amplifier: self.amplifier,
            ambient: self.ambient,
            visible: self.visible,
            show_icon: self.show_icon,
            hidden_effect: None,
            blend_state: BlendState::new(),
        }
    }

    #[inline]
    pub const fn effect(&self) -> MobEffectId {
        self.effect
    }

    #[inline]
    pub const fn effect_def(&self) -> &'static MobEffectDef {
        self.effect.def()
    }

    #[inline]
    pub const fn is(&self, effect: MobEffectId) -> bool {
        self.effect as u8 == effect as u8
    }

    #[inline]
    pub const fn duration(&self) -> i32 {
        self.duration
    }

    #[inline]
    pub const fn amplifier(&self) -> i32 {
        self.amplifier
    }

    #[inline]
    pub const fn is_ambient(&self) -> bool {
        self.ambient
    }

    #[inline]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    #[inline]
    pub const fn show_icon(&self) -> bool {
        self.show_icon
    }

    #[inline]
    pub fn hidden_effect(&self) -> Option<&MobEffectInstance> {
        self.hidden_effect.as_deref()
    }

    #[inline]
    pub const fn particle_options(&self) -> EffectParticle {
        self.effect.def().create_particle_options(self.ambient)
    }

    #[inline]
    pub const fn description_id(&self) -> &'static str {
        self.effect.description_id()
    }

    #[inline]
    pub const fn is_infinite_duration(&self) -> bool {
        self.duration == INFINITE_DURATION
    }

    /// `MobEffectInstance.isShorterDurationThan(MobEffectInstance other)`.
    #[inline]
    pub const fn is_shorter_duration_than(&self, other: &Self) -> bool {
        !self.is_infinite_duration()
            && (self.duration < other.duration || other.is_infinite_duration())
    }

    /// `MobEffectInstance.endsWithin(int ticks)`.
    #[inline]
    pub const fn ends_within(&self, ticks: i32) -> bool {
        !self.is_infinite_duration() && self.duration <= ticks
    }

    /// `MobEffectInstance.hasRemainingDuration()`.
    #[inline]
    pub const fn has_remaining_duration(&self) -> bool {
        self.is_infinite_duration() || self.duration > 0
    }

    /// `MobEffectInstance.mapDuration(Int2IntFunction mapper)`.
    #[inline]
    pub fn map_duration(&self, mapper: impl FnOnce(i32) -> i32) -> i32 {
        if !self.is_infinite_duration() && self.duration != 0 {
            mapper(self.duration)
        } else {
            self.duration
        }
    }

    /// `MobEffectInstance.withScaledDuration(float scale)`.
    pub fn with_scaled_duration(&self, scale: f32) -> Self {
        let mut copy = self.copy_details();
        copy.duration = copy.map_duration(|d| mth::floor((d as f32) * scale).max(1));
        copy
    }

    /// `MobEffectInstance.getBlendFactor(partialTickTime)`.
    #[inline]
    pub fn blend_factor(&self, partial_tick_time: f32) -> f32 {
        self.blend_state.get_factor(partial_tick_time)
    }

    /// `MobEffectInstance.copyBlendState(MobEffectInstance instance)`.
    #[inline]
    pub fn copy_blend_state(&mut self, other: &Self) {
        self.blend_state.copy_from(&other.blend_state);
    }

    /// `MobEffectInstance.skipBlending()`.
    #[inline]
    pub fn skip_blending(&mut self) {
        let has_effect = !self.ends_within(self.effect.def().blend_out_advance_ticks);
        self.blend_state.set_immediate(has_effect);
    }

    fn set_details_from(&mut self, other: &MobEffectInstance) {
        self.duration = other.duration;
        self.amplifier = other.amplifier;
        self.ambient = other.ambient;
        self.visible = other.visible;
        self.show_icon = other.show_icon;
    }

    /// `MobEffectInstance.update(MobEffectInstance takeOver)` — exact Paper 26.2 stacking
    /// and hidden-effect chain merge logic.
    ///
    /// Returns `true` if the active (visible) details changed.
    pub fn update(&mut self, take_over: &MobEffectInstance) -> bool {
        let mut changed = false;
        if take_over.amplifier > self.amplifier {
            if take_over.is_shorter_duration_than(self) {
                let prev_hidden = self.hidden_effect.take();
                let mut saved = self.copy_details();
                saved.hidden_effect = prev_hidden;
                self.hidden_effect = Some(Box::new(saved));
            }
            self.amplifier = take_over.amplifier;
            self.duration = take_over.duration;
            changed = true;
        } else if self.is_shorter_duration_than(take_over) {
            if take_over.amplifier == self.amplifier {
                self.duration = take_over.duration;
                changed = true;
            } else if let Some(ref mut hidden) = self.hidden_effect {
                hidden.update(take_over);
            } else {
                self.hidden_effect = Some(Box::new(take_over.copy_details()));
            }
        }

        if (!take_over.ambient && self.ambient) || changed {
            self.ambient = take_over.ambient;
            changed = true;
        }

        if take_over.visible != self.visible {
            self.visible = take_over.visible;
            changed = true;
        }

        if take_over.show_icon != self.show_icon {
            self.show_icon = take_over.show_icon;
            changed = true;
        }

        changed
    }

    /// `MobEffectInstance.tickDownDuration()` — recursively decrements hidden effects first.
    pub(crate) fn tick_down_duration(&mut self) {
        if let Some(ref mut hidden) = self.hidden_effect {
            hidden.tick_down_duration();
        }
        self.duration = self.map_duration(|d| d.wrapping_sub(1));
    }

    /// `MobEffectInstance.downgradeToHiddenEffect()` — includes Paper's MC-259832 fix
    /// looping through chained 0-duration hidden effects.
    pub(crate) fn downgrade_to_hidden_effect(&mut self) -> bool {
        if self.duration != 0 || self.hidden_effect.is_none() {
            return false;
        }

        loop {
            if let Some(mut hidden) = self.hidden_effect.take() {
                self.set_details_from(&hidden);
                self.hidden_effect = hidden.hidden_effect.take();
            }
            if self.duration != 0 || self.hidden_effect.is_none() {
                break;
            }
        }

        true
    }

    /// `MobEffectInstance.tickClient()`.
    pub fn tick_client(&mut self) {
        if self.has_remaining_duration() {
            self.tick_down_duration();
            self.downgrade_to_hidden_effect();
        }
        let has_effect = !self.ends_within(self.effect.def().blend_out_advance_ticks);
        self.blend_state.tick(self.effect, has_effect);
    }

    /// Serialize this instance to NBT matching `MobEffectInstance.CODEC` / `Details.MAP_CODEC`.
    pub fn save_nbt(&self) -> CompoundTag {
        let mut tag = self.save_details_nbt();
        tag.put_string("id", self.effect.identifier());
        tag
    }

    fn save_details_nbt(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        if self.amplifier != 0 {
            tag.put_byte("amplifier", self.amplifier as u8 as i8);
        }
        if self.duration != 0 {
            tag.put_int("duration", self.duration);
        }
        if self.ambient {
            tag.put_boolean("ambient", true);
        }
        if !self.visible {
            tag.put_boolean("show_particles", false);
        }
        tag.put_boolean("show_icon", self.show_icon);
        if let Some(ref hidden) = self.hidden_effect {
            tag.put(
                "hidden_effect".to_string(),
                Tag::Compound(hidden.save_details_nbt()),
            );
        }
        tag
    }

    /// Deserialize from NBT matching `MobEffectInstance.CODEC` / `Details.MAP_CODEC`.
    pub fn load_nbt(tag: &CompoundTag) -> Option<Self> {
        let id_str = tag.get_string("id")?;
        let effect = MobEffectId::by_name(id_str)?;
        Some(Self::load_details_nbt(effect, tag))
    }

    fn load_details_nbt(effect: MobEffectId, tag: &CompoundTag) -> Self {
        // ExtraCodecs.UNSIGNED_BYTE accepts 0..=255 encoded as byte or int.
        let amplifier = if let Some(b) = tag.get_byte("amplifier") {
            i32::from(b as u8)
        } else {
            tag.get_int_or("amplifier", 0)
        };
        let duration = tag.get_int_or("duration", 0);
        let ambient = tag.get_boolean_or("ambient", false);
        let show_particles = tag.get_boolean_or("show_particles", true);
        let show_icon = tag.get_boolean("show_icon").unwrap_or(show_particles);
        let hidden_effect = tag
            .get_compound("hidden_effect")
            .map(|hidden_tag| Box::new(Self::load_details_nbt(effect, hidden_tag)));

        Self::full(
            effect,
            duration,
            amplifier,
            ambient,
            show_particles,
            show_icon,
            hidden_effect,
        )
    }
}

/// Exact Java `MobEffectInstance.equals()` — compares effect, duration, amplifier,
/// ambient, visible, show_icon; ignores `hidden_effect` and `blend_state`.
impl PartialEq for MobEffectInstance {
    fn eq(&self, other: &Self) -> bool {
        self.duration == other.duration
            && self.amplifier == other.amplifier
            && self.ambient == other.ambient
            && self.visible == other.visible
            && self.show_icon == other.show_icon
            && self.effect == other.effect
    }
}

impl Eq for MobEffectInstance {}

/// Exact Java `MobEffectInstance.hashCode()` — ignores `hidden_effect` and `blend_state`.
impl Hash for MobEffectInstance {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.effect.hash(state);
        self.duration.hash(state);
        self.amplifier.hash(state);
        self.ambient.hash(state);
        self.visible.hash(state);
        self.show_icon.hash(state);
    }
}

impl MobEffectInstance {
    /// Exact Java `MobEffectInstance.compareTo(MobEffectInstance o)` with the `32147` cutoff.
    ///
    /// Exposed as an explicit method rather than `Ord`/`PartialOrd` because Java's `compareTo`
    /// ignores amplifier/visibility/icon and duration above `32147`, which is inconsistent with
    /// `Eq` (`a.compare_java(b) == Equal` does not imply `a == b`).
    pub fn compare_java(&self, other: &Self) -> Ordering {
        const UPDATE_CUTOFF: i32 = 32_147;
        let self_color = self.effect.def().color;
        let other_color = other.effect.def().color;

        if (self.duration <= UPDATE_CUTOFF || other.duration <= UPDATE_CUTOFF)
            && (!self.ambient || !other.ambient)
        {
            self.ambient
                .cmp(&other.ambient)
                .then_with(|| {
                    self.is_infinite_duration()
                        .cmp(&other.is_infinite_duration())
                })
                .then_with(|| self.duration.cmp(&other.duration))
                .then_with(|| self_color.cmp(&other_color))
        } else {
            self.ambient
                .cmp(&other.ambient)
                .then_with(|| self_color.cmp(&other_color))
        }
    }
}

/// Exact Java `MobEffectInstance.toString()`.
impl fmt::Display for MobEffectInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.amplifier > 0 {
            write!(
                f,
                "{} x {}, Duration: ",
                self.description_id(),
                self.amplifier.wrapping_add(1)
            )?;
        } else {
            write!(f, "{}, Duration: ", self.description_id())?;
        }

        if self.is_infinite_duration() {
            f.write_str("infinite")?;
        } else {
            write!(f, "{}", self.duration)?;
        }

        if !self.visible {
            f.write_str(", Particles: false")?;
        }
        if !self.show_icon {
            f.write_str(", Show Icon: false")?;
        }

        Ok(())
    }
}
