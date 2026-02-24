use crate::{
    raptor::{
        Allocator, LazyBuffer, Parent, Update, find_earliest_trip, find_latest_trip,
        get_arrival_time, get_departure_time, time_to_walk, transfer_duration,
    },
    realtime::Realtime,
    repository::{Repository, Trip},
    shared::time,
};
use rayon::prelude::*;

/// Explores all active routes and add any updates to the update buffer in the allocator.
/// This is the core of the k-th round: it propagates travel times by one additional "hop"
/// using only transit routes.
pub fn explore_routes(repository: &Repository, realtime: &Realtime, allocator: &mut Allocator) {
    let updates = allocator
        .active_mask
        .iter_ones()
        .par_bridge()
        .map_init(
            || LazyBuffer::new(32),
            |buffer, route_idx| {
                let p_idx = allocator.active[route_idx];

                let route = &repository.raptor_routes[route_idx];
                let mut active_trip: Option<&Trip> = None;
                let mut boarding_stop: u32 = u32::MAX;
                let mut boarding_p: usize = usize::MAX;

                // Optimization: We only start scanning from the earliest stop that was
                // updated in the previous round (p_idx) to avoid redundant checks.
                for i in p_idx as usize..route.stops.len() {
                    let stop_idx = route.stops[i];
                    // PART A: Update arrival times
                    // If we are currently "on" a trip, check if it reaches this stop
                    // earlier than any path discovered in previous rounds.
                    if let Some(trip) = active_trip
                        && let (scheduled_arrival_time, actual_arrival_time) =
                            get_arrival_time(repository, realtime, trip.index, i)
                        && actual_arrival_time
                            < allocator.tau_star[stop_idx as usize].unwrap_or(time::MAX)
                        && actual_arrival_time < allocator.target.tau_star
                    {
                        let (scheduled_departure_time, actual_departure_time) =
                            get_departure_time(repository, realtime, trip.index, boarding_p);
                        buffer.push(Update::new(
                            stop_idx,
                            actual_arrival_time,
                            Parent::new_transit(
                                boarding_stop.into(),
                                stop_idx.into(),
                                trip.index,
                                scheduled_departure_time,
                                actual_departure_time,
                                scheduled_arrival_time,
                                actual_arrival_time,
                            ),
                        ));
                    }

                    // PART B: Trip Hopping
                    // Check if we can catch an even earlier trip. This happens if the
                    // arrival time at this stop from the PREVIOUS round is earlier
                    // than the departure of a trip on the current route.
                    let prev_label = allocator.prev_labels[stop_idx as usize].unwrap_or(time::MAX);
                    let (_, current_trip_dep) = active_trip
                        .map(|t| get_departure_time(repository, realtime, t.index, i))
                        .unwrap_or((time::MAX, time::MAX));

                    if prev_label <= current_trip_dep
                        && let Some(earlier_trip) =
                            find_earliest_trip(repository, realtime, route, i, prev_label)
                    {
                        // We found a better trip to board (or a fresh start for this route).
                        active_trip = Some(earlier_trip);
                        boarding_stop = stop_idx;
                        boarding_p = i;
                    }
                }
                buffer.swap()
            },
        )
        .flatten();
    allocator.updates.par_extend(updates);
}

/// Reverse exploration for Latest Departure Time (LDT) queries.
pub fn explore_routes_reverse(
    repository: &Repository,
    realtime: &Realtime,
    allocator: &mut Allocator,
) {
    let updates = allocator
        .active_mask
        .iter_ones()
        .par_bridge()
        .map_init(
            || LazyBuffer::new(32),
            |buffer, route_idx| {
                let p_idx = allocator.active[route_idx];

                let route = &repository.raptor_routes[route_idx];
                let mut active_trip: Option<&Trip> = None;
                let mut alighting_stop: u32 = u32::MAX;
                let mut alighting_p: usize = usize::MAX;

                // To find the latest departure, we scan backwards from the destination.
                // We want to "catch" a trip as late as possible to maximize our start time.
                for i in (0..=p_idx).rev() {
                    let stop_idx = route.stops[i as usize];

                    // PART A: If we have an active trip, can we leave this stop LATER
                    // than previously known and still catch it?
                    if let Some(trip) = active_trip {
                        let (scheduled_departure_time, actual_departure_time) =
                            get_departure_time(repository, realtime, trip.index, i as usize);

                        if actual_departure_time
                            > allocator.tau_star[stop_idx as usize].unwrap_or(time::MIN)
                        {
                            let (scheduled_arrival_time, actual_arrival_time) =
                                get_arrival_time(repository, realtime, trip.index, alighting_p);
                            buffer.push(Update::new(
                                stop_idx,
                                actual_departure_time,
                                Parent::new_transit(
                                    (stop_idx).into(),
                                    alighting_stop.into(),
                                    trip.index,
                                    scheduled_departure_time,
                                    actual_departure_time,
                                    scheduled_arrival_time,
                                    actual_arrival_time,
                                ),
                            ));
                        }
                    }

                    // PART B: Look for a trip that arrives at this stop LATER than
                    // our previous round's departure label, allowing us to shift our whole schedule later.
                    let prev_label = allocator.prev_labels[stop_idx as usize].unwrap_or(time::MIN);
                    let (_, trip_arrival) = active_trip
                        .map(|t| get_arrival_time(repository, realtime, t.index, i as usize))
                        .unwrap_or((time::MIN, time::MIN));

                    // If this stop has a departure label LATER than our current trip's arrival,
                    // find a trip that arrives even later (but still before the label)
                    if prev_label >= trip_arrival
                        && let Some(later_trip) =
                            find_latest_trip(repository, realtime, route, i as usize, prev_label)
                    {
                        active_trip = Some(later_trip);
                        alighting_stop = stop_idx;
                        alighting_p = i as usize;
                    }
                }
                buffer.swap()
            },
        )
        .flatten();
    allocator.updates.par_extend(updates);
}

/// Handles footpaths and transfers between stops.
/// In RAPTOR, transfers are processed after route exploration to ensure that
/// round k transit results can be used as the starting point for round k+1.
pub fn explore_transfers(allow_walk: bool, repository: &Repository, allocator: &mut Allocator) {
    let updates = allocator
        .marked_stops
        .iter_ones()
        .par_bridge()
        .map_init(
            || LazyBuffer::<Update>::new(32),
            |buffer, stop_idx| {
                // All the possible transfers
                repository.stop_to_transfers[stop_idx]
                    .iter()
                    .for_each(|transfer_idx| {
                        let transfer = &repository.transfers[*transfer_idx as usize];
                        let departure_time = allocator.curr_labels[stop_idx].unwrap_or(time::MAX);
                        let arrival_time = departure_time + transfer_duration(repository, transfer);
                        if arrival_time
                            < allocator.tau_star[transfer.to_stop_idx as usize].unwrap_or(time::MAX)
                            && arrival_time < allocator.target.tau_star
                        {
                            buffer.push(Update::new(
                                transfer.to_stop_idx,
                                arrival_time,
                                Parent::new_transfer(
                                    (stop_idx as u32).into(),
                                    transfer.to_stop_idx.into(),
                                    departure_time,
                                    arrival_time,
                                ),
                            ));
                        }
                    });

                if allow_walk {
                    let current_stop = &repository.stops[stop_idx];
                    repository.stop_to_walk_stop[stop_idx]
                        .iter()
                        .for_each(|next_stop_idx| {
                            let next_stop = &repository.stops[*next_stop_idx as usize];
                            let walking_distance = current_stop
                                .coordinate
                                .network_distance(&next_stop.coordinate);
                            let departure_time =
                                allocator.curr_labels[stop_idx].unwrap_or(time::MAX);
                            let arrival_time = departure_time + time_to_walk(walking_distance);
                            if arrival_time
                                < allocator.tau_star[next_stop.index as usize].unwrap_or(time::MAX)
                                && arrival_time < allocator.target.tau_star
                            {
                                buffer.push(Update::new(
                                    next_stop.index,
                                    arrival_time,
                                    Parent::new_walk(
                                        (stop_idx as u32).into(),
                                        next_stop.index.into(),
                                        departure_time,
                                        arrival_time,
                                    ),
                                ));
                            }
                        });
                }
                buffer.swap()
            },
        )
        .flatten();
    allocator.updates.par_extend(updates);
}

pub fn explore_transfers_reverse(
    allow_walk: bool,
    repository: &Repository,
    allocator: &mut Allocator,
) {
    let updates = allocator
        .marked_stops
        .iter_ones()
        .par_bridge()
        .map_init(
            || LazyBuffer::<Update>::new(32),
            |buffer, stop_idx| {
                // All the possible transfers
                repository.stop_to_transfers[stop_idx]
                    .iter()
                    .for_each(|transfer_idx| {
                        let transfer = &repository.transfers[*transfer_idx as usize];
                        let arrival_time = allocator.curr_labels[stop_idx].unwrap_or(time::MIN);
                        let departure_time = arrival_time - transfer_duration(repository, transfer);
                        if departure_time
                            > allocator.tau_star[transfer.to_stop_idx as usize].unwrap_or(time::MIN)
                        {
                            buffer.push(Update::new(
                                transfer.to_stop_idx,
                                departure_time,
                                Parent::new_transfer(
                                    transfer.to_stop_idx.into(),
                                    (stop_idx as u32).into(),
                                    departure_time,
                                    arrival_time,
                                ),
                            ));
                        }
                    });

                if allow_walk {
                    let current_stop = &repository.stops[stop_idx];
                    repository.stop_to_walk_stop[stop_idx]
                        .iter()
                        .for_each(|next_stop_idx| {
                            let next_stop = &repository.stops[*next_stop_idx as usize];
                            let walking_distance = current_stop
                                .coordinate
                                .network_distance(&next_stop.coordinate);
                            let arrival_time = allocator.curr_labels[stop_idx].unwrap_or(time::MIN);
                            let departure_time = arrival_time - time_to_walk(walking_distance);
                            if departure_time
                                > allocator.tau_star[next_stop.index as usize].unwrap_or(time::MIN)
                            {
                                buffer.push(Update::new(
                                    next_stop.index,
                                    departure_time,
                                    Parent::new_walk(
                                        next_stop.index.into(),
                                        (stop_idx as u32).into(),
                                        departure_time,
                                        arrival_time,
                                    ),
                                ));
                            }
                        });
                }
                buffer.swap()
            },
        )
        .flatten();
    allocator.updates.par_extend(updates);
}
