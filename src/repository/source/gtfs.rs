use crate::{
    gtfs::{
        GtfsArea, GtfsData, GtfsRoute, GtfsShape, GtfsStop, GtfsStopArea, GtfsStopTime,
        GtfsTransfer, GtfsTrip,
    },
    raptor::get_departure_time,
    realtime::Realtime,
    repository::{
        Area, Cell, RaptorRoute, Repository, Route, Shape, Slice, Stop, StopTime, Transfer, Trip,
    },
    shared::{
        AVERAGE_STOP_DISTANCE, Coordinate, Distance, Time, time::Duration, utils::StrSliceMap,
    },
};
use dashmap::DashMap;
use rayon::prelude::*;
use std::{collections::HashMap, time::Instant};
use tracing::debug;

impl Repository {
    pub fn load_gtfs(mut self, gtfs: GtfsData) -> Self {
        self.load_stops(gtfs.stops);
        self.load_areas(gtfs.areas);
        self.load_area_to_stops(gtfs.stop_areas);
        let shapes_lookup = self.load_shapes(gtfs.shapes);
        self.load_routes(gtfs.routes);
        let trip_to_shape_slice = self.load_trips(gtfs.trips, shapes_lookup);
        self.load_transfers(gtfs.transfers);
        self.load_stop_times(gtfs.stop_times);
        self.generate_geo_hash();
        self.generate_raptor_routes(trip_to_shape_slice);
        self.generate_walks();
        self
    }

    fn load_stops(&mut self, gtfs_stops: Vec<GtfsStop>) {
        debug!("Loading stops...");
        let now = Instant::now();
        let mut stops: Vec<(Stop, Option<String>)> = Vec::with_capacity(gtfs_stops.len());
        let mut strings = StrSliceMap::new();
        gtfs_stops
            .into_iter()
            .enumerate()
            .for_each(|(i, mut stop)| {
                let parent_station = stop.parent_station.take();
                let value = Stop {
                    index: i as u32,
                    id_slice: strings.get_slice(stop.stop_id),
                    name_slice: strings.get_slice(stop.stop_name),
                    coordinate: Coordinate {
                        latitude: stop.stop_lat,
                        longitude: stop.stop_lon,
                    },
                    parent_index: None,
                };
                stops.push((value, parent_station));
            });

        self.stop_strs = strings.take().into_boxed_str();
        dbg!(&self.stop_strs);
        let mut lookup_vec: Vec<u32> = (0..stops.len() as u32).collect();
        lookup_vec.par_sort_unstable_by(|&a, &b| {
            let id_a = self.stop_str_by_slice(&stops[a as usize].0.id_slice);
            let id_b = self.stop_str_by_slice(&stops[b as usize].0.id_slice);
            id_a.cmp(id_b)
        });
        self.stop_lookup = lookup_vec.into_boxed_slice();

        let mut station_to_stops: Vec<Vec<u32>> = vec![Vec::new(); stops.len()];
        stops
            .iter_mut()
            .filter_map(|(stop, parent_station_id)| {
                if let Some(parent_station_id) = parent_station_id {
                    self.stop_by_id(parent_station_id.as_str())
                        .map(|parent_station| (parent_station.index, stop))
                } else {
                    None
                }
            })
            .for_each(|(parent_station, stop)| {
                station_to_stops[parent_station as usize].push(stop.index);
                stop.parent_index = Some(parent_station);
            });

        self.stops = stops.into_iter().map(|(stop, _)| stop).collect();
        self.station_to_stops = station_to_stops
            .into_iter()
            .map(|stops| stops.into())
            .collect();

        debug!(
            "Loading {} stops took {:?}",
            self.stops.len(),
            now.elapsed()
        );
    }

    fn load_areas(&mut self, gtfs_areas: Vec<GtfsArea>) {
        debug!("Loading areas...");
        let now = Instant::now();
        let mut areas: Vec<Area> = Vec::with_capacity(gtfs_areas.len());
        let mut strings = StrSliceMap::new();
        gtfs_areas.into_iter().enumerate().for_each(|(i, area)| {
            let value = Area {
                index: i as u32,
                id_slice: strings.get_slice(area.area_id),
                name_slice: strings.get_slice(area.area_name),
            };
            areas.push(value);
        });
        self.areas = areas.into();

        self.area_strs = strings.take().into_boxed_str();
        let mut lookup_vec: Vec<u32> = (0..self.areas.len() as u32).collect();
        lookup_vec.par_sort_unstable_by(|&a, &b| {
            let id_a = self.area_str_by_slice(&self.areas[a as usize].id_slice);
            let id_b = self.area_str_by_slice(&self.areas[b as usize].id_slice);
            id_a.cmp(id_b)
        });
        self.area_lookup = lookup_vec.into_boxed_slice();

        debug!(
            "Loading {} areas took {:?}",
            self.areas.len(),
            now.elapsed()
        );
    }

    fn load_area_to_stops(&mut self, gtfs_stop_areas: Vec<GtfsStopArea>) {
        debug!("Loading area to stops...");
        let now = Instant::now();

        let mut area_to_stops: Vec<Vec<u32>> = vec![Vec::new(); self.areas.len()];
        let mut stop_to_area: Vec<Option<u32>> = vec![None; self.stops.len()];
        gtfs_stop_areas.into_iter().for_each(|value| {
            // TEMP
            let stop = self
                .stop_by_id(value.stop_id.as_str())
                .expect("Could not find stop by id");
            // TEMP
            let area = self
                .area_by_id(value.area_id.as_str())
                .expect("Could not find stop by id");

            stop_to_area[stop.index as usize] = Some(area.index);
            area_to_stops[area.index as usize].push(stop.index);
        });
        self.stop_to_area = stop_to_area.into();
        let area_to_stops: Box<[Box<[u32]>]> =
            area_to_stops.into_iter().map(|val| val.into()).collect();
        self.area_to_stops = area_to_stops;
        debug!(
            "Loading {} area to stops took {:?}",
            self.area_to_stops.len(),
            now.elapsed()
        );
    }

    fn load_shapes(&mut self, gtfs_shapes: Vec<GtfsShape>) -> HashMap<String, Slice> {
        debug!("Loading shapes...");
        let now = Instant::now();
        let shapes: DashMap<String, Vec<Shape>> = DashMap::with_capacity(gtfs_shapes.len());
        gtfs_shapes.into_par_iter().for_each(|value| {
            let shape = Shape {
                index: u32::MAX,
                coordinate: Coordinate::new(value.shape_pt_lat, value.shape_pt_lon),
                sequence: value.shape_pt_sequence,
                distance_traveled: value.shape_dist_traveled.map(Distance::from_meters),
                slice: Slice::default(),
                inner_idx: u32::MAX,
            };
            shapes.entry(value.shape_id).or_default().push(shape);
        });

        let mut idx = 0;
        let mut shapes_lookup: HashMap<String, Slice> = HashMap::with_capacity(shapes.len());
        let shapes: Vec<_> = shapes
            .into_iter()
            .flat_map(|(id, mut shapes)| {
                let slice = Slice {
                    start_idx: idx,
                    count: shapes.len() as u32,
                };
                shapes_lookup.entry(id).or_insert(slice);

                shapes.par_sort_by_key(|value| value.sequence);
                shapes.iter_mut().enumerate().for_each(|(i, shape)| {
                    shape.slice = slice;
                    shape.inner_idx = i as u32;
                    shape.index = slice.start_idx + shape.inner_idx;
                    idx += 1;
                });
                shapes
            })
            .collect();

        self.shapes = shapes.into();
        debug!(
            "Loading {} shapes took {:?}",
            self.shapes.len(),
            now.elapsed()
        );
        shapes_lookup
    }

    fn load_routes(&mut self, gtfs_routes: Vec<GtfsRoute>) {
        debug!("Loading routes...");
        let now = Instant::now();
        let mut routes: Vec<Route> = Vec::with_capacity(gtfs_routes.len());
        let mut strings = StrSliceMap::new();
        gtfs_routes
            .into_iter()
            .enumerate()
            .for_each(|(i, mut route)| {
                let value = Route {
                    index: i as u32,
                    id_slice: strings.get_slice(route.route_id),
                    short_name_slice: route
                        .route_short_name
                        .take()
                        .map(|val| strings.get_slice(val)),
                    long_name_slice: route
                        .route_long_name
                        .take()
                        .map(|val| strings.get_slice(val)),
                    route_type: route.route_type,
                    route_desc_slice: route.route_desc.take().map(|val| strings.get_slice(val)),
                };
                routes.push(value);
            });
        self.routes = routes.into_boxed_slice();
        self.route_strs = strings.take().into_boxed_str();
        let mut lookup_vec: Vec<u32> = (0..self.routes.len() as u32).collect();
        lookup_vec.par_sort_unstable_by(|&a, &b| {
            let id_a = self.route_str_by_slice(&self.routes[a as usize].id_slice);
            let id_b = self.route_str_by_slice(&self.routes[b as usize].id_slice);
            id_a.cmp(id_b)
        });
        self.route_lookup = lookup_vec.into_boxed_slice();

        debug!(
            "Loading {} routes took {:?}",
            self.routes.len(),
            now.elapsed()
        );
    }

    fn load_trips(
        &mut self,
        gtfs_trips: Vec<GtfsTrip>,
        shapes_lookup: HashMap<String, Slice>,
    ) -> Vec<Option<Slice>> {
        debug!("Loading trips...");
        let now = Instant::now();
        let mut trip_to_shapes_slice: Vec<Option<Slice>> = Vec::with_capacity(gtfs_trips.len());
        let mut route_to_trips: Vec<Vec<u32>> = vec![Vec::new(); self.routes.len()];
        let mut trip_to_route: Vec<u32> = Vec::with_capacity(gtfs_trips.len());
        let mut trips: Vec<Trip> = Vec::with_capacity(gtfs_trips.len());
        let mut strings = StrSliceMap::new();

        gtfs_trips
            .into_iter()
            .enumerate()
            .for_each(|(i, mut trip)| {
                let shape_slice = trip
                    .shape_id
                    .and_then(|shape_id| shapes_lookup.get(&shape_id))
                    .copied();
                trip_to_shapes_slice.push(shape_slice);
                let route = self
                    .route_by_id(trip.route_id.as_str())
                    .expect("Could not get route by id");
                let value = Trip {
                    index: i as u32,
                    id_slice: strings.get_slice(trip.trip_id),
                    route_idx: route.index,
                    raptor_route_idx: 0,
                    headsign_slice: trip.trip_headsign.take().map(|val| strings.get_slice(val)),
                    short_name_slice: trip
                        .trip_short_name
                        .take()
                        .map(|val| strings.get_slice(val)),
                };
                route_to_trips[route.index as usize].push(i as u32);
                trip_to_route.push(route.index);
                trips.push(value);
            });
        self.trips = trips.into_boxed_slice();
        self.trip_strs = strings.take().into_boxed_str();

        let mut lookup_vec: Vec<u32> = (0..self.trips.len() as u32).collect();
        lookup_vec.par_sort_unstable_by(|&a, &b| {
            let id_a = self.trip_str_by_slice(&self.trips[a as usize].id_slice);
            let id_b = self.trip_str_by_slice(&self.trips[b as usize].id_slice);
            id_a.cmp(id_b)
        });
        self.trip_lookup = lookup_vec.into_boxed_slice();

        self.trip_to_route = trip_to_route.into();
        let route_to_trips: Box<[Box<[u32]>]> =
            route_to_trips.into_iter().map(|val| val.into()).collect();
        self.route_to_trips = route_to_trips;
        debug!(
            "Loading {} trips took {:?}",
            self.trips.len(),
            now.elapsed()
        );
        trip_to_shapes_slice
    }

    fn load_transfers(&mut self, gtfs_transfers: Vec<GtfsTransfer>) {
        debug!("Loading transfers...");
        let now = Instant::now();
        let mut transfers: Vec<Transfer> = Vec::with_capacity(gtfs_transfers.len());
        let mut stop_to_transfers: Vec<Vec<u32>> = vec![Vec::new(); self.stops.len()];
        gtfs_transfers
            .into_iter()
            .enumerate()
            .for_each(|(i, transfer)| {
                let from_stop = self
                    .stop_by_id(transfer.from_stop_id.as_str())
                    .expect("Could not get stop by id");

                let to_stop = self
                    .stop_by_id(transfer.to_stop_id.as_str())
                    .expect("Could not get stop by id");

                let from_trip_idx = if let Some(trip_id) = transfer.from_trip_id {
                    let trip = self
                        .stop_by_id(trip_id.as_str())
                        .expect("Could not get trip by id");
                    Some(trip.index)
                } else {
                    None
                };

                let to_trip_idx = if let Some(trip_id) = transfer.to_trip_id {
                    let trip = self
                        .stop_by_id(trip_id.as_str())
                        .expect("Could not get trip by id");
                    Some(trip.index)
                } else {
                    None
                };

                stop_to_transfers[from_stop.index as usize].push(i as u32);

                let value = Transfer {
                    from_stop_idx: from_stop.index,
                    to_stop_idx: to_stop.index,
                    from_trip_idx,
                    to_trip_idx,
                    min_transfer_time: transfer.min_transfer_time.map(Duration::from_seconds),
                };

                transfers.push(value);
            });
        self.transfers = transfers.into();
        self.stop_to_transfers = stop_to_transfers
            .into_iter()
            .map(|val| val.into())
            .collect();
        debug!(
            "Loading {} transfers took {:?}",
            self.transfers.len(),
            now.elapsed()
        );
    }

    fn load_stop_times(&mut self, gtfs_stop_times: Vec<GtfsStopTime>) {
        debug!("Loading stop times...");
        let now = Instant::now();
        let stop_times_map: DashMap<String, Vec<(StopTime, Option<String>)>> =
            DashMap::with_capacity(self.trips.len());
        let mut trip_to_stop_times_slice: Vec<Slice> = vec![Default::default(); self.trips.len()];
        let mut stop_to_trips: Vec<Vec<u32>> = vec![Vec::new(); self.stops.len()];

        let mut strings = StrSliceMap::new();

        gtfs_stop_times.into_par_iter().for_each(|value| {
            let stop = &self
                .stop_by_id(value.stop_id.as_str())
                .expect("Failed to find stop by id");
            let stop_time = StopTime {
                index: u32::MAX,
                trip_idx: u32::MAX,
                stop_idx: stop.index,
                sequence: value.stop_sequence,
                slice: Default::default(),
                inner_idx: u32::MAX,
                arrival_time: Time::from_hms(&value.arrival_time).expect("Invalid time format"),
                departure_time: Time::from_hms(&value.departure_time).expect("Invalid time format"),
                headsign: Default::default(),
                distance_traveled: value.shape_dist_traveled.map(Distance::from_meters),
            };

            stop_times_map
                .entry(value.trip_id)
                .or_default()
                .push((stop_time, value.stop_headsign));
        });

        let mut idx: u32 = 0;

        let stop_times: Vec<_> = stop_times_map
            .into_iter()
            .flat_map(|(trip_id, mut stop_times)| {
                let trip = self
                    .trip_by_id(trip_id.as_str())
                    .expect("Failed to find trip by id");
                let count = stop_times.len() as u32;
                let slice = Slice {
                    start_idx: idx,
                    count,
                };

                trip_to_stop_times_slice[trip.index as usize] = slice;

                stop_times.par_sort_by_key(|s| s.0.sequence);
                let stop_times: Vec<_> = stop_times
                    .into_iter()
                    .enumerate()
                    .map(|(i, (mut s, mut headsign))| {
                        let i = i as u32;
                        s.index = idx + i;
                        s.inner_idx = i;
                        s.slice = slice;
                        s.trip_idx = trip.index;
                        s.headsign = headsign.take().map(|val| strings.get_slice(val));
                        stop_to_trips[s.stop_idx as usize].push(s.trip_idx);
                        s
                    })
                    .collect();
                idx += count;
                stop_times
            })
            .collect();

        self.stop_times = stop_times.into_boxed_slice();
        self.stop_time_strs = strings.take().into_boxed_str();
        self.trip_to_stop_times_slice = trip_to_stop_times_slice.into();
        let stop_to_trips: Box<[Box<[u32]>]> =
            stop_to_trips.into_iter().map(|val| val.into()).collect();
        self.stop_to_trips = stop_to_trips;

        debug!(
            "Loading {} stop times took {:?}",
            self.stop_times.len(),
            now.elapsed()
        );
    }

    fn generate_geo_hash(&mut self) {
        // Link area->stop->real world stop (stops that are linked to any trip)
        // This has to be last because it ties togheter alot
        // To save space and not having a O(n^2) operation trying to map each stop
        // to its nearby stops, we are going to map each stop with trips into a grid
        debug!("Generating geo spatial hash...");
        let now = Instant::now();
        let mut stop_distance_lookup: HashMap<Cell, Vec<u32>> = HashMap::new();
        self.stops.iter().for_each(|stop| {
            let cell = stop.coordinate.to_cell();
            stop_distance_lookup
                .entry(cell)
                .or_default()
                .push(stop.index);
        });
        let stop_distance_lookup: HashMap<Cell, Box<[u32]>> = stop_distance_lookup
            .into_iter()
            .map(|(cell, stops)| (cell, stops.into()))
            .collect();
        self.stop_distance_lookup = stop_distance_lookup;
        debug!("Generating geo spatial hash took {:?}", now.elapsed());
    }

    fn generate_raptor_routes(&mut self, trip_to_shapes_slice: Vec<Option<Slice>>) {
        // Raptor mappings
        // Raptor requires each route's trips to have an identical set of stops.
        // Gtfs does not have this requirement, so we split each route
        // into sub routes that matches these requirements.
        // To build this we are going to go through each route and grab each trip in that route
        // then we are going through each trip and grouping them by there stops
        // with the split trips we can create the raptor route.
        debug!("Generating raptor routes...");
        let now = Instant::now();
        let realtime = Realtime::new(self);
        let mut raptor_routes: Vec<RaptorRoute> = Vec::new();
        let mut route_to_raptors: Vec<Vec<u32>> = vec![Vec::new(); self.routes.len()];
        let mut stop_to_raptors: Vec<Vec<u32>> = vec![Vec::new(); self.stops.len()];
        let mut raptor_to_shapes_slice: Vec<Option<Slice>> = Vec::new();
        self.routes.iter().for_each(|route| {
            let trips = self.stop_times_by_route_idx(route.index);
            let mut raptor_trips: HashMap<Vec<u32>, Vec<u32>> = HashMap::new();
            trips.into_iter().for_each(|trip| {
                let index = trip.first().unwrap().trip_idx;
                let signature: Vec<_> = trip.iter().map(|st| st.stop_idx).collect();
                raptor_trips.entry(signature).or_default().push(index);
            });

            raptor_trips.into_iter().for_each(|(key, mut value)| {
                let index = raptor_routes.len();
                key.iter().for_each(|stop_idx| {
                    stop_to_raptors[*stop_idx as usize].push(index as u32);
                });
                route_to_raptors[route.index as usize].push(index as u32);

                value.par_sort_by_key(|trip_idx| get_departure_time(self, &realtime, *trip_idx, 0));

                // Add slice
                if let Some(trip_idx) = value.first().copied() {
                    let slice = trip_to_shapes_slice[trip_idx as usize];
                    raptor_to_shapes_slice.push(slice);
                } else {
                    raptor_to_shapes_slice.push(None);
                }

                // Add raptor route
                let raptor = RaptorRoute {
                    index: index as u32,
                    route_idx: route.index,
                    // route_id: route.id.clone(),
                    stops: key.into(),
                    trips: value.into(),
                };
                raptor_routes.push(raptor);
            });
        });
        self.raptor_routes = raptor_routes.into();
        let route_to_raptors: Box<[Box<[u32]>]> =
            route_to_raptors.into_iter().map(|val| val.into()).collect();
        self.route_to_raptors = route_to_raptors;
        self.raptor_to_shapes_slice = raptor_to_shapes_slice.into();

        self.stop_to_raptors = stop_to_raptors.into_iter().map(|val| val.into()).collect();
        debug!("Generating raptor routes took {:?}", now.elapsed());
    }

    fn generate_walks(&mut self) {
        debug!("Generating stop to walkable stop mapping...");
        let now = Instant::now();
        let stops: Vec<(u32, Vec<u32>)> = self
            .stops
            .par_iter()
            .map(|sa| {
                let nearby: Vec<u32> = self
                    .stops_by_coordinate(&sa.coordinate, AVERAGE_STOP_DISTANCE)
                    .into_iter()
                    .filter_map(|sb| {
                        if sa.index != sb.index {
                            Some(sb.index)
                        } else {
                            None
                        }
                    })
                    .collect();

                (sa.index, nearby)
            })
            .collect();

        let mut stop_to_walk_stop: Vec<Vec<u32>> = vec![Vec::new(); self.stops.len()];
        stops.into_iter().for_each(|(idx, stops)| {
            stop_to_walk_stop[idx as usize].extend(stops);
        });

        self.stop_to_walk_stop = stop_to_walk_stop
            .into_iter()
            .map(|val| val.into())
            .collect();
        debug!(
            "Generating stop to walkable stop mapping took {:?}",
            now.elapsed()
        );
    }
}
