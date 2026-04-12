use gtfs_bin::{
    consumer::Consumer,
    models::{Coordinate, Duration, Opt, Sentinel, StopIdx, Time, TripIdx, TripPatternIdx},
};
use rayon::iter::{
    IndexedParallelIterator, IntoParallelRefIterator, ParallelExtend, ParallelIterator,
};

use crate::{
    raptor::{Parent, SequnceIdx, Update, state::State},
    spatial::SpatialHash,
};

pub fn get_arrival_time(consumer: &Consumer, trip_idx: TripIdx, p_idx: SequnceIdx) -> Opt<Time> {
    let stop_times = consumer.stop_times_by_trip(trip_idx);
    let stop_time = stop_times[p_idx.as_usize()];
    stop_time.arrival_time
}

pub fn get_departure_time(consumer: &Consumer, trip_idx: TripIdx, p_idx: SequnceIdx) -> Opt<Time> {
    let stop_times = consumer.stop_times_by_trip(trip_idx);
    let stop_time = stop_times[p_idx.as_usize()];
    stop_time.departure_time
}

fn find_earliest_trip(
    consumer: &Consumer,
    trip_pattern: TripPatternIdx,
    p_idx: SequnceIdx,
    ready_time: Time,
) -> Opt<TripIdx> {
    consumer
        .iter_trips_in_trip_pattern(trip_pattern)
        .filter_map(|trip| {
            get_departure_time(consumer, trip.idx, p_idx)
                .get()
                .map(|departure_time| (trip.idx, departure_time))
        })
        .filter(|&(_, departure_time)| departure_time >= ready_time)
        .min_by_key(|&(_, departure_time)| departure_time)
        .map(|(trip_idx, _)| trip_idx)
        .into()
}

pub fn explore_trip_patterns(consumer: &Consumer, state: &mut State) {
    let updates = state
        .active_trip_patterns
        .par_iter()
        .copied()
        .enumerate()
        .filter_map(|(i, p_idx)| p_idx.get().map(|p_idx| (i, p_idx)))
        .map(|(trip_pattern_idx, p_idx)| {
            let mut updates: Vec<Update> = Vec::with_capacity(32);
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
                    updates.push(Update::new(
                        stop.idx,
                        arrival_time,
                        Parent::Transit {
                            boarding_p_idx: boarding_p.get().unwrap_or(SequnceIdx::NONE),
                            trip,
                        },
                    ));
                }

                // PART B
                let previous_label = state.previous_labels[stop.idx.as_usize()]
                    .get()
                    .unwrap_or(Time(u32::MAX));

                let departure_time = active_trip
                    .get()
                    .and_then(|active_trip| get_departure_time(consumer, active_trip, i).get())
                    .unwrap_or(Time(u32::MAX));

                if previous_label <= departure_time
                    && let Some(earlier_trip) =
                        find_earliest_trip(consumer, trip_pattern.idx, i, previous_label).get()
                {
                    active_trip = Opt::new(earlier_trip);
                    boarding_p = Opt::new(i);
                }
            }
            updates
        })
        .flatten();
    state.update_buffer.par_extend(updates);
}

pub fn explore_transfers(consumer: &Consumer, spatial: &SpatialHash, state: &mut State) {
    let updates = state
        .marked_stops
        .par_iter()
        .copied()
        .enumerate()
        .filter(|&(_, marked)| marked)
        .map(|(stop_idx, _)| {
            let mut updates: Vec<Update> = Vec::with_capacity(32);
            let stop_idx = StopIdx(stop_idx as u32);
            let target_tau_star = state.target_tau_star.get().unwrap_or(Time(u32::MAX));

            consumer
                .outbound_transfers_by_stop(stop_idx)
                .iter()
                .for_each(|transfer| {
                    let tau_star = state.tau_star[transfer.to_stop_idx.as_usize()]
                        .get()
                        .unwrap_or(Time(u32::MAX));

                    let departure_time = state.current_labels[stop_idx.as_usize()]
                        .get()
                        .unwrap_or(Time(u32::MAX));
                    let arrival_time = Time(
                        departure_time.0
                            + transfer
                                .min_transfer_time
                                .get()
                                .unwrap_or(Duration(u32::MIN))
                                .0,
                    );

                    if arrival_time < tau_star && arrival_time < target_tau_star {
                        updates.push(Update::new(
                            transfer.to_stop_idx,
                            arrival_time,
                            Parent::Transfer {
                                from_stop: transfer.from_stop_idx,
                            },
                        ));
                    }
                });
            if let Some(coordinate) = consumer.stop(stop_idx).coordinate.get() {
                spatial
                    .get_in_radius_iter(coordinate, 500.0)
                    .filter(|stop_idx| consumer.iter_trips_by_stop(*stop_idx).count() != 0)
                    .filter_map(|stop_idx| {
                        consumer
                            .stop(stop_idx)
                            .coordinate
                            .get()
                            .map(|coordinate| (stop_idx, coordinate))
                    })
                    .for_each(|(to_stop, to_coordinate)| {
                        let tau_star = state.tau_star[to_stop.as_usize()]
                            .get()
                            .unwrap_or(Time(u32::MAX));

                        let departure_time = state.current_labels[stop_idx.as_usize()]
                            .get()
                            .unwrap_or(Time(u32::MAX));
                        let arrival_time =
                            Time(departure_time.0 + time_to_walk(coordinate, to_coordinate).0);

                        if arrival_time < tau_star && arrival_time < target_tau_star {
                            updates.push(Update::new(
                                to_stop,
                                arrival_time,
                                Parent::Transfer {
                                    from_stop: stop_idx,
                                },
                            ));
                        }
                    });
            }
            updates
        })
        .flatten();
    state.update_buffer.par_extend(updates);
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
    let distance = R * c * 1000.0;
    let duration = (distance / 1.5).ceil() as u32;
    Duration(duration)
}
