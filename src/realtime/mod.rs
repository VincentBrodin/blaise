use std::fmt::Debug;

use gtfs_rt::{FeedEntity, FeedMessage, trip_update::StopTimeEvent};
use prost::Message;
use tracing::warn;

use crate::{repository::Repository, shared::Duration};

#[derive(Debug, Clone, Default)]
pub struct Realtime {
    pub trip_updates: Vec<Option<u32>>,
    pub updates: Vec<FeedEntity>,
    pub stop_time_updates: Vec<StopTimeUpdate>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StopTimeUpdate {
    arrival_delay: Delay,
    departure_delay: Delay,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum Delay {
    #[default]
    OnTime,
    Ahead(Duration),
    Behind(Duration),
}

impl Realtime {
    pub fn new(repository: &Repository) -> Self {
        Self {
            trip_updates: vec![None; repository.trips.len()],
            stop_time_updates: vec![Default::default(); repository.stop_times.len()],
            updates: Vec::with_capacity(1024),
        }
    }

    fn load_inner(
        &mut self,
        bytes: &[u8],
        repository: &Repository,
    ) -> Result<(), prost::DecodeError> {
        let message = FeedMessage::decode(bytes)?;
        println!("Found {} updates", message.entity.len());
        message
            .entity
            .into_iter()
            .enumerate()
            .for_each(|(i, entity)| {
                if let Some(trip_update) = &entity.trip_update
                    && let Some(trip) = repository.trip_by_id(trip_update.trip.trip_id())
                {
                    self.trip_updates[trip.index as usize] = Some(i as u32);
                    let stop_times = repository.stop_times_by_trip_idx(trip.index);
                    trip_update.stop_time_update.iter().for_each(|stu| {
                        if let Some(seq) = stu.stop_sequence
                            && seq != 0
                            && (seq as usize) <= stop_times.len()
                        {
                            let st = &stop_times[seq as usize - 1];
                            let arrival_delay = stop_time_event_to_delay(stu.arrival.as_ref());
                            let departure_delay = stop_time_event_to_delay(stu.departure.as_ref());
                            let update = StopTimeUpdate {
                                arrival_delay,
                                departure_delay,
                            };
                            self.stop_time_updates[st.index as usize] = update;
                        } else {
                            warn!(
                                "Found invalid stop time update in {}",
                                trip_update.trip.trip_id()
                            )
                        }
                    });
                }

                self.updates.push(entity);
            });
        Ok(())
    }

    pub fn load(
        mut self,
        bytes: &[u8],
        repository: &Repository,
    ) -> Result<Self, prost::DecodeError> {
        self.trip_updates.fill(None);
        self.stop_time_updates.fill(Default::default());
        self.updates.clear();
        self.load_inner(bytes, repository)?;
        Ok(self)
    }

    pub fn load_many(
        mut self,
        all_bytes: &[&[u8]],
        repository: &Repository,
    ) -> Result<Self, prost::DecodeError> {
        self.trip_updates.fill(None);
        self.stop_time_updates.fill(Default::default());
        self.updates.clear();
        all_bytes
            .iter()
            .try_for_each(|bytes| self.load_inner(bytes, repository))?;
        Ok(self)
    }
}

fn stop_time_event_to_delay(stop_time_event: Option<&StopTimeEvent>) -> Delay {
    if let Some(stu) = stop_time_event {
        let delay = stu.delay();

        if delay == 0 {
            Delay::OnTime
        } else if delay > 0 {
            Delay::Behind(Duration::from_seconds(delay.unsigned_abs()))
        } else {
            Delay::Ahead(Duration::from_seconds(delay.unsigned_abs()))
        }
    } else {
        Delay::OnTime
    }
}
