use chrono::{Local, Timelike};
use gtfs_bin::{
    consumer::Consumer,
    models::{Coordinate, Date, StopIdx, Time},
};

use crate::{
    raptor::{itinerary::Itinerary, solve, state::State},
    spatial::SpatialGrid,
};

pub enum QueryLocation {
    Stop(StopIdx),
    Stops(Vec<StopIdx>),
    Coordinate(Coordinate),
}

impl From<Coordinate> for QueryLocation {
    fn from(value: Coordinate) -> Self {
        QueryLocation::Coordinate(value)
    }
}

impl From<StopIdx> for QueryLocation {
    fn from(value: StopIdx) -> Self {
        QueryLocation::Stop(value)
    }
}

impl From<Vec<StopIdx>> for QueryLocation {
    fn from(value: Vec<StopIdx>) -> Self {
        QueryLocation::Stops(value)
    }
}

impl QueryLocation {
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

pub struct RaptorQuery {
    pub origin: QueryLocation,
    pub destination: QueryLocation,
    pub time_direction: TimeDirection,
    pub date: Date,
    pub search_radius: f64,

    pub transit_penalty: f32,
    pub transfer_penalty: f32,
    pub walk_penalty: f32,
}

impl RaptorQuery {
    pub fn new(origin: QueryLocation, destination: QueryLocation) -> Self {
        let now = Local::now();
        let time = Time(now.num_seconds_from_midnight());
        let seconds_since_epoch = now.timestamp();
        let days_since_epoch = seconds_since_epoch / 86_400;
        let date = Date(days_since_epoch as u32);

        Self {
            origin,
            destination,
            time_direction: TimeDirection::Departure(time),
            search_radius: 1000.0,
            date,

            transit_penalty: 1.0,
            transfer_penalty: 1.5,
            walk_penalty: 2.0,
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

    pub fn solve(
        self,
        consumer: &Consumer,
        spatial: &SpatialGrid,
        state: &mut State,
    ) -> Result<Itinerary, crate::Error> {
        solve(self, consumer, spatial, state)
    }
}
