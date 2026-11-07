use bevy::prelude::*;

use crate::{
    time::ActiveTweenClock, AnimCompletedEvent, CycleCompletedEvent, TweenAnim, TweenClock,
    TweenResolver, TweenTimePluginExt,
};

/// Plugin to register the 🍃 Bevy Tweening animation framework.
///
/// This plugin registers the common resources and events used by 🍃 Bevy
/// Tweening as well as the core animation systems which step all pending
/// tweenable animations. Those systems run in the
/// [`AnimationSystem::AnimationUpdate`] system set, during the [`Update`]
/// schedule, except for the animations using the [`Fixed`] clock which are
/// stepped during the [`FixedUpdate`] schedule.
///
/// The default clock, as well as the [`Virtual`], [`Real`] and [`Fixed`] clocks,
/// are registered by this plugin. See [`TweenClock`] to select a clock for an
/// animation.
///
/// ```no_run
/// use bevy::prelude::*;
/// use bevy_tweening::*;
///
/// App::default()
///     .add_plugins(DefaultPlugins)
///     .add_plugins(TweeningPlugin)
///     .run();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct TweeningPlugin;

impl Plugin for TweeningPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TweenResolver>()
            .add_message::<CycleCompletedEvent>()
            .add_message::<AnimCompletedEvent>()
            .add_systems(
                Update,
                default_animator_system.in_set(AnimationSystem::AnimationUpdate),
            )
            .add_tween_clock::<()>()
            .add_tween_clock::<Virtual>()
            .add_tween_clock::<Real>()
            // Time<Fixed> only advances during the fixed loop, so reading it from Update would
            // return the last timestep no matter how many fixed ticks ran during the frame.
            .add_tween_clock_in::<Fixed>(FixedUpdate);
    }
}

/// Label enum for the systems relating to animations
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, SystemSet)]
#[non_exhaustive]
pub enum AnimationSystem {
    /// Steps all animations. This executes during the [`Update`] schedule, and
    /// the schedule of any clock registered with
    /// [`TweenTimePluginExt::add_tween_clock_in()`], like [`FixedUpdate`] for
    /// the [`Fixed`] clock.
    AnimationUpdate,
}

/// Core animation system ticking all queued animations without a
/// [`TweenClock`], using a value of the animation timestep `delta_time` equal
/// to [`Time::delta()`].
pub(crate) fn default_animator_system(world: &mut World) {
    let delta_time = world.resource::<Time>().delta();
    TweenAnim::step_filtered::<Without<ActiveTweenClock>>(world, delta_time);
}

/// Animation system ticking all queued animations whose [`TweenClock`] selects
/// the clock `T`, using a value of the animation timestep `delta_time` equal to
/// the [`Time::delta()`] of that clock.
pub(crate) fn animator_system<T: Default + Send + Sync + 'static>(world: &mut World) {
    let delta_time = world.resource::<Time<T>>().delta();
    TweenAnim::step_filtered::<With<TweenClock<T>>>(world, delta_time);
}

#[cfg(test)]
mod tests {
    use std::{
        ops::DerefMut,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };

    use bevy::time::{TimePlugin, TimeUpdateStrategy};

    use crate::{lens::TransformPositionLens, test_utils::TestEnv, time::ActiveTweenClock, *};

    #[test]
    fn app() {
        let mut app = App::default();
        app.add_plugins((TimePlugin, TweeningPlugin));
        app.finish();
        app.update();
    }

    #[test]
    fn custom_target_entity() {
        let tween = Tween::new(
            EaseMethod::EaseFunction(EaseFunction::Linear),
            Duration::from_secs(1),
            TransformPositionLens {
                start: Vec3::ZERO,
                end: Vec3::ONE,
            },
        )
        .with_cycle_completed_event(true);
        let mut env = TestEnv::<Transform>::new(tween);

        env.step_all(Duration::ZERO);
        let transform = env.component_mut();
        assert!(transform.translation.abs_diff_eq(Vec3::ZERO, 1e-5));

        env.step_all(Duration::from_millis(500));
        let transform = env.component_mut();
        assert!(transform.translation.abs_diff_eq(Vec3::splat(0.5), 1e-5));
    }

    #[test]
    fn change_detect_component() {
        let tween = Tween::new(
            EaseMethod::default(),
            Duration::from_secs(1),
            TransformPositionLens {
                start: Vec3::ZERO,
                end: Vec3::ONE,
            },
        )
        .with_cycle_completed_event(true);

        let mut env = TestEnv::<Transform>::new(tween);

        // After being inserted, components are always considered changed
        let transform = env.component_mut();
        assert!(transform.is_changed());

        env.step_all(Duration::ZERO);

        let anim = env.anim().unwrap();
        assert_eq!(anim.playback_state, PlaybackState::Playing);
        assert_eq!(anim.tweenable.cycles_completed(), 0);
        let transform = env.component_mut();
        assert!(transform.is_changed());
        assert!(transform.translation.abs_diff_eq(Vec3::ZERO, 1e-5));

        env.step_all(Duration::from_millis(500));

        assert_eq!(env.event_count::<CycleCompletedEvent>(), 0);
        let anim = env.anim().unwrap();
        assert_eq!(anim.playback_state, PlaybackState::Playing);
        assert_eq!(anim.tweenable.cycles_completed(), 0);
        let transform = env.component_mut();
        assert!(transform.is_changed());
        assert!(transform.translation.abs_diff_eq(Vec3::splat(0.5), 1e-5));

        env.step_all(Duration::from_millis(500));

        // The animation is done now, and was deleted from the animator queue.
        // The final state was still applied before deleting the animation,
        // so the component is changed.

        assert_eq!(env.event_count::<CycleCompletedEvent>(), 1);
        let anim = env.anim();
        assert!(anim.is_none()); // done and deleted
        let transform = env.component_mut();
        assert!(transform.is_changed());
        assert!(transform.translation.abs_diff_eq(Vec3::ONE, 1e-5));

        // We can continue to tick as much as we want, this doesn't change anything
        env.step_all(Duration::from_millis(100));

        assert_eq!(env.event_count::<CycleCompletedEvent>(), 0);
        let anim = env.anim();
        assert!(anim.is_none()); // done and deleted
        let transform = env.component_mut();
        assert!(!transform.is_changed());
        assert!(transform.translation.abs_diff_eq(Vec3::ONE, 1e-5));
    }

    #[derive(Debug, Default, Clone, Copy, Component)]
    struct DummyComponent {
        value: f32,
    }

    /// Test [`Lens`] which only access mutably the target component if `defer`
    /// is `true`.
    struct ConditionalDeferLens {
        pub defer: Arc<AtomicBool>,
    }

    impl Lens<DummyComponent> for ConditionalDeferLens {
        fn lerp(&mut self, mut target: Mut<DummyComponent>, ratio: f32) {
            if self.defer.load(Ordering::SeqCst) {
                target.deref_mut().value += ratio;
            }
        }
    }

    #[test]
    fn change_detect_component_conditional() {
        let defer = Arc::new(AtomicBool::new(false));
        let tween = Tween::new(
            EaseMethod::default(),
            Duration::from_secs(1),
            ConditionalDeferLens {
                defer: Arc::clone(&defer),
            },
        )
        .with_cycle_completed_event(true);

        let mut env = TestEnv::<DummyComponent>::new(tween);

        // After being inserted, components are always considered changed
        let component = env.component_mut();
        assert!(component.is_changed());

        assert!(!defer.load(Ordering::SeqCst));

        // Mutation disabled
        env.step_all(Duration::ZERO);

        let anim = env.anim().unwrap();
        assert_eq!(anim.playback_state, PlaybackState::Playing);
        assert_eq!(anim.tweenable.cycles_completed(), 0);
        let component = env.component_mut();
        assert!(!component.is_changed());
        assert!(((*component).value - 0.).abs() <= 1e-5);

        // Zero-length tick should not change the component
        env.step_all(Duration::ZERO);

        let anim = env.anim().unwrap();
        assert_eq!(anim.playback_state, PlaybackState::Playing);
        assert_eq!(anim.tweenable.cycles_completed(), 0);
        let component = env.component_mut();
        assert!(!component.is_changed());
        assert!(((*component).value - 0.).abs() <= 1e-5);

        // New tick, but lens mutation still disabled
        env.step_all(Duration::from_millis(200));

        let anim = env.anim().unwrap();
        assert_eq!(anim.playback_state, PlaybackState::Playing);
        assert_eq!(anim.tweenable.cycles_completed(), 0);
        let component = env.component_mut();
        assert!(!component.is_changed());
        assert!(((*component).value - 0.).abs() <= 1e-5);

        // Enable lens mutation
        defer.store(true, Ordering::SeqCst);

        // The current time is already at t=0.2s, so even if we don't increment it, for
        // a tween duration of 1s the ratio is t=0.2, so the lens will actually
        // increment the component's value.
        env.step_all(Duration::ZERO);

        let anim = env.anim().unwrap();
        assert_eq!(anim.playback_state, PlaybackState::Playing);
        assert_eq!(anim.tweenable.cycles_completed(), 0);
        let component = env.component_mut();
        assert!(component.is_changed());
        assert!(((*component).value - 0.2).abs() <= 1e-5);

        // 0.2s + 0.3s = 0.5s
        // t = 0.5s / 1s = 0.5
        // value += 0.5
        // value == 0.7
        env.step_all(Duration::from_millis(300));

        let anim = env.anim().unwrap();
        assert_eq!(anim.playback_state, PlaybackState::Playing);
        assert_eq!(anim.tweenable.cycles_completed(), 0);
        let component = env.component_mut();
        assert!(component.is_changed());
        assert!(((*component).value - 0.7).abs() <= 1e-5);
    }

    struct NoopLens;

    impl<T> Lens<T> for NoopLens {
        fn lerp(&mut self, _target: Mut<T>, _ratio: f32) {}
    }

    #[derive(Default)]
    struct CustomClock;

    #[derive(Default)]
    struct OtherClock;

    /// Delta time of the virtual and real clocks on each update of [`clock_app`].
    const FRAME: Duration = Duration::from_millis(10);
    const CUSTOM_DELTA: Duration = Duration::from_millis(100);
    const OTHER_DELTA: Duration = Duration::from_millis(7);

    /// Long enough that no animation completes during a test.
    const ANIM_DURATION: Duration = Duration::from_secs(100);

    /// Create an app with custom clocks and a fixed frame time. The first update
    /// of the real clock always has a zero delta, which is consumed here. The
    /// custom clocks keep the same delta on all updates.
    fn clock_app() -> App {
        let mut app = App::default();
        app.add_plugins((TimePlugin, TweeningPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
            .init_resource::<Time<CustomClock>>()
            .init_resource::<Time<OtherClock>>()
            .add_tween_clock::<CustomClock>()
            .add_tween_clock::<OtherClock>();
        app.update();
        app.world_mut()
            .resource_mut::<Time<CustomClock>>()
            .advance_by(CUSTOM_DELTA);
        app.world_mut()
            .resource_mut::<Time<OtherClock>>()
            .advance_by(OTHER_DELTA);
        app
    }

    fn spawn_anim(app: &mut App, clock: impl Bundle) -> Entity {
        let tween =
            Tween::new::<DummyComponent, NoopLens>(EaseFunction::Linear, ANIM_DURATION, NoopLens);
        app.world_mut()
            .spawn((DummyComponent::default(), TweenAnim::new(tween), clock))
            .id()
    }

    fn elapsed(app: &App, entity: Entity) -> Duration {
        app.world()
            .get::<TweenAnim>(entity)
            .unwrap()
            .tweenable()
            .elapsed()
    }

    fn has<C: Component>(app: &App, entity: Entity) -> bool {
        app.world().entity(entity).contains::<C>()
    }

    #[test]
    fn clocks_are_independent() {
        let mut app = clock_app();
        let default_anim = spawn_anim(&mut app, ());
        let custom_anim = spawn_anim(&mut app, TweenClock::<CustomClock>::default());
        let other_anim = spawn_anim(&mut app, TweenClock::<OtherClock>::default());

        app.update();

        assert_eq!(elapsed(&app, default_anim), FRAME);
        assert_eq!(elapsed(&app, custom_anim), CUSTOM_DELTA);
        assert_eq!(elapsed(&app, other_anim), OTHER_DELTA);
    }

    #[test]
    fn builtin_clocks() {
        let mut app = clock_app();
        let explicit_default = spawn_anim(&mut app, TweenClock::<()>::default());
        let virt = spawn_anim(&mut app, TweenClock::<Virtual>::default());
        let real = spawn_anim(&mut app, TweenClock::<Real>::default());

        app.update();
        assert_eq!(elapsed(&app, explicit_default), FRAME);
        assert_eq!(elapsed(&app, virt), FRAME);
        assert_eq!(elapsed(&app, real), FRAME);

        // Only the real clock keeps running while the virtual clock is paused.
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.update();
        assert_eq!(elapsed(&app, explicit_default), FRAME);
        assert_eq!(elapsed(&app, virt), FRAME);
        assert_eq!(elapsed(&app, real), FRAME * 2);
    }

    #[test]
    fn replace_and_remove_clock() {
        let mut app = clock_app();
        let anim = spawn_anim(&mut app, TweenClock::<CustomClock>::default());

        // Reinserting the same clock keeps it active.
        app.world_mut()
            .entity_mut(anim)
            .insert(TweenClock::<CustomClock>::default());
        app.update();
        assert_eq!(elapsed(&app, anim), CUSTOM_DELTA);

        // Inserting another clock removes the previous one.
        app.world_mut()
            .entity_mut(anim)
            .insert(TweenClock::<OtherClock>::default());
        assert!(!has::<TweenClock<CustomClock>>(&app, anim));
        app.update();
        assert_eq!(elapsed(&app, anim), CUSTOM_DELTA + OTHER_DELTA);

        // Removing the clock returns to the default one.
        app.world_mut()
            .entity_mut(anim)
            .remove::<TweenClock<OtherClock>>();
        assert!(!has::<ActiveTweenClock>(&app, anim));
        app.update();
        assert_eq!(elapsed(&app, anim), CUSTOM_DELTA + OTHER_DELTA + FRAME);
    }

    #[test]
    fn two_clocks_in_one_bundle_keeps_one() {
        let mut app = clock_app();
        let anim = spawn_anim(
            &mut app,
            (
                TweenClock::<CustomClock>::default(),
                TweenClock::<OtherClock>::default(),
            ),
        );

        let has_custom = has::<TweenClock<CustomClock>>(&app, anim);
        assert_ne!(has_custom, has::<TweenClock<OtherClock>>(&app, anim));

        app.update();
        let expected = if has_custom {
            CUSTOM_DELTA
        } else {
            OTHER_DELTA
        };
        assert_eq!(elapsed(&app, anim), expected);
    }

    #[test]
    fn double_registration_steps_once() {
        let mut app = clock_app();
        app.add_tween_clock::<CustomClock>()
            .add_tween_clock_in::<CustomClock>(PostUpdate);
        let anim = spawn_anim(&mut app, TweenClock::<CustomClock>::default());

        app.update();

        assert_eq!(elapsed(&app, anim), CUSTOM_DELTA);
    }
}
