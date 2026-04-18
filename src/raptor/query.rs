use std::time::{SystemTime, UNIX_EPOCH};

use gtfs_bin::models::{Coordinate, StopIdx, Time};

#[derive(Clone, Copy, Debug)]
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

pub struct RaptorQuery {
    pub origin: Location,
    pub destination: Location,
    pub time_direction: TimeDirection,
    pub search_radius: f64,
}

impl RaptorQuery {
    pub fn new(origin: Location, destination: Location) -> Self {
        let now = SystemTime::now();

        let duration_since_epoch = now.duration_since(UNIX_EPOCH).expect("Time went backwards");

        let duration_since_midnight = Time((duration_since_epoch.as_secs() % 86_400) as u32);

        Self {
            origin,
            destination,
            time_direction: TimeDirection::Departure(duration_since_midnight),
            search_radius: 500.0,
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
}
