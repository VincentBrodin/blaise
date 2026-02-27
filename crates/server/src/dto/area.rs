use blaise::{
    repository::{Area, Repository},
    shared::geo::Coordinate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AreaDto {
    pub id: String,
    pub name: String,
    pub coordinate: Coordinate,
}

impl AreaDto {
    pub fn from(area: &Area, repository: &Repository) -> Self {
        let id = area.id.to_string();
        let name = repository.str_by_slice(&area.name_slice).to_string();
        let coordinate: Coordinate = repository
            .stops_by_area_idx(area.index)
            .into_iter()
            .map(|stop| stop.coordinate)
            .sum();
        Self {
            id,
            name,
            coordinate,
        }
    }
}
