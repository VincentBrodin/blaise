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
    spatial::SpatialGrid,
};

mod explorer;
pub mod itinerary;
pub mod query;
pub mod state;

const MAX_ROUNDS: usize = 15;
const FRONT_SIZE: usize = 4;

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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct ParetoLabel {
    pub time: Time,
    pub cost: f32,
}

impl ParetoLabel {
    pub const MAX: Self = Self {
        time: Time(u32::MAX),
        cost: f32::MAX,
    };

    pub const MIN: Self = Self {
        time: Time(u32::MIN),
        cost: f32::MAX,
    };

    pub fn new(time: Time, cost: f32) -> Self {
        Self { time, cost }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ParetoFront(smallvec::SmallVec<[ParetoLabel; FRONT_SIZE]>);

impl ParetoFront {
    #[inline]
    pub fn new() -> Self {
        Self(smallvec::SmallVec::new())
    }

    /// Returns `true` if `(time, cost)` is dominated by any label already on the front.
    #[inline]
    pub fn is_dominated(&self, time: Time, cost: f32, is_arrival: bool) -> bool {
        self.0.iter().any(|l| {
            let time_ok = if is_arrival {
                l.time >= time
            } else {
                l.time <= time
            };
            time_ok && l.cost <= cost
        })
    }

    /// Add a label if it is not dominated. Remove any labels the new one dominates.
    /// Returns `true` if the label was accepted.
    #[inline]
    pub fn add(&mut self, label: ParetoLabel, is_arrival: bool) -> bool {
        if self.is_dominated(label.time, label.cost, is_arrival) {
            return false;
        }
        // Remove labels that the new label dominates.
        self.0.retain(|l| {
            let time_ok = if is_arrival {
                label.time >= l.time
            } else {
                label.time <= l.time
            };
            !(time_ok && label.cost <= l.cost)
        });
        self.0.push(label);
        true
    }

    /// Minimum cost across all labels. `f32::MAX` if empty.
    #[inline]
    pub fn best_cost(&self) -> f32 {
        self.0.iter().fold(f32::MAX, |acc, l| acc.min(l.cost))
    }

    /// Best time: earliest for forward, latest for reverse. `None` if empty.
    #[inline]
    pub fn best_time(&self, is_arrival: bool) -> Option<Time> {
        if is_arrival {
            self.0.iter().map(|l| l.time).max()
        } else {
            self.0.iter().map(|l| l.time).min()
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn clear(&mut self) {
        self.0.clear();
    }

    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &ParetoLabel> {
        self.0.iter()
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
    pub time: Time,
    pub cost: f32,

    pub parent: Parent,
}

impl Update {
    pub fn new(stop: StopIdx, time: Time, cost: f32, parent: Parent) -> Self {
        Self {
            stop,
            time,
            cost,
            parent,
        }
    }
}

pub fn solve(
    query: RaptorQuery,
    consumer: &Consumer,
    spatial: &SpatialGrid,
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
            transit_penalty: query.transit_penalty,
            transfer_penalty: query.transfer_penalty,
            walk_penalty: query.walk_penalty,
        };

        state.reset();
        return solve_core(&forward_query, consumer, spatial, state);
    }

    Ok(itinerary)
}

fn solve_core(
    query: &RaptorQuery,
    consumer: &Consumer,
    spatial: &SpatialGrid,
    state: &mut State,
) -> Result<Itinerary, crate::Error> {
    match query.time_direction {
        query::TimeDirection::Arrival(time) => {
            let init_label = ParetoLabel::new(time, 0.0);
            match &query.destination {
                QueryLocation::Stop(stop) => {
                    state.marked_stops[stop.as_usize()] = true;
                    state.current_labels[stop.as_usize()].add(init_label, true);
                    state.tau_star[stop.as_usize()].add(init_label, true);
                    let parent_idx = state.calc_parent_idx(0, *stop);
                    state.parents[parent_idx] = Some(Parent::Origin);
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops
                        .iter()
                        .filter(|&&stop| consumer.iter_trips_by_stop(stop).count() != 0)
                        .copied()
                    {
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()].add(init_label, true);
                        state.tau_star[stop.as_usize()].add(init_label, true);
                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .iter_stops_in_radius(*coordinate, query.search_radius)
                    .filter(|stop| consumer.iter_trips_by_stop(*stop).count() != 0)
                    .filter_map(|stop| consumer.stop(stop).coordinate.get().map(|c| (stop, c)))
                    .for_each(|(stop, to_coordinate)| {
                        let time_to_walk = time_to_walk(*coordinate, to_coordinate);
                        let arr_time = Time(time.0 - time_to_walk.0);
                        let walk_cost = time_to_walk.0 as f32 * query.walk_penalty;
                        let label = ParetoLabel::new(arr_time, walk_cost);
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()].add(label, true);
                        state.tau_star[stop.as_usize()].add(label, true);
                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }),
            };

            match &query.origin {
                QueryLocation::Stop(stop) => {
                    state.target_stops.push((*stop, Duration(0)));
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops
                        .iter()
                        .filter(|&&stop| consumer.iter_trips_by_stop(stop).count() != 0)
                        .copied()
                    {
                        state.target_stops.push((stop, Duration(0)));
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .iter_stops_in_radius(*coordinate, query.search_radius)
                    .filter(|stop_idx| consumer.iter_trips_by_stop(*stop_idx).count() != 0)
                    .filter_map(|stop_idx| {
                        consumer
                            .stop(stop_idx)
                            .coordinate
                            .get()
                            .map(|c| (stop_idx, c))
                    })
                    .for_each(|(stop_idx, to_coordinate)| {
                        let walk_time = time_to_walk(*coordinate, to_coordinate);
                        state.target_stops.push((stop_idx, walk_time));
                    }),
            }
        }
        query::TimeDirection::Departure(time) => {
            let init_label = ParetoLabel::new(time, 0.0);
            match &query.origin {
                QueryLocation::Stop(stop) => {
                    state.marked_stops[stop.as_usize()] = true;
                    state.current_labels[stop.as_usize()].add(init_label, false);
                    state.tau_star[stop.as_usize()].add(init_label, false);

                    let parent_idx = state.calc_parent_idx(0, *stop);
                    state.parents[parent_idx] = Some(Parent::Origin);
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops
                        .iter()
                        .filter(|&&stop| consumer.iter_trips_by_stop(stop).count() != 0)
                        .copied()
                    {
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()].add(init_label, false);
                        state.tau_star[stop.as_usize()].add(init_label, false);

                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .iter_stops_in_radius(*coordinate, query.search_radius)
                    .filter(|stop| consumer.iter_trips_by_stop(*stop).count() != 0)
                    .filter_map(|stop| consumer.stop(stop).coordinate.get().map(|c| (stop, c)))
                    .for_each(|(stop, to_coordinate)| {
                        let time_to_walk = time_to_walk(*coordinate, to_coordinate);
                        let arr_time = Time(time.0 + time_to_walk.0);
                        let walk_cost = time_to_walk.0 as f32 * query.walk_penalty;
                        let label = ParetoLabel::new(arr_time, walk_cost);
                        state.marked_stops[stop.as_usize()] = true;
                        state.current_labels[stop.as_usize()].add(label, false);
                        state.tau_star[stop.as_usize()].add(label, false);

                        let parent_idx = state.calc_parent_idx(0, stop);
                        state.parents[parent_idx] = Some(Parent::Origin);
                    }),
            };

            match &query.destination {
                QueryLocation::Stop(stop) => {
                    state.target_stops.push((*stop, Duration(0)));
                }
                QueryLocation::Stops(stops) => {
                    for stop in stops
                        .iter()
                        .filter(|&&stop| consumer.iter_trips_by_stop(stop).count() != 0)
                        .copied()
                    {
                        state.target_stops.push((stop, Duration(0)));
                    }
                }
                QueryLocation::Coordinate(coordinate) => spatial
                    .iter_stops_in_radius(*coordinate, query.search_radius)
                    .filter(|stop| consumer.iter_trips_by_stop(*stop).count() != 0)
                    .filter_map(|stop| consumer.stop(stop).coordinate.get().map(|c| (stop, c)))
                    .for_each(|(stop, to_coordinate)| {
                        let walk_time = time_to_walk(*coordinate, to_coordinate);
                        state.target_stops.push((stop, walk_time));
                    }),
            }
        }
    }

    let is_arrival = matches!(query.time_direction, query::TimeDirection::Arrival(_));

    let updates = match query.time_direction {
        query::TimeDirection::Arrival(_) => {
            explore_transfers_reverse(query, consumer, spatial, state)
        }
        query::TimeDirection::Departure(_) => explore_transfers(query, consumer, spatial, state),
    };

    state.apply_updates(0, is_arrival, &updates);

    // Evaluate whether any target stop is already reachable (round 0 walk).
    for (target_stop, duration) in state.target_stops.iter() {
        for label in state.current_labels[target_stop.as_usize()].iter() {
            let true_time = if is_arrival {
                Time(label.time.0 - duration.0)
            } else {
                Time(label.time.0 + duration.0)
            };
            let true_cost = label.cost + duration.0 as f32 * query.walk_penalty;
            let new_label = ParetoLabel::new(true_time, true_cost);
            if state.target_tau_star.add(new_label, is_arrival)
                && true_cost <= state.target_tau_star.best_cost()
            {
                state.target_best_stop = Opt::new(*target_stop);
                state.target_best_round = Some(0);
            }
        }
    }

    for round in 1..MAX_ROUNDS {
        if !state.marked_stops.iter().any(|&marked| marked) {
            break;
        }

        mem::swap(&mut state.current_labels, &mut state.previous_labels);
        state.current_labels.iter_mut().for_each(|f| f.clear());

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
                let updates = explore_trip_patterns_reverse(query, consumer, state);
                state.apply_updates(round, is_arrival, &updates);

                let updates = explore_transfers_reverse(query, consumer, spatial, state);
                state.apply_updates(round, is_arrival, &updates);
            }
            query::TimeDirection::Departure(_) => {
                let updates = explore_trip_patterns(query, consumer, state);
                state.apply_updates(round, is_arrival, &updates);

                let updates = explore_transfers(query, consumer, spatial, state);
                state.apply_updates(round, is_arrival, &updates);
            }
        }

        for (target_stop, duration) in state.target_stops.iter() {
            for label in state.current_labels[target_stop.as_usize()].iter() {
                let true_time = if is_arrival {
                    Time(label.time.0 - duration.0)
                } else {
                    Time(label.time.0 + duration.0)
                };
                let true_cost = label.cost + duration.0 as f32 * query.walk_penalty;
                let new_label = ParetoLabel::new(true_time, true_cost);
                if state.target_tau_star.add(new_label, is_arrival)
                    && true_cost <= state.target_tau_star.best_cost()
                {
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
