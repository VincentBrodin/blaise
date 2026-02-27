use crate::{
    raptor::{
        Parent, ParentType,
        location::{Location, Point},
    },
    realtime::Realtime,
    repository::Repository,
    shared::{Distance, time::Time},
};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct Leg<'a> {
    pub from: Location<'a>,
    pub to: Location<'a>,
    pub scheduled_departure_time: Time,
    pub actual_departure_time: Time,
    pub scheduled_arrival_time: Time,
    pub actual_arrival_time: Time,
    pub stops: Vec<LegStop<'a>>,
    pub leg_type: LegType,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum LegType {
    Transit(u32),
    Transfer,
    Walk,
}

impl From<ParentType> for LegType {
    fn from(value: ParentType) -> Self {
        match value {
            ParentType::Transit(trip_idx) => Self::Transit(trip_idx),
            ParentType::Transfer => Self::Transfer,
            ParentType::Walk => Self::Walk,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LegStop<'a> {
    pub location: Location<'a>,
    pub scheduled_departure_time: Time,
    pub actual_departure_time: Time,
    pub scheduled_arrival_time: Time,
    pub actual_arrival_time: Time,
    pub distance_traveled: Option<Distance>,
}

impl<'a> LegStop<'a> {
    pub(crate) fn generate_stops(
        parent: &Parent,
        repository: &'a Repository,
        realtime: &Realtime,
    ) -> Vec<Self> {
        match parent.parent_type {
            ParentType::Transit(trip_idx) => {
                let trip = &repository.trips[trip_idx as usize];
                let stop_times = repository.stop_times_by_trip_idx(trip.index);
                let mut stops = Vec::with_capacity(stop_times.len());
                if let Point::Stop(from_idx) = parent.from
                    && let Point::Stop(to_idx) = parent.to
                {
                    let mut in_trip = false;
                    for stop_time in stop_times {
                        if stop_time.stop_idx == from_idx {
                            in_trip = true;
                        }
                        if in_trip {
                            let stop = &repository.stops[stop_time.stop_idx as usize];
                            let delay = &realtime.stop_time_updates[stop_time.index as usize];
                            stops.push(LegStop {
                                location: Location::from_stop(stop, repository),
                                scheduled_departure_time: stop_time.departure_time,
                                actual_departure_time: stop_time
                                    .departure_time
                                    .with_delay(delay.departure_delay),
                                scheduled_arrival_time: stop_time.arrival_time,
                                actual_arrival_time: stop_time
                                    .arrival_time
                                    .with_delay(delay.arrival_delay),
                                distance_traveled: stop_time.distance_traveled,
                            });
                            if stop_time.stop_idx == to_idx && in_trip {
                                break;
                            }
                        }
                    }
                }

                stops
            }
            ParentType::Transfer => vec![],
            ParentType::Walk => vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct Itinerary<'a> {
    pub from: Location<'a>,
    pub to: Location<'a>,
    pub legs: Vec<Leg<'a>>,
}

impl<'a> Itinerary<'a> {
    pub(crate) fn new(
        from: Location<'a>,
        to: Location<'a>,
        path: Vec<Parent>,
        repository: &'a Repository,
        realtime: &Realtime,
    ) -> Self {
        let legs = path
            .into_iter()
            .map(|parent| {
                let leg_from = point_to_location(parent.from, repository);
                let leg_to = point_to_location(parent.to, repository);
                Leg {
                    from: leg_from,
                    to: leg_to,
                    scheduled_departure_time: parent.scheduled_departure_time,
                    actual_departure_time: parent.actual_departure_time,
                    scheduled_arrival_time: parent.scheduled_arrival_time,
                    actual_arrival_time: parent.actual_arrival_time,
                    stops: LegStop::generate_stops(&parent, repository, realtime),
                    leg_type: parent.parent_type.into(),
                }
            })
            .collect();
        Self { from, to, legs }
    }
}

fn point_to_location<'a>(point: Point, repository: &'a Repository) -> Location<'a> {
    match point {
        Point::Coordinate(coordinate) => Location::from_coordinate(coordinate),
        Point::Stop(idx) => {
            let stop = &repository.stops[idx as usize];
            Location::from_stop(stop, repository)
        }
    }
}
