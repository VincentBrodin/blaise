use std::{collections::HashMap, f64::consts::PI, ops::Range};

use gtfs_bin::{
    consumer::Consumer,
    models::{Coordinate, StopIdx},
};
use rayon::{
    iter::{IntoParallelRefIterator, ParallelIterator},
    slice::ParallelSliceMut,
};

const EARTH_RADIUS_M: f64 = 6_378_137.0;
const CELL_SIZE: f64 = 500.0;

struct CellSlice {
    pub start: u32,
    pub count: u32,
}

impl CellSlice {
    #[inline]
    fn to_range(&self) -> Range<usize> {
        let start = self.start as usize;
        let end = start + self.count as usize;
        start..end
    }
}

pub struct SpatialHash {
    map: HashMap<u64, CellSlice>,
    buffer: Vec<StopIdx>,
}

impl SpatialHash {
    pub fn new(consumer: &Consumer) -> Self {
        let mut entries: Vec<_> = consumer
            .stops
            .par_iter()
            .filter_map(|stop| {
                stop.coordinate.get().map(|coord| {
                    let key = Self::lat_lon_to_key(coord);
                    (key, stop.idx)
                })
            })
            .collect();

        entries.par_sort_by_key(|(key, _)| *key);

        let mut map: HashMap<u64, CellSlice> = HashMap::new();
        let mut buffer: Vec<StopIdx> = Vec::with_capacity(entries.len());
        let mut last_key: Option<u64> = None;
        let mut last_start: usize = 0;
        for (key, stop_idx) in entries.into_iter() {
            if let Some(lk) = last_key
                && lk != key
            {
                let slice = CellSlice {
                    start: last_start as u32,
                    count: (buffer.len() - last_start) as u32,
                };
                last_start = buffer.len();
                map.insert(lk, slice);
                last_key = Some(key)
            } else {
                last_key = Some(key)
            }

            buffer.push(stop_idx);
        }

        if let Some(lk) = last_key
            && last_start != buffer.len()
        {
            let slice = CellSlice {
                start: last_start as u32,
                count: (buffer.len() - last_start) as u32,
            };
            map.insert(lk, slice);
        }

        Self { map, buffer }
    }

    pub fn get_in_radius_iter<'a>(
        &'a self,
        coord: Coordinate,
        radius_m: f64,
    ) -> impl Iterator<Item = StopIdx> + 'a {
        const CELL_SIZE: f64 = 500.0;

        let (center_x, center_y) = Self::lat_lon_to_grid(coord);
        let cell_radius = (radius_m / CELL_SIZE).ceil() as i32;

        (-cell_radius..=cell_radius)
            .flat_map(move |dy| {
                (-cell_radius..=cell_radius).map(move |dx| {
                    let grid_x = center_x + dx;
                    let grid_y = center_y + dy;
                    Self::pack_key(grid_x, grid_y)
                })
            })
            .filter_map(move |key| self.map.get(&key))
            .flat_map(move |slice| self.buffer[slice.to_range()].iter().copied())
    }

    #[inline]
    fn lat_lon_to_grid(coord: Coordinate) -> (i32, i32) {
        let lat_rad = coord.lat_f64().to_radians();
        let lon_rad = coord.lon_f64().to_radians();

        let x_meters = EARTH_RADIUS_M * lon_rad;
        let y_meters = EARTH_RADIUS_M * ((PI / 4.0) + (lat_rad / 2.0)).tan().ln();

        let grid_x = (x_meters / CELL_SIZE).floor() as i32;
        let grid_y = (y_meters / CELL_SIZE).floor() as i32;

        (grid_x, grid_y)
    }

    #[inline]
    fn pack_key(x: i32, y: i32) -> u64 {
        ((x as u64) << 32) | (y as u64)
    }

    #[inline]
    fn lat_lon_to_key(coord: Coordinate) -> u64 {
        let (x, y) = Self::lat_lon_to_grid(coord);
        Self::pack_key(x, y)
    }
}
