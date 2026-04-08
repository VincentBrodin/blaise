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
    let spatial_hash = SpatialHash::new(&consumer);

    let query = RaptorQuery::new(
        Coordinate::new(59.5832, 17.8807).into(),
        Coordinate::new(59.3404, 18.0381).into(),
    );

    solve(query, &consumer, &spatial_hash);
}
