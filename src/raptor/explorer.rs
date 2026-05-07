use gtfs_bin::{
    consumer::Consumer,
    models::{Duration, Opt, Sentinel, StopIdx, Time, TripIdx, TripPatternIdx},
};
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::{
    raptor::{
        LiveTime, Parent, SequnceIdx, Update, query::RaptorQuery, state::State, time_to_walk,
    },
    spatial::SpatialGrid,
};

#[inline(always)]
pub fn get_arrival_time(consumer: &Consumer, trip_idx: TripIdx, p_idx: SequnceIdx) -> Opt<Time> {
    consumer.stop_times_by_trip(trip_idx)[p_idx.as_usize()].arrival_time
}

#[inline(always)]
pub fn get_departure_time(consumer: &Consumer, trip_idx: TripIdx, p_idx: SequnceIdx) -> Opt<Time> {
    consumer.stop_times_by_trip(trip_idx)[p_idx.as_usize()].departure_time
}

#[inline(always)]
fn merge_buffers<T>(mut a: Vec<T>, mut b: Vec<T>) -> Vec<T> {
    if a.len() >= b.len() {
        a.append(&mut b);
        a
    } else {
        b.append(&mut a);
        b
    }
}

/// Calculates transfer time, falling back to spatial walking if time is 0
#[inline]
fn calculate_transfer_time(
    consumer: &Consumer,
    from_stop: StopIdx,
    to_stop: StopIdx,
    min_time: Opt<Duration>,
) -> u32 {
    let mut time = min_time.as_option().unwrap_or(Duration(0)).0;
    if time == 0
        && let Some(from_coord) = consumer.stop(from_stop).coordinate.as_option()
        && let Some(to_coord) = consumer.stop(to_stop).coordinate.as_option()
    {
        time = time_to_walk(from_coord, to_coord).0;
    }
    time
}

// ==========================================
// TRIP SEARCH HELPERS
// ==========================================

fn find_earliest_trip(
    consumer: &Consumer,
    query: &RaptorQuery,
    trip_pattern: TripPatternIdx,
    p_idx: SequnceIdx,
    ready_time: Time,
) -> Opt<TripIdx> {
    consumer
        .iter_trips_in_trip_pattern(trip_pattern)
        .filter(|trip| consumer.is_service_active(trip.service_idx, query.date))
        .filter_map(|trip| {
            get_departure_time(consumer, trip.idx, p_idx)
                .as_option()
                .map(|t| (trip.idx, t))
        })
        .filter(|&(_, departure_time)| departure_time >= ready_time)
        .min_by_key(|&(_, departure_time)| departure_time)
        .map(|(trip_idx, _)| trip_idx)
        .into()
}

fn find_latest_trip(
    consumer: &Consumer,
    query: &RaptorQuery,
    trip_pattern: TripPatternIdx,
    p_idx: SequnceIdx,
    ready_time: Time,
) -> Opt<TripIdx> {
    consumer
        .iter_trips_in_trip_pattern(trip_pattern)
        .filter(|trip| consumer.is_service_active(trip.service_idx, query.date))
        .filter_map(|trip| {
            get_arrival_time(consumer, trip.idx, p_idx)
                .as_option()
                .map(|t| (trip.idx, t))
        })
        .filter(|&(_, arrival_time)| arrival_time <= ready_time)
        .max_by_key(|&(_, arrival_time)| arrival_time)
        .map(|(trip_idx, _)| trip_idx)
        .into()
}

// ==========================================
// CORE ALGORITHM PHASES
// ==========================================

pub fn explore_trip_patterns(
    query: &RaptorQuery,
    consumer: &Consumer,
    state: &mut State,
) -> Vec<Update> {
    state
        .active_trip_patterns
        .par_iter()
        .copied()
        .enumerate()
        .filter_map(|(i, p_idx)| p_idx.as_option().map(|p_idx| (i, p_idx)))
        .fold(
            || Vec::with_capacity(512),
            |mut buffer, (trip_pattern_idx, p_idx)| {
                let trip_pattern_idx = TripPatternIdx(trip_pattern_idx as u32);
                let trip_pattern = consumer.trip_pattern(trip_pattern_idx);

                let mut active_trip = Opt::new(TripIdx::NONE);
                let mut boarding_p = Opt::new(SequnceIdx::NONE);
                // Cost accumulated up to the boarding stop.
                let mut boarding_cost = f64::MAX;

                for (i, stop) in consumer
                    .iter_stop_sequence_by_trip_pattern(trip_pattern_idx)
                    .enumerate()
                    .skip(p_idx.as_usize())
                    .map(|(i, stop)| (SequnceIdx(i as u32), stop))
                {
                    // PART A: emit an update for the current alighting stop.
                    if let Some(trip) = active_trip.as_option()
                        && let arrival_time = get_arrival_time(consumer, trip, i)
                            .as_option()
                            .unwrap_or(Time(u32::MAX))
                        && !state.tau_star[stop.idx.as_usize()].is_dominated(
                            arrival_time,
                            boarding_cost as f32,
                            false,
                        )
                    {
                        let boarding_p_val = boarding_p.as_option().unwrap_or(SequnceIdx::NONE);
                        let departure_time = get_departure_time(consumer, trip, boarding_p_val)
                            .as_option()
                            .unwrap_or(Time(u32::MAX));
                        let ride_time = arrival_time.0.saturating_sub(departure_time.0) as f64;
                        // Transit ride cost: ride time × 1.0 (no penalty for being on a vehicle).
                        let prospective_cost = boarding_cost + ride_time * query.transit_penalty;

                        if !state.tau_star[stop.idx.as_usize()].is_dominated(
                            arrival_time,
                            prospective_cost as f32,
                            false,
                        ) && !state.target_tau_star.is_dominated(
                            arrival_time,
                            prospective_cost as f32,
                            false,
                        ) {
                            buffer.push(Update::new(
                                stop.idx,
                                arrival_time,
                                prospective_cost as f32,
                                Parent::Transit {
                                    boarding_p: boarding_p_val,
                                    alighting_p: i,
                                    trip,
                                    departure_time: LiveTime::scheduled_only(departure_time),
                                    arrival_time: LiveTime::scheduled_only(arrival_time),
                                },
                            ));
                        }
                    }

                    // PART B: consider boarding from every label on the front at this stop.
                    let front = &state.previous_labels[stop.idx.as_usize()];
                    for prev_label in front.iter() {
                        let current_dep = active_trip
                            .as_option()
                            .and_then(|t| get_departure_time(consumer, t, i).as_option())
                            .unwrap_or(Time(u32::MAX));

                        if prev_label.time < current_dep
                            && let Some(candidate_trip) = find_earliest_trip(
                                consumer,
                                query,
                                trip_pattern.idx,
                                i,
                                prev_label.time,
                            )
                            .as_option()
                        {
                            let candidate_dep = get_departure_time(consumer, candidate_trip, i)
                                .as_option()
                                .unwrap_or(Time(u32::MAX));
                            let wait = candidate_dep.0.saturating_sub(prev_label.time.0) as f64;
                            let candidate_cost =
                                prev_label.cost as f64 + wait * query.transit_penalty;

                            // Accept if earlier departure, or same departure with lower cost.
                            let better = candidate_dep < current_dep
                                || (candidate_dep == current_dep && candidate_cost < boarding_cost);
                            if better {
                                active_trip = Opt::new(candidate_trip);
                                boarding_p = Opt::new(i);
                                boarding_cost = candidate_cost;
                            }
                        }
                    }
                }
                buffer
            },
        )
        .reduce(Vec::new, merge_buffers)
}

pub fn explore_trip_patterns_reverse(
    query: &RaptorQuery,
    consumer: &Consumer,
    state: &mut State,
) -> Vec<Update> {
    state
        .active_trip_patterns
        .par_iter()
        .copied()
        .enumerate()
        .filter_map(|(i, p_idx)| p_idx.as_option().map(|p_idx| (i, p_idx)))
        .fold(
            || Vec::with_capacity(512),
            |mut buffer, (trip_pattern_idx, p_idx)| {
                let trip_pattern_idx = TripPatternIdx(trip_pattern_idx as u32);
                let trip_pattern = consumer.trip_pattern(trip_pattern_idx);

                let mut active_trip = Opt::new(TripIdx::NONE);
                let mut alighting_p = Opt::new(SequnceIdx::NONE);
                let mut alighting_cost = f64::MAX;

                let stops: Vec<_> = consumer
                    .iter_stop_sequence_by_trip_pattern(trip_pattern_idx)
                    .enumerate()
                    .take(p_idx.as_usize() + 1)
                    .map(|(i, stop)| (SequnceIdx(i as u32), stop))
                    .collect();

                for (i, stop) in stops.into_iter().rev() {
                    // PART A: emit an update for the current boarding stop.
                    if let Some(trip) = active_trip.as_option()
                        && let departure_time = get_departure_time(consumer, trip, i)
                            .as_option()
                            .unwrap_or(Time(u32::MIN))
                        && !state.tau_star[stop.idx.as_usize()].is_dominated(
                            departure_time,
                            alighting_cost as f32,
                            true,
                        )
                    {
                        let alighting_p_val = alighting_p.as_option().unwrap_or(SequnceIdx::NONE);
                        let arrival_time = get_arrival_time(consumer, trip, alighting_p_val)
                            .as_option()
                            .unwrap_or(Time(u32::MIN));

                        let ride_time = arrival_time.0.saturating_sub(departure_time.0) as f64;
                        let prospective_cost = alighting_cost + ride_time * query.transit_penalty;

                        if !state.tau_star[stop.idx.as_usize()].is_dominated(
                            departure_time,
                            prospective_cost as f32,
                            true,
                        ) && !state.target_tau_star.is_dominated(
                            departure_time,
                            prospective_cost as f32,
                            true,
                        ) {
                            buffer.push(Update::new(
                                stop.idx,
                                departure_time,
                                prospective_cost as f32,
                                Parent::Transit {
                                    boarding_p: i,
                                    alighting_p: alighting_p_val,
                                    trip,
                                    departure_time: LiveTime::scheduled_only(departure_time),
                                    arrival_time: LiveTime::scheduled_only(arrival_time),
                                },
                            ));
                        }
                    }

                    // PART B: consider alighting from every label on the front at this stop.
                    let front = &state.previous_labels[stop.idx.as_usize()];
                    for prev_label in front.iter() {
                        let current_arrival = active_trip
                            .as_option()
                            .and_then(|t| get_arrival_time(consumer, t, i).as_option())
                            .unwrap_or(Time(u32::MIN));

                        if prev_label.time >= current_arrival
                            && let Some(latest_trip) = find_latest_trip(
                                consumer,
                                query,
                                trip_pattern.idx,
                                i,
                                prev_label.time,
                            )
                            .as_option()
                        {
                            let later_arrival = get_arrival_time(consumer, latest_trip, i)
                                .as_option()
                                .unwrap_or(Time(u32::MIN));
                            let wait = prev_label.time.0.saturating_sub(later_arrival.0) as f64;
                            let candidate_cost =
                                prev_label.cost as f64 + wait * query.transit_penalty;

                            let better = active_trip.is_none()
                                || later_arrival > current_arrival
                                || (later_arrival == current_arrival
                                    && candidate_cost < alighting_cost);
                            if better {
                                active_trip = Opt::new(latest_trip);
                                alighting_p = Opt::new(i);
                                alighting_cost = candidate_cost;
                            }
                        }
                    }
                }
                buffer
            },
        )
        .reduce(Vec::new, merge_buffers)
}

pub fn explore_transfers(
    query: &RaptorQuery,
    consumer: &Consumer,
    spatial: &SpatialGrid,
    state: &mut State,
) -> Vec<Update> {
    state
        .marked_stops
        .par_iter()
        .copied()
        .enumerate()
        .filter(|&(_, marked)| marked)
        .fold(
            || Vec::with_capacity(512),
            |mut buffer, (stop_idx, _)| {
                let stop_idx = StopIdx(stop_idx as u32);

                // Emit one update per label on the current front for this stop.
                for current_label in state.current_labels[stop_idx.as_usize()].iter() {
                    let current_label = *current_label;

                    // Explicit Transfers
                    consumer
                        .outbound_transfers_by_stop(stop_idx)
                        .iter()
                        .for_each(|transfer| {
                            let transfer_time = calculate_transfer_time(
                                consumer,
                                transfer.from_stop_idx,
                                transfer.to_stop_idx,
                                transfer.min_transfer_time,
                            );
                            let arrival_time = Time(current_label.time.0 + transfer_time);
                            let transfer_cost = current_label.cost as f64
                                + transfer_time as f64 * query.transfer_penalty;

                            if !state.tau_star[transfer.to_stop_idx.as_usize()].is_dominated(
                                arrival_time,
                                transfer_cost as f32,
                                false,
                            ) && !state.target_tau_star.is_dominated(
                                arrival_time,
                                transfer_cost as f32,
                                false,
                            ) {
                                buffer.push(Update::new(
                                    transfer.to_stop_idx,
                                    arrival_time,
                                    transfer_cost as f32,
                                    Parent::Transfer {
                                        from_stop: transfer.from_stop_idx,
                                        departure_time: LiveTime::scheduled_only(
                                            current_label.time,
                                        ),
                                        arrival_time: LiveTime::scheduled_only(arrival_time),
                                    },
                                ));
                            }
                        });

                    // Spatial walking
                    if let Some(coordinate) = consumer.stop(stop_idx).coordinate.as_option() {
                        spatial
                            .iter_stops_in_radius(coordinate, query.search_radius)
                            .filter(|&s| consumer.iter_trips_by_stop(s).count() != 0)
                            .filter_map(|s| consumer.stop(s).coordinate.as_option().map(|c| (s, c)))
                            .for_each(|(to_stop, to_coordinate)| {
                                let walk_time = time_to_walk(coordinate, to_coordinate).0;
                                let arrival_time = Time(current_label.time.0 + walk_time);
                                let walk_cost = current_label.cost as f64
                                    + walk_time as f64 * query.walk_penalty;

                                if !state.tau_star[to_stop.as_usize()].is_dominated(
                                    arrival_time,
                                    walk_cost as f32,
                                    false,
                                ) && !state.target_tau_star.is_dominated(
                                    arrival_time,
                                    walk_cost as f32,
                                    false,
                                ) {
                                    buffer.push(Update::new(
                                        to_stop,
                                        arrival_time,
                                        walk_cost as f32,
                                        Parent::Walk {
                                            from_stop: stop_idx,
                                            departure_time: LiveTime::scheduled_only(
                                                current_label.time,
                                            ),
                                            arrival_time: LiveTime::scheduled_only(arrival_time),
                                        },
                                    ));
                                }
                            });
                    }
                }
                buffer
            },
        )
        .reduce(Vec::new, merge_buffers)
}

pub fn explore_transfers_reverse(
    query: &RaptorQuery,
    consumer: &Consumer,
    spatial: &SpatialGrid,
    state: &mut State,
) -> Vec<Update> {
    state
        .marked_stops
        .par_iter()
        .copied()
        .enumerate()
        .filter(|&(_, marked)| marked)
        .fold(
            || Vec::with_capacity(512),
            |mut buffer, (stop_idx, _)| {
                let stop_idx = StopIdx(stop_idx as u32);

                // Emit one update per label on the current front for this stop.
                for current_label in state.current_labels[stop_idx.as_usize()].iter() {
                    let current_label = *current_label;

                    // Explicit Transfers
                    consumer
                        .iter_inbound_transfers_by_stop(stop_idx)
                        .for_each(|transfer| {
                            let transfer_time = calculate_transfer_time(
                                consumer,
                                transfer.from_stop_idx,
                                transfer.to_stop_idx,
                                transfer.min_transfer_time,
                            );
                            let transfer_cost = current_label.cost as f64
                                + transfer_time as f64 * query.transfer_penalty;

                            if !state.tau_star[transfer.from_stop_idx.as_usize()].is_dominated(
                                Time(0),
                                transfer_cost as f32,
                                true,
                            ) && !state.target_tau_star.is_dominated(
                                Time(0),
                                transfer_cost as f32,
                                true,
                            ) && current_label.time.0 >= transfer_time
                            {
                                let departure_time = Time(current_label.time.0 - transfer_time);
                                if !state.tau_star[transfer.from_stop_idx.as_usize()].is_dominated(
                                    departure_time,
                                    transfer_cost as f32,
                                    true,
                                ) && !state.target_tau_star.is_dominated(
                                    departure_time,
                                    transfer_cost as f32,
                                    true,
                                ) {
                                    buffer.push(Update::new(
                                        transfer.from_stop_idx,
                                        departure_time,
                                        transfer_cost as f32,
                                        Parent::Transfer {
                                            from_stop: stop_idx,
                                            departure_time: LiveTime::scheduled_only(
                                                departure_time,
                                            ),
                                            arrival_time: LiveTime::scheduled_only(
                                                current_label.time,
                                            ),
                                        },
                                    ));
                                }
                            }
                        });

                    // Spatial walking
                    if let Some(coordinate) = consumer.stop(stop_idx).coordinate.as_option() {
                        spatial
                            .iter_stops_in_radius(coordinate, query.search_radius)
                            .filter(|&s| consumer.iter_trips_by_stop(s).count() != 0)
                            .filter_map(|s| consumer.stop(s).coordinate.as_option().map(|c| (s, c)))
                            .for_each(|(other_stop, other_coordinate)| {
                                let walk_time = time_to_walk(coordinate, other_coordinate).0;
                                let walk_cost = current_label.cost as f64
                                    + walk_time as f64 * query.walk_penalty;

                                if current_label.time.0 >= walk_time {
                                    let departure_time = Time(current_label.time.0 - walk_time);
                                    if !state.tau_star[other_stop.as_usize()].is_dominated(
                                        departure_time,
                                        walk_cost as f32,
                                        true,
                                    ) && !state.target_tau_star.is_dominated(
                                        departure_time,
                                        walk_cost as f32,
                                        true,
                                    ) {
                                        buffer.push(Update::new(
                                            other_stop,
                                            departure_time,
                                            walk_cost as f32,
                                            Parent::Walk {
                                                from_stop: stop_idx,
                                                departure_time: LiveTime::scheduled_only(
                                                    departure_time,
                                                ),
                                                arrival_time: LiveTime::scheduled_only(
                                                    current_label.time,
                                                ),
                                            },
                                        ));
                                    }
                                }
                            });
                    }
                }
                buffer
            },
        )
        .reduce(Vec::new, merge_buffers)
}
