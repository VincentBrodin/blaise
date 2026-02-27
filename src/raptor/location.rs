use crate::{
    repository::{Area, Repository, Stop},
    shared::geo::Coordinate,
};

#[derive(Debug, Clone)]
pub enum Location<'a> {
    Area(&'a str),
    Stop(&'a str),
    Coordinate(Coordinate),
}

impl<'a> Location<'a> {
    pub fn from_area(area: &Area, repository: &'a Repository) -> Self {
        Self::Area(repository.area_str_by_slice(&area.id_slice))
    }
    pub fn from_stop(stop: &Stop, repository: &'a Repository) -> Self {
        Self::Stop(repository.stop_str_by_slice(&stop.id_slice))
    }
    pub fn from_coordinate(coordinate: Coordinate) -> Self {
        Self::Coordinate(coordinate)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Point {
    Coordinate(Coordinate),
    Stop(u32),
}

impl From<u32> for Point {
    fn from(value: u32) -> Self {
        Self::Stop(value)
    }
}

impl From<Coordinate> for Point {
    fn from(value: Coordinate) -> Self {
        Self::Coordinate(value)
    }
}
