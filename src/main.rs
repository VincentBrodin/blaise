use std::{env, fs::File, time::Instant};

use blaise::{
    raptor::{query::RaptorQuery, solve},
    spatial::SpatialHash,
};
use gtfs_bin::{consumer::Consumer, models::Coordinate};
use memmap2::MmapOptions;

pub fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        panic!("Missing args");
    }

    let path = args[1].as_str();
    let file = File::open(path).expect("Failed to open file");
    let mmap = unsafe { MmapOptions::new().map(&file).expect("Failed to map memory") };
    let consumer = Consumer::new(&mmap).expect("Failed to parse files header");

    let now = Instant::now();
    let spatial_hash = SpatialHash::new(&consumer);
    println!(
        "Took {:?} to generate spatial hash",
        Instant::now().duration_since(now)
    );

    let now = Instant::now();
    let stops = spatial_hash.get_in_radius_iter(Coordinate::new(59.3404, 18.0381), 500.0);
    println!(
        "Took {:?} to query spatial hash",
        Instant::now().duration_since(now)
    );

    for stop_idx in stops {
        let stop = consumer.stop(stop_idx);
        let name = stop
            .name
            .get()
            .map(|slice| consumer.string(slice))
            .unwrap_or("No name");

        println!("Found: {name}");
    }

    let query = RaptorQuery::new(
        Coordinate::new(59.5832, 17.8807).into(),
        Coordinate::new(59.3404, 18.0381).into(),
    );

    solve(query, &consumer);
}
