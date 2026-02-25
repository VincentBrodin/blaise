use blaise::{
    raptor::{Itinerary, Leg, LegStop, LegType, Location},
    repository::{Repository, Shape},
    shared::{geo::Coordinate, time::Time},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationDto {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub coordinate: Coordinate,
}

impl From<Coordinate> for LocationDto {
    fn from(value: Coordinate) -> Self {
        LocationDto {
            kind: "coordinate".into(),
            id: value.to_string(),
            name: value.to_string(),
            coordinate: value,
        }
    }
}

impl LocationDto {
    pub fn from(location: Location, repository: &Repository) -> Option<Self> {
        match location {
            Location::Area(id) => repository.area_by_id(&id).map(|val| {
                let coordinate: Coordinate = repository
                    .stops_by_area_idx(val.index)
                    .into_iter()
                    .map(|stop| stop.coordinate)
                    .sum();
                LocationDto {
                    kind: "area".into(),
                    id: val.id.to_string(),
                    name: val.name.to_string(),
                    coordinate,
                }
            }),
            Location::Stop(id) => repository.stop_by_id(&id).map(|val| LocationDto {
                kind: "stop".into(),
                id: val.id.to_string(),
                name: val.name.to_string(),
                coordinate: val.coordinate,
            }),
            Location::Coordinate(coordinate) => Some(coordinate.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegStopDto {
    pub location: LocationDto,
    pub scheduled_departure_time: Time,
    pub actual_departure_time: Time,
    pub scheduled_arrival_time: Time,
    pub actual_arrival_time: Time,
    pub distance_traveled: Option<f32>,
}

impl LegStopDto {
    pub fn from(leg_stop: LegStop, repository: &Repository) -> Option<Self> {
        Some(Self {
            location: LocationDto::from(leg_stop.location, repository)?,
            scheduled_departure_time: leg_stop.scheduled_departure_time,
            actual_departure_time: leg_stop.actual_departure_time,
            scheduled_arrival_time: leg_stop.scheduled_arrival_time,
            actual_arrival_time: leg_stop.actual_arrival_time,
            distance_traveled: leg_stop.distance_traveled.map(|value| value.as_meters()),
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LegDto {
    pub from: LocationDto,
    pub to: LocationDto,
    pub scheduled_departure_time: Time,
    pub actual_departure_time: Time,
    pub scheduled_arrival_time: Time,
    pub actual_arrival_time: Time,
    pub stops: Vec<LegStopDto>,
    pub mode: Mode,
    pub head_sign: Option<String>,
    pub long_name: Option<String>,
    pub short_name: Option<String>,
    pub shapes: Option<Vec<ShapeDto>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShapeDto {
    pub location: LocationDto,
    pub sequence: u32,
    pub distance_traveled: Option<f32>,
}
impl From<&Shape> for ShapeDto {
    fn from(value: &Shape) -> Self {
        Self {
            location: value.coordinate.into(),
            sequence: value.sequence,
            distance_traveled: value.distance_traveled.map(|value| value.as_meters()),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub enum Mode {
    // Base modes
    Tram,
    Subway,
    Rail,
    Bus,
    Ferry,
    Walk,
    Transfer,
    Unknown,
}

impl Mode {
    fn from_leg(value: LegType, repository: &Repository) -> Self {
        match value {
            LegType::Transit(trip_idx) => {
                Mode::from(repository.route_by_trip_idx(trip_idx).route_type)
            }
            LegType::Transfer => Mode::Transfer,
            LegType::Walk => Mode::Walk,
        }
    }
}
impl From<i32> for Mode {
    fn from(value: i32) -> Self {
        match value {
            0 | 900..=999 => Mode::Tram,
            1 | 400..=405 => Mode::Subway,
            2 | 100..=199 => Mode::Rail,
            3 | 700..=799 => Mode::Bus,
            4 | 1000..=1099 => Mode::Ferry,
            _ => Mode::Unknown,
        }
    }
}

impl LegDto {
    pub fn from(leg: Leg, repository: &Repository) -> Option<Self> {
        let stops: Option<Vec<_>> = leg
            .stops
            .into_iter()
            .map(|stop| LegStopDto::from(stop, repository))
            .collect();

        let (head_sign, long_name, short_name) = if let LegType::Transit(trip_idx) = leg.leg_type {
            let trip = &repository.trips[trip_idx as usize];
            let head_sign = trip
                .head_sign
                .as_ref()
                .map(|head_sign| head_sign.to_string());
            let route = repository.route_by_trip_idx(trip_idx);
            let long_name = route
                .long_name
                .as_ref()
                .map(|long_name| long_name.to_string());
            let short_name = route
                .short_name
                .as_ref()
                .map(|short_name| short_name.to_string());
            (head_sign, long_name, short_name)
        } else {
            (None, None, None)
        };

        Some(Self {
            from: LocationDto::from(leg.from, repository)?,
            to: LocationDto::from(leg.to, repository)?,
            scheduled_departure_time: leg.scheduled_departure_time,
            actual_departure_time: leg.actual_departure_time,
            scheduled_arrival_time: leg.scheduled_arrival_time,
            actual_arrival_time: leg.actual_arrival_time,
            stops: stops?,
            mode: Mode::from_leg(leg.leg_type, repository),
            head_sign,
            long_name,
            short_name,
            shapes: if let LegType::Transit(trip_idx) = leg.leg_type {
                repository
                    .shapes_by_trip_idx(trip_idx)
                    .map(|value| value.iter().map(ShapeDto::from).collect())
            } else {
                None
            },
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ItineraryDto {
    pub from: LocationDto,
    pub to: LocationDto,
    pub departure_time: Time,
    pub arrival_time: Time,
    pub legs: Vec<LegDto>,
}

impl ItineraryDto {
    pub fn from(itinerary: Itinerary, repository: &Repository) -> Option<Self> {
        let legs: Option<Vec<_>> = itinerary
            .legs
            .into_iter()
            .map(|leg| LegDto::from(leg, repository))
            .collect();

        if let Some(legs) = legs {
            let departure_time = legs.first().map(|leg| leg.actual_departure_time)?;
            let arrival_time = legs.last().map(|leg| leg.actual_arrival_time)?;

            Some(Self {
                from: LocationDto::from(itinerary.from, repository)?,
                to: LocationDto::from(itinerary.to, repository)?,
                legs,
                departure_time,
                arrival_time,
            })
        } else {
            None
        }
    }
}
