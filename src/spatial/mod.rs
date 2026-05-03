use std::collections::HashMap;

use gtfs_bin::{
    consumer::Consumer,
    models::{Coordinate, Opt, Sentinel, Slice, StopIdx},
};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

pub const DEG_OF_LAT: f64 = 111_320.0;
pub const CELL_SIZE_M: f64 = 1000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CellSlice {
    pub start: u32,
    pub count: u32,
}

impl Slice for CellSlice {
    fn range(self) -> std::ops::Range<usize> {
        let start = self.start as usize;
        let end = start + self.count as usize;
        start..end
    }

    fn new(start: u32, count: u32) -> Self {
        Self { start, count }
    }
}

impl Sentinel for CellSlice {
    const NONE: Self = Self {
        start: u32::MAX,
        count: u32::MAX,
    };
}

pub struct SpatialGrid {
    cells: Vec<Opt<CellSlice>>,
    buffer: Vec<(StopIdx, f64, f64)>,
    min_lon: f64,
    min_lat: f64,
    width: i32,
    height: i32,
    center_lat_cos: f64,
}

impl SpatialGrid {
    pub fn new(consumer: &Consumer) -> Self {
        let (min_lat, max_lat, min_lon, max_lon) = consumer
            .stops
            .par_iter()
            .filter_map(|stop| stop.coordinate.get())
            .map(|coord| {
                let lat = coord.lat_f64();
                let lon = coord.lon_f64();

                (lat, lat, lon, lon)
            })
            .reduce(
                || (f64::MAX, f64::MIN, f64::MAX, f64::MIN),
                |a, b| {
                    (
                        a.0.min(b.0), // min_lat
                        a.1.max(b.1), // max_lat
                        a.2.min(b.2), // min_lon
                        a.3.max(b.3), // max_lon
                    )
                },
            );

        let center_lat = (min_lat + max_lat) / 2.0;
        let center_lat_cos = f64::to_radians(center_lat).cos();

        let max_x_meters = (max_lon - min_lon) * DEG_OF_LAT * center_lat_cos;
        let max_y_meters = (max_lat - min_lat) * DEG_OF_LAT;

        let width = (max_x_meters / CELL_SIZE_M).ceil() as i32 + 1;
        let height = (max_y_meters / CELL_SIZE_M).ceil() as i32 + 1;
        let total_cells = (width * height) as usize;

        let mut cell_map: HashMap<usize, Vec<(StopIdx, f64, f64)>> = HashMap::new();

        let valid_stops = consumer
            .stops
            .iter()
            .filter_map(|s| s.coordinate.get().map(|c| (s.idx, c)));

        for (stop, coord) in valid_stops {
            let x = (coord.lon_f64() - min_lon) * DEG_OF_LAT * center_lat_cos;
            let y = (coord.lat_f64() - min_lat) * DEG_OF_LAT;

            let center_gx = (x / CELL_SIZE_M) as i32;
            let center_gy = (y / CELL_SIZE_M) as i32;
            let cell = (center_gy * width + center_gx) as usize;
            cell_map
                .entry(cell)
                .or_default()
                .push((stop, coord.lat_f64(), coord.lon_f64()));
        }

        let mut active_cells: Vec<_> = cell_map.into_iter().collect();
        active_cells.sort_unstable_by(|a, b| a.0.cmp(&b.0));

        let mut cells = vec![Opt::new(CellSlice::NONE); total_cells];
        let mut buffer: Vec<(StopIdx, f64, f64)> = Vec::with_capacity(total_cells);

        for (idx, stops) in active_cells {
            let slice = CellSlice {
                start: buffer.len() as u32,
                count: stops.len() as u32,
            };
            cells[idx] = Opt::new(slice);
            buffer.extend_from_slice(&stops);
        }

        Self {
            cells,
            buffer,
            min_lon,
            min_lat,
            width,
            height,
            center_lat_cos,
        }
    }

    pub fn iter_stops_in_radius(
        &self,
        coord: Coordinate,
        radius: f64,
    ) -> impl Iterator<Item = StopIdx> {
        let x = (coord.lon_f64() - self.min_lon) * DEG_OF_LAT * self.center_lat_cos;
        let y = (coord.lat_f64() - self.min_lat) * DEG_OF_LAT;
        let radius_sq = radius * radius;

        let min_gx = ((x - radius) / CELL_SIZE_M).floor() as i32 - 1;
        let min_gx = min_gx.clamp(0, self.width - 1);
        let max_gx = ((x + radius) / CELL_SIZE_M).floor() as i32 + 1;
        let max_gx = max_gx.clamp(0, self.width - 1);
        let min_gy = ((y - radius) / CELL_SIZE_M).floor() as i32 - 1;
        let min_gy = min_gy.clamp(0, self.height - 1);
        let max_gy = ((y + radius) / CELL_SIZE_M).floor() as i32 + 1;
        let max_gy = max_gy.clamp(0, self.height - 1);

        let min_lon = self.min_lon;
        let min_lat = self.min_lat;
        let center_lat_cos = self.center_lat_cos;

        (min_gy..=max_gy)
            .flat_map(move |gy| {
                (min_gx..=max_gx).filter_map(move |gx| {
                    let cell_idx = (gy * self.width + gx) as usize;
                    self.cells[cell_idx]
                        .get()
                        .map(|slice| &self.buffer[slice.range()])
                })
            })
            .flatten()
            .filter(move |(_, lat, lon)| {
                let dx = x - ((lon - min_lon) * DEG_OF_LAT * center_lat_cos);
                let dy = y - ((lat - min_lat) * DEG_OF_LAT);

                dx * dx + dy * dy <= radius_sq
            })
            .map(|(stop, _, _)| *stop)
    }
}
