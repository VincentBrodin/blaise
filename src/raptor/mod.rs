use gtfs_bin::consumer::Consumer;

use crate::raptor::query::RaptorQuery;

pub mod query;

pub fn solve(query: RaptorQuery, consumer: &Consumer) {
    println!(
        "Solveing query for network with {} stops and {} trips",
        consumer.stops.len(),
        consumer.trips.len()
    );
}
