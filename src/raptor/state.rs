use gtfs_bin::{
    consumer::Consumer,
    models::{Opt, Sentinel, StopIdx, Time},
};

use crate::raptor::{MAX_ROUNDS, Parent, SequnceIdx, Update};

pub struct State {
    pub tau_star: Vec<Opt<Time>>,
    pub current_labels: Vec<Opt<Time>>,
    pub previous_labels: Vec<Opt<Time>>,
    pub marked_stops: Vec<bool>,
    pub active_trip_patterns: Vec<Opt<SequnceIdx>>,

    pub target_tau_star: Opt<Time>,
    pub target_stops: Vec<StopIdx>,
    pub target_best_stop: Opt<StopIdx>,
    pub target_best_round: Option<usize>,

    pub update_buffer: Vec<Update>,
    parents: Vec<Option<Parent>>,

    stop_count: usize,
}

impl State {
    pub fn new(consumer: &Consumer) -> Self {
        Self {
            tau_star: vec![Opt::new(Time::NONE); consumer.stops.len()],
            current_labels: vec![Opt::new(Time::NONE); consumer.stops.len()],
            previous_labels: vec![Opt::new(Time::NONE); consumer.stops.len()],
            marked_stops: vec![false; consumer.stops.len()],
            active_trip_patterns: vec![Opt::new(SequnceIdx::NONE); consumer.trip_patterns.len()],
            target_tau_star: Opt::new(Time::NONE),
            target_stops: Vec::new(),
            target_best_stop: Opt::new(StopIdx::NONE),
            target_best_round: None,
            update_buffer: Vec::with_capacity(512),
            parents: vec![None; consumer.stops.len() * MAX_ROUNDS],

            stop_count: consumer.stops.len(),
        }
    }

    pub fn apply_updates(&mut self, round: usize) {
        let target_tau_star = self.target_tau_star.get().unwrap_or(Time(u32::MAX));

        for update in self.update_buffer.iter() {
            let tau_star = self.tau_star[update.stop.as_usize()]
                .get()
                .unwrap_or(Time(u32::MAX));

            if update.arrival_time < tau_star && update.arrival_time < target_tau_star {
                self.current_labels[update.stop.as_usize()] = Opt::new(update.arrival_time);
                let parent_idx = self.calc_parent_idx(round, update.stop);
                self.parents[parent_idx] = Some(update.parent);
                self.marked_stops[update.stop.as_usize()] = true;
            }
        }
        self.update_buffer.clear();
    }

    #[inline(always)]
    pub(crate) fn calc_parent_idx(&self, round: usize, stop: StopIdx) -> usize {
        (round * self.stop_count) + stop.as_usize()
    }
}
