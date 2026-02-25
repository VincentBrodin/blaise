use crate::gtfs::{
    GtfsArea, GtfsRoute, GtfsShape, GtfsStop, GtfsStopArea, GtfsStopTime, GtfsTransfer, GtfsTrip,
};

#[derive(Default, Debug)]
pub struct GtfsData {
    pub stops: Vec<GtfsStop>,
    pub areas: Vec<GtfsArea>,
    pub stop_areas: Vec<GtfsStopArea>,
    pub routes: Vec<GtfsRoute>,
    pub trips: Vec<GtfsTrip>,
    pub transfers: Vec<GtfsTransfer>,
    pub stop_times: Vec<GtfsStopTime>,
    pub shapes: Vec<GtfsShape>,
}

impl From<Vec<GtfsTable>> for GtfsData {
    fn from(value: Vec<GtfsTable>) -> Self {
        let mut data = Self::default();
        value.into_iter().for_each(|table| match table {
            GtfsTable::Stops(gtfs_stops) => data.stops = gtfs_stops,
            GtfsTable::Areas(gtfs_areas) => data.areas = gtfs_areas,
            GtfsTable::StopAreas(gtfs_stop_areas) => data.stop_areas = gtfs_stop_areas,
            GtfsTable::Routes(gtfs_routes) => data.routes = gtfs_routes,
            GtfsTable::Trips(gtfs_trips) => data.trips = gtfs_trips,
            GtfsTable::Transfers(gtfs_transfers) => data.transfers = gtfs_transfers,
            GtfsTable::StopTimes(gtfs_stop_times) => data.stop_times = gtfs_stop_times,
            GtfsTable::Shapes(gtfs_shapes) => data.shapes = gtfs_shapes,
            GtfsTable::Unkown => (),
        });
        data
    }
}

#[derive(Debug)]
pub enum GtfsTable {
    Stops(Vec<GtfsStop>),
    Areas(Vec<GtfsArea>),
    StopAreas(Vec<GtfsStopArea>),
    Routes(Vec<GtfsRoute>),
    Trips(Vec<GtfsTrip>),
    Transfers(Vec<GtfsTransfer>),
    StopTimes(Vec<GtfsStopTime>),
    Shapes(Vec<GtfsShape>),
    Unkown,
}
