use gtfs_bin::{
    consumer::Consumer,
    models::{Duration, Opt, Sentinel, StopIdx, Time, TripIdx, TripPatternIdx},
};
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator};

use crate::{
    raptor::{
        LiveTime, Parent, SequnceIdx, Update, query::RaptorQuery, state::State, time_to_walk,
    },
    spatial::SpatialHash,
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
    let mut time = min_time.get().unwrap_or(Duration(0)).0;
    if time == 0
        && let Some(from_coord) = consumer.stop(from_stop).coordinate.get()
        && let Some(to_coord) = consumer.stop(to_stop).coordinate.get()
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
                .get()
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
                .get()
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
        .filter_map(|(i, p_idx)| p_idx.get().map(|p_idx| (i, p_idx)))
        .fold(
            || Vec::with_capacity(512),
            |mut buffer, (trip_pattern_idx, p_idx)| {
                let trip_pattern_idx = TripPatternIdx(trip_pattern_idx as u32);
                let trip_pattern = consumer.trip_pattern(trip_pattern_idx);

                let mut active_trip = Opt::new(TripIdx::NONE);
                let mut boarding_p = Opt::new(SequnceIdx::NONE);

                for (i, stop) in consumer
                    .iter_stop_sequence_by_trip_pattern(trip_pattern_idx)
                    .enumerate()
                    .skip(p_idx.as_usize())
                    .map(|(i, stop)| (SequnceIdx(i as u32), stop))
                {
                    let tau_star = state.tau_star[stop.idx.as_usize()]
                        .get()
                        .unwrap_or(Time(u32::MAX));
                    let target_tau_star = state.target_tau_star.get().unwrap_or(Time(u32::MAX));

                    // PART A
                    if let Some(trip) = active_trip.get()
                        && let arrival_time = get_arrival_time(consumer, trip, i)
                            .get()
                            .unwrap_or(Time(u32::MAX))
                        && arrival_time < tau_star
                        && arrival_time < target_tau_star
                    {
                        let boarding_p = boarding_p.get().unwrap_or(SequnceIdx::NONE);
                        let departure_time = get_departure_time(consumer, trip, boarding_p)
                            .get()
                            .unwrap_or(Time(u32::MAX));

                        buffer.push(Update::new(
                            stop.idx,
                            arrival_time,
                            Parent::Transit {
                                boarding_p,
                                alighting_p: i,
                                trip,
                                departure_time: LiveTime::scheduled_only(departure_time),
                                arrival_time: LiveTime::scheduled_only(arrival_time),
                            },
                        ));
                    }

                    // PART B
                    let previous_label = state.previous_labels[stop.idx.as_usize()]
                        .get()
                        .unwrap_or(Time(u32::MAX));
                    let departure_time = active_trip
                        .get()
                        .and_then(|t| get_departure_time(consumer, t, i).get())
                        .unwrap_or(Time(u32::MAX));

                    if previous_label < departure_time
                        && let Some(earlier_trip) =
                            find_earliest_trip(consumer, query, trip_pattern.idx, i, previous_label)
                                .get()
                    {
                        let earlier_departure = get_departure_time(consumer, earlier_trip, i)
                            .get()
                            .unwrap_or(Time(u32::MAX));
                        if earlier_departure < departure_time {
                            active_trip = Opt::new(earlier_trip);
                            boarding_p = Opt::new(i);
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
        .filter_map(|(i, p_idx)| p_idx.get().map(|p_idx| (i, p_idx)))
        .fold(
            || Vec::with_capacity(512),
            |mut buffer, (trip_pattern_idx, p_idx)| {
                let trip_pattern_idx = TripPatternIdx(trip_pattern_idx as u32);
                let trip_pattern = consumer.trip_pattern(trip_pattern_idx);

                let mut active_trip = Opt::new(TripIdx::NONE);
                let mut alighting_p = Opt::new(SequnceIdx::NONE);

                let stops: Vec<_> = consumer
                    .iter_stop_sequence_by_trip_pattern(trip_pattern_idx)
                    .enumerate()
                    .take(p_idx.as_usize() + 1)
                    .map(|(i, stop)| (SequnceIdx(i as u32), stop))
                    .collect();

                for (i, stop) in stops.into_iter().rev() {
                    let tau_star = state.tau_star[stop.idx.as_usize()]
                        .get()
                        .unwrap_or(Time(u32::MIN));
                    let target_tau_star = state.target_tau_star.get().unwrap_or(Time(u32::MIN));

                    // PART A
                    if let Some(trip) = active_trip.get()
                        && let departure_time = get_departure_time(consumer, trip, i)
                            .get()
                            .unwrap_or(Time(u32::MIN))
                        && departure_time > tau_star
                        && departure_time > target_tau_star
                    {
                        let alighting_p = alighting_p.get().unwrap_or(SequnceIdx::NONE);
                        let arrival_time = get_arrival_time(consumer, trip, alighting_p)
                            .get()
                            .unwrap_or(Time(u32::MIN));

                        buffer.push(Update::new(
                            stop.idx,
                            departure_time,
                            Parent::Transit {
                                boarding_p: i,
                                alighting_p,
                                trip,
                                departure_time: LiveTime::scheduled_only(departure_time),
                                arrival_time: LiveTime::scheduled_only(arrival_time),
                            },
                        ));
                    }

                    // PART B
                    let previous_label = state.previous_labels[stop.idx.as_usize()]
                        .get()
                        .unwrap_or(Time(u32::MIN));
                    let arrival_time = active_trip
                        .get()
                        .and_then(|t| get_arrival_time(consumer, t, i).get())
                        .unwrap_or(Time(u32::MIN));

                    if previous_label >= arrival_time
                        && let Some(latest_trip) =
                            find_latest_trip(consumer, query, trip_pattern.idx, i, previous_label)
                                .get()
                    {
                        let later_arrival = get_arrival_time(consumer, latest_trip, i)
                            .get()
                            .unwrap_or(Time(u32::MIN));
                        if active_trip.is_none() || later_arrival > arrival_time {
                            active_trip = Opt::new(latest_trip);
                            alighting_p = Opt::new(i);
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
    spatial: &SpatialHash,
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
                let target_tau_star = state.target_tau_star.get().unwrap_or(Time(u32::MAX));
                let departure_time = state.current_labels[stop_idx.as_usize()]
                    .get()
                    .unwrap_or(Time(u32::MAX));

                // Explicit Transfers
                consumer
                    .outbound_transfers_by_stop(stop_idx)
                    .iter()
                    .for_each(|transfer| {
                        let tau_star = state.tau_star[transfer.to_stop_idx.as_usize()]
                            .get()
                            .unwrap_or(Time(u32::MAX));

                        let transfer_time = calculate_transfer_time(
                            consumer,
                            transfer.from_stop_idx,
                            transfer.to_stop_idx,
                            transfer.min_transfer_time,
                        );

                        let arrival_time = Time(departure_time.0 + transfer_time);

                        if arrival_time < tau_star && arrival_time < target_tau_star {
                            buffer.push(Update::new(
                                transfer.to_stop_idx,
                                arrival_time,
                                Parent::Transfer {
                                    from_stop: transfer.from_stop_idx,
                                    departure_time: LiveTime::scheduled_only(departure_time),
                                    arrival_time: LiveTime::scheduled_only(arrival_time),
                                },
                            ));
                        }
                    });

                // Spatial walking logic
                if let Some(coordinate) = consumer.stop(stop_idx).coordinate.get() {
                    spatial
                        .get_in_radius_iter(consumer, coordinate, query.search_radius)
                        .filter(|&s| consumer.iter_trips_by_stop(s).count() != 0)
                        .filter_map(|s| consumer.stop(s).coordinate.get().map(|c| (s, c)))
                        .for_each(|(to_stop, to_coordinate)| {
                            let tau_star = state.tau_star[to_stop.as_usize()]
                                .get()
                                .unwrap_or(Time(u32::MAX));
                            let arrival_time =
                                Time(departure_time.0 + time_to_walk(coordinate, to_coordinate).0);

                            if arrival_time < tau_star && arrival_time < target_tau_star {
                                buffer.push(Update::new(
                                    to_stop,
                                    arrival_time,
                                    Parent::Walk {
                                        from_stop: stop_idx,
                                        departure_time: LiveTime::scheduled_only(departure_time),
                                        arrival_time: LiveTime::scheduled_only(arrival_time),
                                    },
                                ));
                            }
                        });
                }
                buffer
            },
        )
        .reduce(Vec::new, merge_buffers)
}

pub fn explore_transfers_reverse(
    query: &RaptorQuery,
    consumer: &Consumer,
    spatial: &SpatialHash,
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
                let target_tau_star = state.target_tau_star.get().unwrap_or(Time(u32::MIN));
                let arrival_time = state.current_labels[stop_idx.as_usize()]
                    .get()
                    .unwrap_or(Time(u32::MIN));

                // Explicit Transfers
                consumer
                    .iter_inbound_transfers_by_stop(stop_idx)
                    .for_each(|transfer| {
                        let tau_star = state.tau_star[transfer.from_stop_idx.as_usize()]
                            .get()
                            .unwrap_or(Time(u32::MIN));

                        let transfer_time = calculate_transfer_time(
                            consumer,
                            transfer.from_stop_idx,
                            transfer.to_stop_idx,
                            transfer.min_transfer_time,
                        );

                        if arrival_time.0 >= transfer_time {
                            let departure_time = Time(arrival_time.0 - transfer_time);

                            if departure_time > tau_star && departure_time > target_tau_star {
                                buffer.push(Update::new(
                                    transfer.from_stop_idx,
                                    departure_time,
                                    Parent::Transfer {
                                        from_stop: stop_idx,
                                        departure_time: LiveTime::scheduled_only(departure_time),
                                        arrival_time: LiveTime::scheduled_only(arrival_time),
                                    },
                                ));
                            }
                        }
                    });

                // Spatial walking logic
                if let Some(coordinate) = consumer.stop(stop_idx).coordinate.get() {
                    spatial
                        .get_in_radius_iter(consumer, coordinate, query.search_radius)
                        .filter(|&s| consumer.iter_trips_by_stop(s).count() != 0)
                        .filter_map(|s| consumer.stop(s).coordinate.get().map(|c| (s, c)))
                        .for_each(|(other_stop, other_coordinate)| {
                            let tau_star = state.tau_star[other_stop.as_usize()]
                                .get()
                                .unwrap_or(Time(u32::MIN));
                            let walk_time = time_to_walk(coordinate, other_coordinate).0;

                            if arrival_time.0 >= walk_time {
                                let departure_time = Time(arrival_time.0 - walk_time);
                                if departure_time > tau_star && departure_time > target_tau_star {
                                    buffer.push(Update::new(
                                        other_stop,
                                        departure_time,
                                        Parent::Walk {
                                            from_stop: stop_idx,
                                            departure_time: LiveTime::scheduled_only(
                                                departure_time,
                                            ),
                                            arrival_time: LiveTime::scheduled_only(arrival_time),
                                        },
                                    ));
                                }
                            }
                        });
                }
                buffer
            },
        )
        .reduce(Vec::new, merge_buffers)
}
