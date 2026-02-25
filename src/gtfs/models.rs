use crate::{
    repository::{Area, Route, Stop, StopTime},
    shared::{
        geo::{Coordinate, Distance},
        time::Time,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsStop {
    pub stop_id: String,
    pub stop_name: String,
    pub stop_lat: f32,
    pub stop_lon: f32,
    pub location_type: Option<u8>,
    pub parent_station: Option<String>,
    pub platform_code: Option<String>,
}

impl From<GtfsStop> for Stop {
    fn from(value: GtfsStop) -> Self {
        Self {
            index: u32::MAX,
            id: value.stop_id.into(),
            name: value.stop_name.clone().into(),
            normalized_name: value.stop_name.to_lowercase().into(),
            coordinate: Coordinate {
                latitude: value.stop_lat,
                longitude: value.stop_lon,
            },
            parent_index: None,
        }
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsArea {
    pub area_id: String,
    pub area_name: String,
    pub samtrafiken_area_type: String,
}

impl From<GtfsArea> for Area {
    fn from(value: GtfsArea) -> Self {
        Self {
            index: u32::MAX,
            id: value.area_id.into(),
            name: value.area_name.clone().into(),
            normalized_name: value.area_name.to_lowercase().into(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsRoute {
    pub route_id: String,
    pub agency_id: String,
    pub route_short_name: Option<String>,
    pub route_long_name: Option<String>,
    pub route_type: i32,
    pub route_desc: Option<String>,
}

impl From<GtfsRoute> for Route {
    fn from(value: GtfsRoute) -> Self {
        Self {
            index: u32::MAX,
            id: value.route_id.into(),
            agency_id: value.agency_id.into(),
            short_name: value.route_short_name.map(|val| val.into()),
            long_name: value.route_long_name.map(|val| val.into()),
            route_type: value.route_type,
            route_desc: value.route_desc.map(|val| val.into()),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsAgency {
    pub agency_id: String,
    pub agency_name: String,
    pub agency_url: String,
    pub agency_timezone: String,
    pub agency_lang: String,
    pub agency_fare_url: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsStopArea {
    pub area_id: String,
    pub stop_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsTransfer {
    pub from_stop_id: String,
    pub to_stop_id: String,
    pub transfer_type: String,
    pub min_transfer_time: Option<u32>,
    pub from_trip_id: Option<String>,
    pub to_trip_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsStopTime {
    pub trip_id: String,
    pub arrival_time: String,
    pub departure_time: String,
    pub stop_id: String,
    pub stop_sequence: u32,
    pub stop_headsign: Option<String>,
    pub pickup_type: u8,
    pub drop_off_type: u8,
    pub shape_dist_traveled: Option<f32>,
    pub timepoint: Option<u8>,
    pub pickup_booking_rule_id: Option<String>,
    pub drop_off_booking_rule_id: Option<String>,
}

impl From<GtfsStopTime> for StopTime {
    fn from(value: GtfsStopTime) -> Self {
        Self {
            index: u32::MAX,
            // trip_id: Default::default(),
            trip_idx: u32::MAX,
            // stop_id: Default::default(),
            stop_idx: u32::MAX,
            slice: Default::default(),
            inner_idx: u32::MAX,
            sequence: value.stop_sequence,
            arrival_time: Time::from_hms(&value.arrival_time).unwrap(),
            departure_time: Time::from_hms(&value.departure_time).unwrap(),
            headsign: value.stop_headsign.map(|val| val.into()),
            distance_traveled: value.shape_dist_traveled.map(Distance::from_meters),
            // pickup_type: StopAccessType::Regularly,
            // drop_off_type: StopAccessType::Regularly,
            // timepoint: Timepoint::Exact,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsTrip {
    pub route_id: String,
    pub service_id: String,
    pub trip_id: String,
    pub trip_headsign: Option<String>,
    pub trip_short_name: Option<String>,
    pub direction_id: Option<u8>,
    pub shape_id: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct GtfsShape {
    pub shape_id: String,
    pub shape_pt_lat: f32,
    pub shape_pt_lon: f32,
    pub shape_pt_sequence: u32,
    pub shape_dist_traveled: Option<f32>,
}
