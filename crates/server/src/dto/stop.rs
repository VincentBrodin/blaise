use blaise::{
    repository::{Repository, Stop},
    shared::geo::Coordinate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopDto {
    pub id: String,
    pub name: String,
    pub coordinate: Coordinate,
}

impl StopDto {
    pub fn from(stop: &Stop, repository: &Repository) -> Self {
        let id = repository.stop_str_by_slice(&stop.id_slice).to_string();
        let name = repository.stop_str_by_slice(&stop.name_slice).to_string();
        let coordinate = stop.coordinate;
        Self {
            id,
            name,
            coordinate,
        }
    }
}
