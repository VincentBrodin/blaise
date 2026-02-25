use crate::{
    raptor::{Location, Raptor},
    shared::{
        self,
        geo::{AVERAGE_STOP_DISTANCE, Coordinate, Distance},
    },
};
use std::{collections::HashMap, sync::Arc};

mod entities;
pub mod source;

pub use entities::*;

pub type Cell = (i32, i32);

/// A read-only, memory-efficient data store containing all transit network information.
///
/// The `Repository` acts as a flattened relational database, optimized for high-performance
/// pathfinding algorithms like RAPTOR. It uses `Box<[T]>` instead of `Vec<T>` to minimize
/// memory overhead and signal immutability after construction.
#[derive(Debug, Clone, Default)]
pub struct Repository {
    // --- Core Entities ---
    /// Global list of all physical transit stops or stations.
    pub stops: Box<[Stop]>,
    /// Geographical or logical groupings of stops.
    pub areas: Box<[Area]>,
    /// High-level transit routes (e.g., "Bus 42").
    pub routes: Box<[Route]>,
    /// Specialized route structures where every trip follows an identical stop sequence.
    /// Required for the RAPTOR algorithm's optimization passes.
    pub raptor_routes: Box<[RaptorRoute]>,
    /// Individual vehicle journeys occurring at specific times.
    pub trips: Box<[Trip]>,
    /// The specific arrival/departure events linking trips to stops.
    pub stop_times: Box<[StopTime]>,
    /// All known transfers.
    pub transfers: Box<[Transfer]>,
    /// All the shapes.
    pub shapes: Box<[Shape]>,

    // --- Primary Key Lookups ---
    /// Maps a unique `Stop.id` string to its index within the `stops` slice.
    stop_lookup: HashMap<Arc<str>, u32>,
    /// Maps a unique `Trip.id` string to its index within the `trips` slice.
    trip_lookup: HashMap<Arc<str>, u32>,
    /// Maps a unique `Area.id` string to its index within the `areas` slice.
    area_lookup: HashMap<Arc<str>, u32>,
    /// Maps a unique `Route.id` string to its index within the `routes` slice.
    route_lookup: HashMap<Arc<str>, u32>,
    /// Spatial index used to find stops within specific grid cells.
    stop_distance_lookup: HashMap<Cell, Box<[u32]>>,

    // --- Relationship Indicies (Adjacency Lists) ---
    /// Index mapping: `route_index -> [trip_index, ...]`.
    pub(crate) route_to_trips: Box<[Box<[u32]>]>,
    /// Index mapping: `trip_index -> route_index`.
    pub(crate) trip_to_route: Box<[u32]>,
    /// Index mapping: `area_index -> [stop_index, ...]`.
    pub(crate) area_to_stops: Box<[Box<[u32]>]>,
    /// Index mapping: `stop_index -> area_index`.
    pub(crate) stop_to_area: Box<[Option<u32>]>,
    /// Index mapping: `stop_index -> [stop_index, ...]`.
    pub(crate) station_to_stops: Box<[Box<[u32]>]>,
    /// Index mapping: `from.stop_index -> [transfer_index, ...]`.
    pub(crate) stop_to_transfers: Box<[Box<[u32]>]>,
    /// Index mapping: `stop_index -> [trip_index, ...]`.
    pub(crate) stop_to_trips: Box<[Box<[u32]>]>,
    /// Defines the range within the `stop_times` slice that belongs to a specific trip.
    pub(crate) trip_to_stop_times_slice: Box<[Slice]>,
    /// Defines the range within the `shapes` slice that belongs to a specific raptor route.

    // --- RAPTOR Specialized Lookups ---
    /// Maps a standard route index to its corresponding `RaptorRoute` versions.
    pub(crate) route_to_raptors: Box<[Box<[u32]>]>,
    /// Maps a stop index to all `RaptorRoute` indices that serve it.
    pub(crate) stop_to_raptors: Box<[Box<[u32]>]>,
    /// Maps a stop index to all walkable stops near it.
    pub(crate) stop_to_walk_stop: Box<[Box<[u32]>]>,
    /// Maps a stop index to all walkable stops near it.
    pub(crate) raptor_to_shapes_slice: Box<[Option<Slice>]>,
}

impl Repository {
    /// Creates a new, empty repository instance.
    pub fn new() -> Self {
        Default::default()
    }

    /// Initializes a new RAPTOR router instance tied to the lifetime of this repository.
    ///
    /// This is the entry point for performing pathfinding between two locations.
    pub fn router(&'_ self, from: Location, to: Location) -> Raptor<'_> {
        Raptor::new(self, from, to)
    }

    // --- Primary Key Lookups Functions ---

    /// Retrieves a [`Stop`] by its string identifier `Stop.id`.
    /// Returns `None` if the ID does not exist.
    pub fn stop_by_id(&self, id: &str) -> Option<&Stop> {
        let stop_index = self.stop_lookup.get(id)?;
        Some(&self.stops[*stop_index as usize])
    }

    /// Retrieves a [`Area`] by its string identifier `Area.id`.
    /// Returns `None` if the ID does not exist.
    pub fn area_by_id(&self, id: &str) -> Option<&Area> {
        let area_index = self.area_lookup.get(id)?;
        Some(&self.areas[*area_index as usize])
    }

    /// Retrieves a [`Trip`] by its string identifier `Trip.id`.
    /// Returns `None` if the ID does not exist.
    pub fn trip_by_id(&self, id: &str) -> Option<&Trip> {
        let trip_index = self.trip_lookup.get(id)?;
        Some(&self.trips[*trip_index as usize])
    }

    /// Retrieves a [`Route`] by its string identifier `Route.id`.
    /// Returns `None` if the ID does not exist.
    pub fn route_by_id(&self, id: &str) -> Option<&Route> {
        let index = self.route_lookup.get(id)?;
        Some(&self.routes[*index as usize])
    }

    // --- Relationship Indicies (Adjacency Lists) Functions ---
    /// Returns a list of all stops contained within a specific parent area.
    pub fn stops_by_area_idx(&self, area_idx: u32) -> Vec<&Stop> {
        self.area_to_stops[area_idx as usize]
            .iter()
            .flat_map(|stop_idx| {
                let children = &self.station_to_stops[*stop_idx as usize];
                children
                    .iter()
                    .map(|stop_idx| &self.stops[*stop_idx as usize])
            })
            .collect()
    }

    /// Returns a list of all stops contained within a specific parent stop.
    pub fn stops_by_station(&self, stop_idx: u32) -> Vec<&Stop> {
        self.station_to_stops[stop_idx as usize]
            .iter()
            .map(|stop_idx| &self.stops[*stop_idx as usize])
            .collect()
    }

    /// Returns the parent [`Area`] for a given [`Stop`] using it's index (`Stop.index`).
    pub fn area_by_stop_idx(&self, stop_idx: u32) -> Option<&Area> {
        let area_idx = self.stop_to_area[stop_idx as usize]?;
        Some(&self.areas[area_idx as usize])
    }

    /// Calculates the centroid/representative coordinate of an area by
    /// averaging the coordinates of all stops within it.
    pub fn coordinate_by_area_idx(&self, area_idx: u32) -> Coordinate {
        self.stops_by_area_idx(area_idx)
            .iter()
            .map(|stop| stop.coordinate)
            .sum()
    }

    /// Retrieves all outbound [`Transfer`] connections available from a specific [`Stop`] using it's index (`Stop.index`).
    pub fn transfers_by_stop_idx(&self, stop_idx: u32) -> Vec<&Transfer> {
        let transfers = &self.stop_to_transfers[stop_idx as usize];
        transfers
            .iter()
            .map(|transfer_idx| &self.transfers[*transfer_idx as usize])
            .collect()
    }

    /// Finds all trips that call at a specific [`Stop`] using it's index (`Stop.index`).
    pub fn trips_by_stop_idx(&self, stop_idx: u32) -> Vec<&Trip> {
        self.stop_to_trips[stop_idx as usize]
            .iter()
            .map(|trip_idx| &self.trips[*trip_idx as usize])
            .collect()
    }

    /// Returns true if a specific [`Stop`] using it's index (`Stop.index`) has any trips connected to it.
    pub fn stop_idx_has_trips(&self, stop_idx: u32) -> bool {
        !self.stop_to_trips[stop_idx as usize].is_empty()
    }

    /// Identifies which high-level [`Route`] a specific [`Trip`] belongs to using it's index (`Trip.index`).
    pub fn route_by_trip_idx(&self, trip_idx: u32) -> &Route {
        let route_idx = self.trip_to_route[trip_idx as usize];
        &self.routes[route_idx as usize]
    }

    /// Retrieves all scheduled trips for a specific [`Route`].
    pub fn trips_by_route_idx(&self, route_idx: u32) -> Vec<&Trip> {
        self.route_to_trips[route_idx as usize]
            .iter()
            .map(|trip_idx| &self.trips[*trip_idx as usize])
            .collect()
    }

    /// Retrieves the full schedule (arrival/departure times) for every trip on a [`Route`].
    pub fn stop_times_by_route_idx(&self, route_idx: u32) -> Vec<&[StopTime]> {
        self.route_to_trips[route_idx as usize]
            .iter()
            .map(|trip_idx| self.stop_times_by_trip_idx(*trip_idx))
            .collect()
    }

    /// Efficiently retrieves a slice of [`StopTime`] entries for a specific trip.
    ///
    /// This uses a pre-computed pointer slice (start/count) into the global
    /// `stop_times` array for `O(1)` access.
    pub fn stop_times_by_trip_idx(&self, trip_idx: u32) -> &[StopTime] {
        let slice = self.trip_to_stop_times_slice[trip_idx as usize];
        let start = slice.start_idx as usize;
        let end = start + slice.count as usize;
        &self.stop_times[start..end]
    }

    /// Efficiently retrieves a slice of [`Shape`] entries for a specific trip.
    ///
    /// This uses a pre-computed pointer slice (start/count) into the global
    /// `shapes` array for `O(1)` access.
    pub fn shapes_by_trip_idx(&self, trip_idx: u32) -> Option<&[Shape]> {
        let trip = &self.trips[trip_idx as usize];
        let slice = self.raptor_to_shapes_slice[trip.raptor_route_idx as usize]?;
        let start = slice.start_idx as usize;
        let end = start + slice.count as usize;
        Some(&self.shapes[start..end])
    }

    /// Spatial query: Returns all stops within a certain distance of a coordinate.
    ///
    /// This uses a grid-based cell lookup for performance, followed by an
    /// exact distance filter using the network distance metric.
    pub fn stops_by_coordinate(&self, coordinate: &Coordinate, distance: Distance) -> Vec<&Stop> {
        let reach = (distance / AVERAGE_STOP_DISTANCE).as_meters().ceil().abs() as i32 + 1;
        let (origin_x, origin_y) = coordinate.to_cell();
        (-reach..=reach)
            .flat_map(|x| {
                (-reach..=reach)
                    .flat_map(move |y| {
                        let cell = (origin_x + x, origin_y + y);
                        if let Some(stop_idxs) = self.stop_distance_lookup.get(&cell) {
                            stop_idxs
                                .iter()
                                .filter_map(|stop_idx| {
                                    let stop = &self.stops[*stop_idx as usize];
                                    if stop.coordinate.network_distance(coordinate) <= distance {
                                        Some(stop)
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                        } else {
                            Vec::new()
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Spatial query: Returns all logical areas within range of a coordinate.  
    pub fn areas_by_coordinate(&self, coordinate: &Coordinate, distance: Distance) -> Vec<&Area> {
        let stops = self.stops_by_coordinate(coordinate, distance);
        stops
            .into_iter()
            .filter_map(|stop| self.area_by_stop_idx(stop.index))
            .collect()
    }

    // --- RAPTOR Specialized Lookups Functions ---
    /// Returns the optimized `RaptorRoute` variations for a given standard route.
    pub fn raptors_by_route_idx(&self, route_idx: u32) -> Vec<&RaptorRoute> {
        self.route_to_raptors[route_idx as usize]
            .iter()
            .map(|raptor_idx| &self.raptor_routes[*raptor_idx as usize])
            .collect()
    }

    /// Identifies which optimized RAPTOR routes pass through a specific stop.
    pub fn raptors_by_stop_idx(&self, stop_idx: u32) -> Vec<&RaptorRoute> {
        self.stop_to_raptors[stop_idx as usize]
            .iter()
            .map(|raptor_idx| &self.raptor_routes[*raptor_idx as usize])
            .collect()
    }

    /// Returns all possible walkable stops by stop_idx.  
    pub fn nearby_stops_by_stop_idx(&self, stop_idx: u32) -> Vec<&Stop> {
        let stops = &self.stop_to_walk_stop[stop_idx as usize];
        stops
            .iter()
            .map(|stop_idx| &self.stops[*stop_idx as usize])
            .collect()
    }

    // --- Fuzzy ---

    /// Performs a fuzzy text search against area names to find matches for partial user input.
    pub fn search_areas_by_name<'a>(&'a self, needle: &'a str) -> Vec<&'a Area> {
        shared::search(needle, &self.areas)
    }

    /// Performs a fuzzy text search against stop names (e.g., for autocomplete).
    pub fn search_stops_by_name<'a>(&'a self, needle: &'a str) -> Vec<&'a Stop> {
        shared::search(needle, &self.stops)
    }
}
