use blaise::{
    gtfs::GtfsReader,
    raptor::{Allocator, Location, Raptor},
    realtime::Realtime,
    repository::Repository,
    shared::{Coordinate, Time},
};
use criterion::{Criterion, criterion_group, criterion_main};
use std::{env, hint::black_box, path::Path, time::Duration};

fn short_solve(repository: &Repository, realtime: &Realtime, allocator: &mut Allocator) {
    let from = Location::from_coordinate(Coordinate::from((59.370_136, 18.001_749)));
    let to = Location::from_coordinate(Coordinate::from((59.335_34, 18.057_737)));
    let time = Time::from_seconds(28800);
    let raptor = Raptor::new(repository, realtime, from, to);
    allocator.reset();
    let _ = black_box(raptor.departure_at(time).solve_with_allocator(allocator));
}

fn long_solve(repository: &Repository, realtime: &Realtime, allocator: &mut Allocator) {
    let from = Location::from_coordinate(Coordinate::from((59.196_198, 17.628_841)));
    let to = Location::from_coordinate(Coordinate::from((59.857_834, 17.629_814)));
    let time = Time::from_seconds(28800);
    let raptor = Raptor::new(repository, realtime, from, to);
    allocator.reset();
    let _ = black_box(raptor.departure_at(time).solve_with_allocator(allocator));
}
fn criterion_benchmark(c: &mut Criterion) {
    let gtfs_data_path = match env::var("GTFS_DATA_PATH") {
        Ok(path_str) => Path::new(&path_str).to_owned(),
        Err(err) => {
            println!("Missing GTFS_DATA_PATH environment variable: {err}");
            return;
        }
    };

    let gtfs = GtfsReader::new()
        .from_zip_cache(gtfs_data_path)
        .expect("Failed to create GTFS reader")
        .par_read()
        .expect("Failed to read GTFS data");
    let repository = Repository::new().load_gtfs(gtfs);
    let realtime = Realtime::new(&repository);

    let mut allocator = Allocator::new(&repository);

    let mut group = c.benchmark_group("Routing");

    group.warm_up_time(Duration::from_secs(10));

    group.measurement_time(Duration::from_secs(30));

    group.bench_function("Short route solve", |b| {
        b.iter(|| short_solve(&repository, &realtime, &mut allocator))
    });

    group.bench_function("Long route solve", |b| {
        b.iter(|| long_solve(&repository, &realtime, &mut allocator))
    });

    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
