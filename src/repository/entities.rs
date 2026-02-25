use std::sync::Arc;

use crate::shared::{
    Identifiable,
    geo::{Coordinate, Distance},
    time::{Duration, Time},
};

/// Represents a logical grouping of stops, such as a large transit center,
/// a city district.
#[derive(Debug, Default, Clone)]
pub struct Area {
    /// The global internal index used for O(1) array lookups in the repository.
    pub index: u32,
    /// The unique external identifier.
    pub id: Arc<str>,
    /// The display name of the area.
    pub name: Arc<str>,

    /// A search-optimized version of the name (e.g., lowercase, stripped of accents).
    pub normalized_name: Arc<str>,
}

impl Identifiable for Area {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn normalized_name(&self) -> &str {
        &self.normalized_name
    }
}

/// Categorizes the specific nature of a transit stop location.
#[derive(Debug, Default, Clone)]
pub enum LocationType {
    /// A standard bus stop or platform.
    #[default]
    Stop,
    /// A specific platform within a larger station.
    Platform {
        /// ID of the parent station.
        parent_station: Arc<str>,
        /// The alphanumeric code for the platform (e.g., "4B").
        platform_code: Arc<str>,
    },
    /// A major transit hub or rail station containing multiple platforms.
    Station,
    /// A specific physical entrance to a station.
    Entrance(Arc<str>),
    /// A generic node in the transit network (often used for logical junctions).
    Node,
    /// A specific designated boarding point.
    Boarding,
}

/// A physical point where passengers can board or alight from a vehicle.
#[derive(Debug, Default, Clone)]
pub struct Stop {
    /// The global internal index for this stop.
    pub index: u32,
    /// Unique external identifier for the stop.
    pub id: Arc<str>,
    /// Human-readable name (e.g., "Main St & 4th Ave").
    pub name: Arc<str>,
    /// Normalized name used for fuzzy search comparisons.
    pub normalized_name: Arc<str>,
    pub coordinate: Coordinate,
    /// The index of the parent station/platform
    pub parent_index: Option<u32>,
    // The specific GTFS location classification.
    // pub location_type: LocationType,
}

impl Identifiable for Stop {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn normalized_name(&self) -> &str {
        &self.normalized_name
    }
}

#[derive(Debug, Default, Clone)]
pub enum Timepoint {
    #[default]
    Approximate,
    Exact,
}

#[derive(Debug, Default, Clone)]
pub enum StopAccessType {
    #[default]
    Regularly,
    NoneAvailable,
    AgencyArrange,
    DriverArrange,
}

/// Individual event within a trip where a vehicle calls at a stop.
#[derive(Debug, Default, Clone)]
pub struct StopTime {
    /// Global internal index of this stop-time record.
    pub index: u32,
    /// Internal index of the parent [`Trip`].
    pub trip_idx: u32,
    /// Internal index of the associated [`Stop`].
    pub stop_idx: u32,
    /// The order of this stop within the trip (starts from 1).
    pub sequence: u32,
    /// Pointer to the full range of stop times for the parent trip.
    pub slice: Slice,
    /// Zero-based position of this stop within its specific trip.
    pub inner_idx: u32,
    /// Scheduled arrival time (stored as seconds since midnight).
    pub arrival_time: Time,
    /// Scheduled departure time (stored as seconds since midnight).
    pub departure_time: Time,
    /// Destination shown to passengers when at this stop.
    pub headsign: Option<Arc<str>>,
    /// Cumulative distance traveled along the trip's shape.
    pub distance_traveled: Option<Distance>,
    // Policy for passenger boarding (Regular, No Pickup, etc.).
    // pub pickup_type: StopAccessType,
    // Policy for passenger alighting.
    // pub drop_off_type: StopAccessType,
    // Indicates if times are exact or estimates.
    // pub timepoint: Timepoint,
}

/// Metadata describing a contiguous range within a global array.
#[derive(Default, Debug, Clone, Copy)]
pub struct Slice {
    /// The index where the data begins.
    pub start_idx: u32,
    /// The total number of elements in the array.
    pub count: u32,
}

/// A connection between two points in the network, often representing walking or shuttle legs.
#[derive(Debug, Default, Clone)]
pub struct Transfer {
    pub from_stop_idx: u32,
    pub to_stop_idx: u32,

    /// If present, this transfer is only valid when arriving on this specific trip.
    pub from_trip_idx: Option<u32>,
    /// If present, this transfer is only valid when departing on this specific trip.
    pub to_trip_idx: Option<u32>,
    /// The minimum time (in seconds) required to successfully complete this transfer.
    pub min_transfer_time: Option<Duration>,
}

/// A specific journey taken by a vehicle through a sequence of stops.
#[derive(Debug, Default, Clone)]
pub struct Trip {
    pub index: u32,
    pub id: Arc<str>,
    /// Pointer to the parent [`Route`].
    pub route_idx: u32,
    /// Pointer to the optimized [`RaptorRoute`] used by the routing engine.
    pub raptor_route_idx: u32,
    pub head_sign: Option<Arc<str>>,
    pub short_name: Option<Arc<str>>,
}

/// A grouping of trips that are displayed to riders under a single name (e.g., "Blue Line").
#[derive(Debug, Default, Clone)]
pub struct Route {
    pub index: u32,
    pub id: Arc<str>,
    pub agency_id: Arc<str>,
    pub short_name: Option<Arc<str>>,
    pub long_name: Option<Arc<str>>,
    /// Classification of the vehicle (0: Tram, 1: Subway, 3: Bus, etc.).
    pub route_type: i32,
    pub route_desc: Option<Arc<str>>,
}

#[derive(Debug, Default, Clone)]
pub struct Shape {
    pub index: u32,
    pub coordinate: Coordinate,
    pub sequence: u32,
    pub inner_idx: u32,
    pub distance_traveled: Option<Distance>,
    pub slice: Slice,
}

/// An optimized route structure strictly for the RAPTOR algorithm.
///
/// Unlike a standard [`Route`], a `RaptorRoute` guarantees that every trip
/// within it shares the *exact same stop sequence*.
#[derive(Debug, Default, Clone)]
pub struct RaptorRoute {
    /// Internal index of this RAPTOR-specific route.
    pub index: u32,
    /// Pointer back to the display-level [`Route`].
    pub route_idx: u32,
    /// List of stop indices served by this route in order.
    pub stops: Arc<[u32]>,
    /// List of trip indices that follow this stop sequence.
    pub trips: Arc<[u32]>,
}
