use std::time::{SystemTime, UNIX_EPOCH};

use gtfs_bin::models::{Coordinate, Date, StopIdx, Time};

pub enum QueryLocation<'a> {
    Stop(StopIdx),
    Stops(&'a [StopIdx]),
    Coordinate(Coordinate),
}

impl<'a> From<Coordinate> for QueryLocation<'a> {
    fn from(value: Coordinate) -> Self {
        QueryLocation::Coordinate(value)
    }
}

impl<'a> From<StopIdx> for QueryLocation<'a> {
    fn from(value: StopIdx) -> Self {
        QueryLocation::Stop(value)
    }
}

impl<'a> From<&'a [StopIdx]> for QueryLocation<'a> {
    fn from(value: &'a [StopIdx]) -> Self {
        QueryLocation::Stops(value)
    }
}

impl<'a> QueryLocation<'a> {
    pub fn resolve(&self, stop: StopIdx) -> Location {
        match self {
            QueryLocation::Coordinate(c) => Location::Coordinate(*c),
            _ => Location::Stop(stop),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Stop(StopIdx),
    Coordinate(Coordinate),
}

impl From<Coordinate> for Location {
    fn from(value: Coordinate) -> Self {
        Location::Coordinate(value)
    }
}

impl From<StopIdx> for Location {
    fn from(value: StopIdx) -> Self {
        Location::Stop(value)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TimeDirection {
    Arrival(Time),
    Departure(Time),
}

pub struct RaptorQuery<'a> {
    pub origin: QueryLocation<'a>,
    pub destination: QueryLocation<'a>,
    pub time_direction: TimeDirection,
    pub date: Date,
    pub search_radius: f64,
}

impl<'a> RaptorQuery<'a> {
    pub fn new(origin: QueryLocation<'a>, destination: QueryLocation<'a>) -> Self {
        let now = SystemTime::now();

        let duration_since_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

        let duration_since_midnight = Time((duration_since_epoch.as_secs() % 86_400) as u32);

        Self {
            origin,
            destination,
            time_direction: TimeDirection::Departure(duration_since_midnight),
            search_radius: 1500.0,
            date: Date((duration_since_epoch.as_secs() / 86_400) as u32),
        }
    }

    pub fn with_arrival(mut self, arrival: Time) -> Self {
        self.time_direction = TimeDirection::Arrival(arrival);
        self
    }
    pub fn with_departure(mut self, departure: Time) -> Self {
        self.time_direction = TimeDirection::Departure(departure);
        self
    }

    pub fn with_date(mut self, date: Date) -> Self {
        self.date = date;
        self
    }
}
