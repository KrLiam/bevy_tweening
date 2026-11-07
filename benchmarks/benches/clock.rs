//! Measures one app update stepping many animations, for different
//! distributions of animations across clocks.

#[macro_use]
extern crate criterion;

use std::time::Duration;

use bevy::{
    prelude::*,
    time::{TimePlugin, TimeUpdateStrategy},
};
use bevy_tweening::{lens::TransformPositionLens, *};
use criterion::{BenchmarkId, Criterion, Throughput};

const SIZES: [usize; 2] = [1_000, 10_000];

const FRAME: Duration = Duration::from_millis(16);

type SpawnFn = fn(&mut World, usize);

#[derive(Default)]
struct CustomA;

#[derive(Default)]
struct CustomB;

fn clock<T: Send + Sync + 'static>() -> impl Bundle {
    TweenClock::<T>::default()
}

fn make_anim() -> TweenAnim {
    let tween = Tween::new(
        EaseFunction::Linear,
        Duration::from_secs(1),
        TransformPositionLens {
            start: Vec3::ZERO,
            end: Vec3::ONE,
        },
    )
    // Keep the workload constant: no animation completes during the benchmark.
    .with_repeat_count(RepeatCount::Infinite);
    TweenAnim::new(tween)
}

fn make_app() -> App {
    let mut app = App::default();
    app.add_plugins((TimePlugin, TweeningPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME))
        .init_resource::<Time<CustomA>>()
        .init_resource::<Time<CustomB>>()
        .add_tween_clock::<CustomA>()
        .add_tween_clock::<CustomB>();

    // The delta of a clock remains until it is advanced again.
    app.world_mut()
        .resource_mut::<Time<CustomA>>()
        .advance_by(FRAME);
    app.world_mut()
        .resource_mut::<Time<CustomB>>()
        .advance_by(FRAME);

    app.finish();
    app.cleanup();
    // The first update of the real clock has a zero delta.
    app.update();
    app
}

fn spawn_default(world: &mut World) {
    world.spawn((Transform::default(), make_anim()));
}

fn spawn_custom_a(world: &mut World) {
    world.spawn((Transform::default(), make_anim(), clock::<CustomA>()));
}

fn spawn_custom_b(world: &mut World) {
    world.spawn((Transform::default(), make_anim(), clock::<CustomB>()));
}

/// 50% default clock, 25% `CustomA`, 25% `CustomB`.
fn spawn_mixed(world: &mut World, index: usize) {
    match index % 4 {
        0 | 1 => spawn_default(world),
        2 => spawn_custom_a(world),
        _ => spawn_custom_b(world),
    }
}

fn update_clocks(c: &mut Criterion) {
    let cases: [(&str, SpawnFn); 3] = [
        ("all_default", |world, _| spawn_default(world)),
        ("mixed", spawn_mixed),
        ("all_custom", |world, _| spawn_custom_a(world)),
    ];

    let mut group = c.benchmark_group("clock_update");
    for (name, spawn) in cases {
        for size in SIZES {
            let mut app = make_app();
            for index in 0..size {
                spawn(app.world_mut(), index);
            }
            group.throughput(Throughput::Elements(size as u64));
            group.bench_function(BenchmarkId::new(name, size), |b| b.iter(|| app.update()));
        }
    }
    group.finish();
}

criterion_group!(benches, update_clocks);
criterion_main!(benches);
