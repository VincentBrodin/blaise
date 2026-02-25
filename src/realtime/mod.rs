use gtfs_rt::{FeedEntity, FeedMessage};
use prost::Message;
use tracing::debug;

use crate::repository::Repository;

#[derive(Debug, Clone, Default)]
pub struct Realtime {
    pub trip_updates: Vec<Option<u32>>,
    pub stop_updates: Vec<Option<u32>>,
    pub updates: Vec<FeedEntity>,
}

impl Realtime {
    pub fn new(repository: &Repository) -> Self {
        Self {
            trip_updates: vec![None; repository.trips.len()],
            stop_updates: vec![None; repository.stops.len()],
            updates: Vec::with_capacity(1024),
        }
    }

    pub fn load(
        mut self,
        bytes: &[u8],
        repository: &Repository,
    ) -> Result<Self, prost::DecodeError> {
        let message = FeedMessage::decode(bytes)?;
        println!("Found {} updates", message.entity.len());
        self.trip_updates.fill(None);
        self.stop_updates.fill(None);
        self.updates.clear();
        message
            .entity
            .into_iter()
            .enumerate()
            .for_each(|(i, entity)| {
                if let Some(trip_update) = &entity.trip_update
                    && let Some(trip) = repository.trip_by_id(trip_update.trip.trip_id())
                {
                    self.trip_updates[trip.index as usize] = Some(i as u32);
                }
                if let Some(entity_stop) = &entity.stop
                    && let Some(stop) = repository.stop_by_id(entity_stop.stop_id())
                {
                    self.stop_updates[stop.index as usize] = Some(i as u32);
                }
                self.updates.push(entity);
            });
        Ok(self)
    }
}
