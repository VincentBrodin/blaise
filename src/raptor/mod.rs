use std::mem;

use gtfs_bin::{
    consumer::Consumer,
    models::{Opt, Sentinel, StopIdx, Time},
};

use crate::{
    raptor::query::{Location, RaptorQuery},
    spatial::SpatialHash,
};

pub mod query;

const MAX_ROUNDS: usize = 15;

pub fn solve(query: RaptorQuery, consumer: &Consumer, spatial: &SpatialHash) {
    let tau_star = vec![Opt::new(Time::NONE); consumer.stops.len()];
    let mut current_labels = vec![Opt::new(Time::NONE); consumer.stops.len()];
    let mut previous_labels = vec![Opt::new(Time::NONE); consumer.stops.len()];
    let mut marked_stops = vec![false; consumer.stops.len()];
    let mut active = vec![Opt::new(StopIdx::NONE); consumer.stops.len()];
    let mut round = 0;
    let target_tau_star: Time;
    let target_stops: Vec<StopIdx> = Vec::new();
    let best_target_stop = Opt::new(StopIdx::NONE);
    let best_round: Option<usize> = None;

    match query.time_direction {
        query::TimeDirection::Arrival(time) => {
            match query.destination {
                Location::Stop(stop_idx) => {
                    marked_stops[stop_idx.to_usize()] = true;
                    current_labels[stop_idx.to_usize()] = Opt::new(time);
                }
                Location::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(coordinate, query.search_radius)
                    .filter(|stop_idx| consumer.iter_stop_trips(*stop_idx).count() != 0)
                    .for_each(|stop_idx| {
                        marked_stops[stop_idx.to_usize()] = true;
                        current_labels[stop_idx.to_usize()] = Opt::new(time);
                    }),
            };
            target_tau_star = Time(u32::MIN);
        }
        query::TimeDirection::Departure(time) => {
            match query.origin {
                Location::Stop(stop_idx) => {
                    marked_stops[stop_idx.to_usize()] = true;
                    current_labels[stop_idx.to_usize()] = Opt::new(time);
                }
                Location::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(coordinate, query.search_radius)
                    .filter(|stop_idx| consumer.iter_stop_trips(*stop_idx).count() != 0)
                    .for_each(|stop_idx| {
                        marked_stops[stop_idx.to_usize()] = true;
                        current_labels[stop_idx.to_usize()] = Opt::new(time);
                    }),
            };
            target_tau_star = Time(u32::MAX);
        }
    }

    loop {
        if round >= MAX_ROUNDS {
            return;
        }
        mem::swap(&mut current_labels, &mut previous_labels);
        active.fill(Opt::new(StopIdx::NONE));

        marked_stops
            .iter()
            .enumerate()
            .filter(|(_, marked)| **marked)
            .map(|(i, _)| StopIdx(i as u32))
            .for_each(|stop_idx| {
                // Add lookup in gtfs-bin for finding trip patterns based on if they serve a stop
                // for trip_pattern in consumer.iter_inbound_transfers(idx);
            });

        round += 1;
    }

    println!(
        "Solveing query for network with {} stops and {} trips",
        consumer.stops.len(),
        consumer.trips.len()
    );
}
