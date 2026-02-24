use crate::{
    raptor::{self, Allocator, Location},
    realtime::Realtime,
    repository::{RaptorRoute, Repository, Stop, Transfer, Trip},
    shared::{AVERAGE_STOP_DISTANCE, Distance, Duration, Time},
};
use tracing::{trace, warn};

pub fn stops_by_location<'a>(
    repository: &'a Repository,
    location: &'a Location,
) -> Result<Vec<&'a Stop>, raptor::Error> {
    match location {
        Location::Area(id) => {
            trace!("Possible area: {id}");
            let area = repository
                .area_by_id(id)
                .ok_or(raptor::Error::InvalidAreaID)?;

            let stops: Vec<_> = repository
                .stops_by_area_idx(area.index)
                .into_iter()
                .filter(|stop| repository.stop_idx_has_trips(stop.index))
                .collect();
            if !stops.is_empty() {
                Ok(stops)
            } else {
                warn!("Had to use coordinates to satisfy stops for area {id}");
                let coordiante = repository.coordinate_by_area_idx(area.index);
                Ok(repository
                    .stops_by_coordinate(&coordiante, AVERAGE_STOP_DISTANCE)
                    .into_iter()
                    .filter(|stop| repository.stop_idx_has_trips(stop.index))
                    .collect())
            }
        }
        Location::Stop(id) => {
            trace!("Possible stop: {id}");
            let stop = repository
                .stop_by_id(id)
                .ok_or(raptor::Error::InvalidStopID)?;
            if let Some(station_idx) = stop.parent_index {
                Ok(repository.stops_by_station(station_idx))
            } else {
                let stops = repository.stops_by_station(stop.index);
                if stops.is_empty() {
                    Ok(vec![stop])
                } else {
                    Ok(stops)
                }
            }
        }
        Location::Coordinate(coordinate) => Ok(repository
            .stops_by_coordinate(coordinate, AVERAGE_STOP_DISTANCE)
            .into_iter()
            .filter(|stop| repository.stop_idx_has_trips(stop.index))
            .collect()),
    }
}

pub(crate) struct ServingRoute {
    pub route_idx: u32,
    pub idx_in_route: u32,
}
pub fn routes_serving_stop(repository: &Repository, stop_idx: u32, allocator: &mut Allocator) {
    allocator.routes_serving_stops.clear();
    allocator.routes_serving_stops.extend(
        repository.stop_to_raptors[stop_idx as usize]
            .iter()
            .filter_map(|route_idx| {
                let route = &repository.raptor_routes[*route_idx as usize];
                index_in_route(route, stop_idx).map(|idx_in_route| ServingRoute {
                    route_idx: route.index,
                    idx_in_route,
                })
            }),
    )
}

pub fn index_in_route(route: &RaptorRoute, stop_idx: u32) -> Option<u32> {
    for (index, route_stop_idx) in route.stops.iter().enumerate() {
        if *route_stop_idx == stop_idx {
            return Some(index as u32);
        }
    }
    None
}

pub fn get_arrival_time(
    repository: &Repository,
    realtime: &Realtime,
    trip_idx: u32,
    p_idx: usize,
) -> (Time, Time) {
    let stop_times = repository.stop_times_by_trip_idx(trip_idx);
    let stop_time = &stop_times[p_idx];
    let delay = &realtime.stop_time_updates[stop_time.index as usize];
    let arr = stop_times[p_idx].departure_time;
    (arr, arr.with_delay(delay.arrival_delay))
}

pub fn get_departure_time(
    repository: &Repository,
    realtime: &Realtime,
    trip_idx: u32,
    p_idx: usize,
) -> (Time, Time) {
    let stop_times = repository.stop_times_by_trip_idx(trip_idx);
    let stop_time = &stop_times[p_idx];
    let delay = &realtime.stop_time_updates[stop_time.index as usize];
    let dep = stop_times[p_idx].departure_time;
    (dep, dep.with_delay(delay.departure_delay))
}

/// Finds the latest trip that we can take from current stop based on the time
pub fn find_latest_trip<'a>(
    repository: &'a Repository,
    realtime: &Realtime,
    route: &'a RaptorRoute,
    p_idx: usize,
    max_arrival: Time,
) -> Option<&'a Trip> {
    let idx = route.trips.partition_point(|&trip_idx| {
        let (_, arrival_time) = get_arrival_time(repository, realtime, trip_idx, p_idx);
        arrival_time <= max_arrival
    });

    if idx == 0 {
        None
    } else {
        route
            .trips
            .get(idx - 1)
            .map(|&t_idx| &repository.trips[t_idx as usize])
    }
}

/// Finds the earliest trip that we can take from current stop based on the time
pub fn find_earliest_trip<'a>(
    repository: &'a Repository,
    realtime: &Realtime,
    route: &'a RaptorRoute,
    p_idx: usize,
    min_departure: Time,
) -> Option<&'a Trip> {
    let idx = route.trips.partition_point(|&trip_idx| {
        let (_, arrival_time) = get_arrival_time(repository, realtime, trip_idx, p_idx);
        arrival_time < min_departure
    });
    route
        .trips
        .get(idx)
        .map(|&t_idx| &repository.trips[t_idx as usize])
}

pub fn transfer_duration<'a>(repository: &'a Repository, transfer: &'a Transfer) -> Duration {
    if let Some(duration) = transfer.min_transfer_time {
        duration
    } else {
        let from = &repository.stops[transfer.from_stop_idx as usize];
        let to = &repository.stops[transfer.to_stop_idx as usize];
        time_to_walk(from.coordinate.network_distance(&to.coordinate))
    }
}

#[inline(always)]
pub const fn time_to_walk(distance: Distance) -> Duration {
    let duration = (distance.as_meters() / 1.5).ceil() as u32;
    Duration::from_seconds(duration)
}
