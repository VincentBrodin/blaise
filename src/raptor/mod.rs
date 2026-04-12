use std::mem;

use gtfs_bin::{
    consumer::Consumer,
    models::{Opt, Sentinel, StopIdx, Time, TripIdx},
};
use rayon::iter::ParallelExtend;

use crate::{
    raptor::{
        explorer::{explore_transfers, explore_trip_patterns},
        query::{Location, RaptorQuery},
        state::State,
    },
    spatial::SpatialHash,
};

mod explorer;
pub mod query;
mod state;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SequnceIdx(u32);
impl Sentinel for SequnceIdx {
    const NONE: Self = Self(u32::MAX);
}

impl SequnceIdx {
    pub fn as_usize(&self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Parent {
    Transit {
        boarding_p_idx: SequnceIdx,
        trip: TripIdx,
    },

    Transfer {
        from_stop: StopIdx,
    },
    Origin,
}

pub struct Update {
    pub stop: StopIdx,
    pub arrival_time: Time,

    pub parent: Parent,
}

impl Update {
    pub fn new(stop: StopIdx, arrival_time: Time, parent: Parent) -> Self {
        Self {
            stop,
            arrival_time,
            parent,
        }
    }
}

const MAX_ROUNDS: usize = 15;

pub fn solve(query: RaptorQuery, consumer: &Consumer, spatial: &SpatialHash) {
    let mut state = State::new(consumer);

    match query.time_direction {
        query::TimeDirection::Arrival(time) => {
            match query.destination {
                Location::Stop(stop_idx) => {
                    state.marked_stops[stop_idx.as_usize()] = true;
                    state.current_labels[stop_idx.as_usize()] = Opt::new(time);
                }
                Location::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(coordinate, query.search_radius)
                    .filter(|stop_idx| consumer.iter_trips_by_stop(*stop_idx).count() != 0)
                    .for_each(|stop_idx| {
                        state.marked_stops[stop_idx.as_usize()] = true;
                        state.current_labels[stop_idx.as_usize()] = Opt::new(time);
                    }),
            };
        }
        query::TimeDirection::Departure(time) => {
            match query.origin {
                Location::Stop(stop_idx) => {
                    state.marked_stops[stop_idx.as_usize()] = true;
                    state.current_labels[stop_idx.as_usize()] = Opt::new(time);
                }
                Location::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(coordinate, query.search_radius)
                    .filter(|stop_idx| consumer.iter_trips_by_stop(*stop_idx).count() != 0)
                    .for_each(|stop_idx| {
                        state.marked_stops[stop_idx.as_usize()] = true;
                        state.current_labels[stop_idx.as_usize()] = Opt::new(time);
                    }),
            };
        }
    }

    for round in (0..MAX_ROUNDS) {
        mem::swap(&mut state.current_labels, &mut state.previous_labels);
        state.active_trip_patterns.fill(Opt::new(SequnceIdx::NONE));

        for marked_stop in state
            .marked_stops
            .iter()
            .enumerate()
            .filter(|(_, marked)| **marked)
            .map(|(i, _)| StopIdx(i as u32))
        {
            for trip_pattern in consumer.iter_trip_patterns_by_stop(marked_stop) {
                if let Some(p_idx) = consumer
                    .iter_stop_sequence_by_trip_pattern(trip_pattern.idx)
                    .position(|stop| stop.idx.0 == marked_stop.0)
                {
                    let p_idx = SequnceIdx(p_idx as u32);
                    let active_p_idx = state.active_trip_patterns[trip_pattern.idx.as_usize()];
                    match query.time_direction {
                        query::TimeDirection::Arrival(_) => {
                            if let Some(active_p_idx) = active_p_idx.get()
                                && p_idx < active_p_idx
                            {
                                state.active_trip_patterns[trip_pattern.idx.as_usize()] =
                                    Opt::new(p_idx);
                            } else if active_p_idx.is_none() {
                                state.active_trip_patterns[trip_pattern.idx.as_usize()] =
                                    Opt::new(p_idx);
                            }
                        }
                        query::TimeDirection::Departure(_) => {
                            if let Some(active_p_idx) = active_p_idx.get()
                                && p_idx > active_p_idx
                            {
                                state.active_trip_patterns[trip_pattern.idx.as_usize()] =
                                    Opt::new(p_idx);
                            } else if active_p_idx.is_none() {
                                state.active_trip_patterns[trip_pattern.idx.as_usize()] =
                                    Opt::new(p_idx);
                            }
                        }
                    }
                }
            }
        }
        state.marked_stops.fill(false);

        match query.time_direction {
            query::TimeDirection::Arrival(_) => todo!(),
            query::TimeDirection::Departure(_) => {
                explore_trip_patterns(consumer, &mut state);
                state.apply_updates(round);

                explore_transfers(consumer, &mut state);
                state.apply_updates(round);
            }
        }

        let target_tau_star = state.target_tau_star.get().unwrap_or(Time(u32::MAX));
        state
            .target_stops
            .iter()
            .filter_map(|target_stop| {
                state.tau_star[target_stop.as_usize()]
                    .get()
                    .map(|tau_star| (target_stop, tau_star))
            })
            .for_each(|(target_stop, tau_star)| {
                let improvement = match query.time_direction {
                    query::TimeDirection::Arrival(_) => tau_star > target_tau_star,
                    query::TimeDirection::Departure(_) => tau_star < target_tau_star,
                };

                if target_tau_star.is_none() || improvement {
                    state.target_tau_star = Opt::new(tau_star);
                    state.target_best_stop = Opt::new(*target_stop);
                    state.target_best_round = Some(round);
                }
            });
    }

    if let Some(stop) = state.target_best_stop.get()
        && let Some(round) = state.target_best_round
    {
        println!("Found route in {round} round(s)");
    } else {
        println!("Failed to find route");
    }
}
