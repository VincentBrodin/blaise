use std::mem;

use gtfs_bin::{
    consumer::Consumer,
    models::{Coordinate, Delay, Duration, Opt, Sentinel, StopIdx, Time, TripIdx},
};

use crate::{
    raptor::{
        explorer::{
            explore_transfers, explore_transfers_reverse, explore_trip_patterns,
            explore_trip_patterns_reverse,
        },
        itinerary::Itinerary,
        query::{QueryLocation, RaptorQuery},
        state::State,
    },
    spatial::SpatialHash,
};

mod explorer;
pub mod itinerary;
pub mod query;
pub mod state;

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
pub struct LiveTime {
    pub scheduled: Time,
    pub actual: Time,
}

impl LiveTime {
    pub fn delay(&self) -> Delay {
        Delay((self.actual.0 as i64 - self.scheduled.0 as i64) as i16)
    }

    pub fn scheduled_only(time: Time) -> Self {
        Self {
            scheduled: time,
            actual: time,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Parent {
    Transit {
        boarding_p: SequnceIdx,
        alighting_p: SequnceIdx,
        trip: TripIdx,
        departure_time: LiveTime,
        arrival_time: LiveTime,
    },

    Transfer {
        from_stop: StopIdx,
        departure_time: LiveTime,
        arrival_time: LiveTime,
    },
    Walk {
        from_stop: StopIdx,
        departure_time: LiveTime,
        arrival_time: LiveTime,
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

pub fn solve(
    query: RaptorQuery,
    consumer: &Consumer,
    spatial: &SpatialHash,
    state: &mut State,
) -> Result<Itinerary, crate::Error> {
    let is_arrival = matches!(query.time_direction, query::TimeDirection::Arrival(_));

    let itinerary = solve_core(&query, consumer, spatial, state)?;

    if is_arrival && let Some(first_leg) = itinerary.legs.first() {
        let optimal_departure = first_leg.departure_time.scheduled;

        let forward_query = RaptorQuery {
            origin: query.origin,
            destination: query.destination,
            time_direction: query::TimeDirection::Departure(optimal_departure),
            search_radius: query.search_radius,
            date: query.date,
        };

        state.reset();
        return solve_core(&forward_query, consumer, spatial, state);
    }

    Ok(itinerary)
}

fn solve_core(
    query: &RaptorQuery,
    consumer: &Consumer,
    spatial: &SpatialHash,
    state: &mut State,
) -> Result<Itinerary, crate::Error> {
    match query.time_direction {
        query::TimeDirection::Arrival(time) => {
            match query.destination {
                QueryLocation::Stop(stop) => {
                    state.marked_stops[stop.as_usize()] = true;
                    state.current_labels[stop.as_usize()] = Opt::new(time);
                    state.tau_star[stop.as_usize()] = Opt::new(time);
                    let parent_idx = state.calc_parent_idx(0, stop);
                    state.parents[parent_idx] = Some(Parent::Origin);
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops.iter().copied() {
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()] = Opt::new(time);
                        state.tau_star[stop.as_usize()] = Opt::new(time);
                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(consumer, coordinate, query.search_radius)
                    .filter(|stop| consumer.iter_trips_by_stop(*stop).count() != 0)
                    .filter_map(|stop| consumer.stop(stop).coordinate.get().map(|c| (stop, c)))
                    .for_each(|(stop, to_coordinate)| {
                        let time_to_walk = time_to_walk(coordinate, to_coordinate);
                        let arr_time = Time(time.0 - time_to_walk.0);
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()] = Opt::new(arr_time);
                        state.tau_star[stop.as_usize()] = Opt::new(arr_time);
                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }),
            };

            match query.origin {
                QueryLocation::Stop(stop) => {
                    state.target_stops.push((stop, Duration(0)));
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops.iter().copied() {
                        state.target_stops.push((stop, Duration(0)));
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(consumer, coordinate, query.search_radius)
                    .filter(|stop_idx| consumer.iter_trips_by_stop(*stop_idx).count() != 0)
                    .filter_map(|stop_idx| {
                        consumer
                            .stop(stop_idx)
                            .coordinate
                            .get()
                            .map(|c| (stop_idx, c))
                    })
                    .for_each(|(stop_idx, to_coordinate)| {
                        let walk_time = time_to_walk(coordinate, to_coordinate);
                        state.target_stops.push((stop_idx, walk_time));
                    }),
            }
        }
        query::TimeDirection::Departure(time) => {
            match query.origin {
                QueryLocation::Stop(stop) => {
                    state.marked_stops[stop.as_usize()] = true;
                    state.current_labels[stop.as_usize()] = Opt::new(time);
                    state.tau_star[stop.as_usize()] = Opt::new(time);
                    let parent_idx = state.calc_parent_idx(0, stop);
                    state.parents[parent_idx] = Some(Parent::Origin);
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops.iter().copied() {
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()] = Opt::new(time);
                        state.tau_star[stop.as_usize()] = Opt::new(time);
                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(consumer, coordinate, query.search_radius)
                    .filter(|stop| consumer.iter_trips_by_stop(*stop).count() != 0)
                    .filter_map(|stop| consumer.stop(stop).coordinate.get().map(|c| (stop, c)))
                    .for_each(|(stop, to_coordinate)| {
                        let time_to_walk = time_to_walk(coordinate, to_coordinate);
                        let arr_time = Time(time.0 + time_to_walk.0);
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()] = Opt::new(arr_time);
                        state.tau_star[stop.as_usize()] = Opt::new(arr_time);
                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }),
            };

            match query.destination {
                QueryLocation::Stop(stop) => {
                    state.target_stops.push((stop, Duration(0)));
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops.iter().copied() {
                        state.target_stops.push((stop, Duration(0)));
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .get_in_radius_iter(consumer, coordinate, query.search_radius)
                    .filter(|stop| consumer.iter_trips_by_stop(*stop).count() != 0)
                    .filter_map(|stop| consumer.stop(stop).coordinate.get().map(|c| (stop, c)))
                    .for_each(|(stop, to_coordinate)| {
                        let walk_time = time_to_walk(coordinate, to_coordinate);
                        state.target_stops.push((stop, walk_time));
                    }),
            }
        }
    }

    match query.time_direction {
        query::TimeDirection::Arrival(_) => {
            explore_transfers_reverse(query, consumer, spatial, state)
        }
        query::TimeDirection::Departure(_) => explore_transfers(query, consumer, spatial, state),
    }

    state.apply_updates(0, query.time_direction);

    for (target_stop, duration) in state.target_stops.iter() {
        if let Some(label_time) = state.current_labels[target_stop.as_usize()].get() {
            let true_time = match query.time_direction {
                query::TimeDirection::Arrival(_) => Time(label_time.0 - duration.0),
                query::TimeDirection::Departure(_) => Time(label_time.0 + duration.0),
            };
            state.target_tau_star = Opt::new(true_time);
            state.target_best_stop = Opt::new(*target_stop);
            state.target_best_round = Some(0);
        }
    }

    for round in 1..MAX_ROUNDS {
        if !state.marked_stops.iter().any(|&marked| marked) {
            break;
        }

        mem::swap(&mut state.current_labels, &mut state.previous_labels);
        state.current_labels.fill(Opt::new(Time::NONE));

        state
            .active_trip_patterns
            .fill(Opt::new(SequnceIdx(u32::MAX)));
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
                                && p_idx > active_p_idx
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
                                && p_idx < active_p_idx
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
            query::TimeDirection::Arrival(_) => {
                explore_trip_patterns_reverse(query, consumer, state);
                state.apply_updates(round, query.time_direction);

                explore_transfers_reverse(query, consumer, spatial, state);
                state.apply_updates(round, query.time_direction);
            }
            query::TimeDirection::Departure(_) => {
                explore_trip_patterns(query, consumer, state);
                state.apply_updates(round, query.time_direction);

                explore_transfers(query, consumer, spatial, state);
                state.apply_updates(round, query.time_direction);
            }
        }

        for (target_stop, duration) in state.target_stops.iter() {
            if let Some(label_time) = state.current_labels[target_stop.as_usize()].get() {
                let current_best = state.target_tau_star.get();
                let true_time = match query.time_direction {
                    query::TimeDirection::Arrival(_) => Time(label_time.0 - duration.0),
                    query::TimeDirection::Departure(_) => Time(label_time.0 + duration.0),
                };

                let improvement = match current_best {
                    None => true,
                    Some(best) => match query.time_direction {
                        query::TimeDirection::Arrival(_) => true_time > best,
                        query::TimeDirection::Departure(_) => true_time < best,
                    },
                };

                if improvement {
                    state.target_tau_star = Opt::new(true_time);
                    state.target_best_stop = Opt::new(*target_stop);
                    state.target_best_round = Some(round);
                }
            }
        }
    }

    Itinerary::new(query, state, consumer)
}

pub fn time_to_walk(coordinate_a: Coordinate, coordinate_b: Coordinate) -> Duration {
    const R: f64 = 6371.0;
    let dist_lat = f64::to_radians(coordinate_a.lat_f64() - coordinate_b.lat_f64());
    let dist_lon = f64::to_radians(coordinate_a.lon_f64() - coordinate_b.lon_f64());
    let a = f64::powi(f64::sin(dist_lat / 2.0), 2)
        + f64::cos(f64::to_radians(coordinate_b.lat_f64()))
            * f64::cos(f64::to_radians(coordinate_a.lat_f64()))
            * f64::sin(dist_lon / 2.0)
            * f64::sin(dist_lon / 2.0);
    let c = 2.0 * f64::atan2(f64::sqrt(a), f64::sqrt(1.0 - a));
    let euclidean_distance = R * c * 1000.0;

    let network_distance = euclidean_distance * 1.3;

    let duration = (network_distance / 1.2).ceil() as u32;
    Duration(duration)
}
