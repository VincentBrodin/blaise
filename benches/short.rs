use blaise::{
    raptor::{
        query::{QueryLocation, RaptorQuery},
        state::State,
    },
    spatial::SpatialGrid,
};
use criterion::{Criterion, criterion_group, criterion_main};
use gtfs_bin::{
    consumer::Consumer,
    models::{Coordinate, Time},
};
use memmap2::MmapOptions;
use std::{fs::File, hint::black_box, time::Duration};

fn solve_forward(consumer: &Consumer, spatial: &SpatialGrid, state: &mut State) {
    let from: QueryLocation = Coordinate::new(59.370_136, 18.001_749).into();
    let to: QueryLocation = Coordinate::new(59.335_34, 18.057_737).into();
    let time = Time::from_hms("08:00:00").expect("Failed to parse time");
    let query = RaptorQuery::new(from, to).with_departure(time);
    state.reset();
    let _ = black_box(query.solve(consumer, spatial, state));
}

fn solve_backward(consumer: &Consumer, spatial: &SpatialGrid, state: &mut State) {
    let from: QueryLocation = Coordinate::new(59.370_136, 18.001_749).into();
    let to: QueryLocation = Coordinate::new(59.335_34, 18.057_737).into();
    let time = Time::from_hms("10:00:00").expect("Failed to parse time");
    let query = RaptorQuery::new(from, to).with_arrival(time);
    state.reset();
    let _ = black_box(query.solve(consumer, spatial, state));
}

fn criterion_benchmark(c: &mut Criterion) {
    let file = File::open("bench.gtfs").expect("Failed to open file");
    let mmap = unsafe { MmapOptions::new().map(&file).expect("Failed to map memory") };
    let consumer = Consumer::new(&mmap).expect("Failed to parse files header");
    let spatial_hash = SpatialGrid::new(&consumer);
    let mut state = State::new(&consumer);

    let mut group = c.benchmark_group("Short");

    group.warm_up_time(Duration::from_secs(10));

    group.measurement_time(Duration::from_secs(30));

    group.bench_function("Forward", |b| {
        b.iter(|| solve_forward(&consumer, &spatial_hash, &mut state))
    });

    group.bench_function("Backward", |b| {
        b.iter(|| solve_backward(&consumer, &spatial_hash, &mut state))
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
