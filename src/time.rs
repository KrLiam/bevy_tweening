//! Per-animation clocks.
//!
//! By default a [`TweenAnim`] is stepped by the generic [`Time`] clock, which
//! during the [`Update`] schedule is the [`Virtual`] clock. To use another
//! clock, add a [`TweenClock`] component to the entity owning the
//! [`TweenAnim`].
//!
//! The [`()`], [`Virtual`], [`Real`] and [`Fixed`] clocks are registered by the
//! [`TweeningPlugin`]. Custom clocks need to be registered with
//! [`TweenTimePluginExt::add_tween_clock()`].
//!
//! [`TweenAnim`]: crate::TweenAnim
//! [`TweeningPlugin`]: crate::TweeningPlugin
//! [`()`]: unit

use std::{any::TypeId, fmt, marker::PhantomData};

use bevy::{
    ecs::{
        component::ComponentId, lifecycle::HookContext, schedule::ScheduleLabel,
        world::DeferredWorld,
    },
    prelude::*,
};

use crate::plugin::{animator_system, AnimationSystem};

/// Selects the clock stepping the [`TweenAnim`] on the same entity.
///
/// This component is optional. Without it, the animation is stepped by the
/// [`Virtual`] clock by default.
///
/// An entity has at most one clock. Inserting a [`TweenClock`] of another type
/// removes the previous one, and removing it returns the animation to the
/// default clock. If a single bundle contains several clocks, which one is kept
/// is unspecified.
///
/// The clock `T` must be registered, otherwise the animation is never stepped.
/// See [`TweenTimePluginExt::add_tween_clock()`].
///
/// This component is not removed when the animation completes, so any
/// [`TweenAnim`] later inserted on the same entity uses the same clock.
///
/// # Example
///
/// ```
/// # use bevy::prelude::*;
/// # use bevy_tweening::*;
/// # fn make_tweenable() -> Tween { unimplemented!() }
/// fn my_system(mut commands: Commands) {
///     // This animation keeps playing while the Virtual clock is paused.
///     commands.spawn((
///         Transform::default(),
///         TweenAnim::new(make_tweenable()),
///         TweenClock::<Real>::default(),
///     ));
/// }
/// ```
///
/// [`TweenAnim`]: crate::TweenAnim
#[derive(Component)]
#[component(on_insert = on_clock_insert, on_remove = on_clock_remove)]
pub struct TweenClock<T: Send + Sync + 'static>(PhantomData<fn() -> T>);

impl<T: Send + Sync + 'static> Default for TweenClock<T> {
    #[inline]
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: Send + Sync + 'static> Clone for TweenClock<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Send + Sync + 'static> Copy for TweenClock<T> {}

impl<T: Send + Sync + 'static> fmt::Debug for TweenClock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TweenClock<{}>", std::any::type_name::<T>())
    }
}

/// The [`TweenClock`] currently active on an entity, identified by the
/// [`ComponentId`] of its concrete type.
///
/// This allows excluding all clocked animations from the default clock, which
/// can't be expressed with a filter on the generic [`TweenClock`], and finding
/// the previous clock to remove when another one is inserted.
#[derive(Component, Clone, Copy)]
pub(crate) struct ActiveTweenClock(ComponentId);

// The clock hooks queue commands instead of reading the entity right away. Hooks
// of the same bundle all run before any of those commands, so reading from the
// hook wouldn't see another clock inserted by that bundle, and both would stay.
// The commands are silenced because the entity may be despawned by then.

fn on_clock_insert(mut world: DeferredWorld, context: HookContext) {
    let clock = context.component_id;
    world
        .commands()
        .entity(context.entity)
        .queue_silenced(move |mut entity: EntityWorldMut| {
            let previous = entity.get::<ActiveTweenClock>().map(|active| active.0);
            // Update this first, so that the hook removing the previous clock
            // doesn't consider that clock to be the active one.
            entity.insert(ActiveTweenClock(clock));
            if let Some(previous) = previous.filter(|previous| *previous != clock) {
                entity.remove_by_id(previous);
            }
        });
}

fn on_clock_remove(mut world: DeferredWorld, context: HookContext) {
    let clock = context.component_id;
    world
        .commands()
        .entity(context.entity)
        .queue_silenced(move |mut entity: EntityWorldMut| {
            if entity.get::<ActiveTweenClock>().map(|active| active.0) == Some(clock) {
                entity.remove::<ActiveTweenClock>();
            }
        });
}

/// Clocks which already have an animation system.
///
/// Bevy doesn't deduplicate systems, so without this registering a clock twice
/// would step its animations twice per frame. This is only accessed during
/// registration.
#[derive(Resource, Default)]
struct RegisteredTweenClocks(Vec<TypeId>);

/// Extension trait to register clocks on an [`App`].
pub trait TweenTimePluginExt {
    /// Register a clock that steps animations on the [`Update`] schedule.
    ///
    /// See [`add_tween_clock_in()`](Self::add_tween_clock_in) for details.
    fn add_tween_clock<T: Default + Send + Sync + 'static>(&mut self) -> &mut Self;

    /// Register a clock that steps animations on the given schedule.
    ///
    /// This adds a system to the [`AnimationSystem::AnimationUpdate`] set of
    /// that schedule, which steps the animations having a matching
    /// [`TweenClock`] by [`Time::delta()`]. Registering an already registered
    /// clock does nothing, even if the schedule is different.
    ///
    /// The [`Time<T>`] resource must exist when the system runs, otherwise the
    /// system panics. If your code advances that clock, it should do so before
    /// [`AnimationSystem::AnimationUpdate`].
    ///
    /// The order in which the systems of different clocks run is unspecified.
    fn add_tween_clock_in<T: Default + Send + Sync + 'static>(
        &mut self,
        schedule: impl ScheduleLabel,
    ) -> &mut Self;
}

impl TweenTimePluginExt for App {
    fn add_tween_clock<T: Default + Send + Sync + 'static>(&mut self) -> &mut Self {
        self.add_tween_clock_in::<T>(Update)
    }

    fn add_tween_clock_in<T: Default + Send + Sync + 'static>(
        &mut self,
        schedule: impl ScheduleLabel,
    ) -> &mut Self {
        let type_id = TypeId::of::<T>();
        let mut registered = self
            .world_mut()
            .get_resource_or_init::<RegisteredTweenClocks>();
        if registered.0.contains(&type_id) {
            return self;
        }
        registered.0.push(type_id);

        self.add_systems(
            schedule,
            animator_system::<T>.in_set(AnimationSystem::AnimationUpdate),
        )
    }
}
