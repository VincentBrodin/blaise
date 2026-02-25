use crate::{
    raptor::Point,
    shared::{Time, time},
};

#[derive(Debug, Clone)]
pub(crate) struct Update {
    pub stop_idx: u32,
    pub arrival_time: Time,
    pub parent: Parent,
}

impl Update {
    pub fn new(stop_idx: u32, arrival_time: Time, parent: Parent) -> Self {
        Self {
            stop_idx,
            arrival_time,
            parent,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Parent {
    pub from: Point,
    pub to: Point,
    pub parent_type: ParentType,
    pub scheduled_departure_time: Time,
    pub actual_departure_time: Time,
    pub scheduled_arrival_time: Time,
    pub actual_arrival_time: Time,
}

impl Parent {
    pub fn new_transit(
        from: Point,
        to: Point,
        trip: u32,
        scheduled_departure_time: Time,
        actual_departure_time: Time,
        scheduled_arrival_time: Time,
        actual_arrival_time: Time,
    ) -> Self {
        Self {
            from,
            to,
            parent_type: ParentType::Transit(trip),
            scheduled_departure_time,
            actual_departure_time,
            scheduled_arrival_time,
            actual_arrival_time,
        }
    }
    pub fn new_transfer(from: Point, to: Point, departure_time: Time, arrival_time: Time) -> Self {
        Self {
            from,
            to,
            parent_type: ParentType::Transfer,
            scheduled_departure_time: departure_time,
            actual_departure_time: departure_time,
            scheduled_arrival_time: arrival_time,
            actual_arrival_time: arrival_time,
        }
    }
    pub fn new_walk(from: Point, to: Point, departure_time: Time, arrival_time: Time) -> Self {
        Self {
            from,
            to,
            parent_type: ParentType::Walk,
            scheduled_departure_time: departure_time,
            actual_departure_time: departure_time,
            scheduled_arrival_time: arrival_time,
            actual_arrival_time: arrival_time,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ParentType {
    Transit(u32),
    Transfer,
    Walk,
}

impl ParentType {
    pub fn is_transit(&self) -> bool {
        matches!(self, ParentType::Transit(_))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub stops: Vec<u32>,
    pub tau_star: Time,
    pub best_stop: Option<u32>,
    pub best_round: Option<usize>,
}

impl Target {
    pub fn new() -> Self {
        Self {
            stops: vec![],
            tau_star: time::MAX,
            best_stop: None,
            best_round: None,
        }
    }

    pub fn clear(&mut self) {
        self.stops.clear();
        self.tau_star = time::MAX;
        self.best_stop = None;
        self.best_round = None;
    }
}
