use gtfs_bin::{
    consumer::Consumer,
    models::{Duration, Opt, Sentinel, StopIdx},
};

use crate::raptor::{MAX_ROUNDS, Parent, ParetoLabel, SequnceIdx, Update};

pub struct State {
    pub tau_star: Vec<Option<ParetoLabel>>,
    pub current_labels: Vec<Option<ParetoLabel>>,
    pub previous_labels: Vec<Option<ParetoLabel>>,
    pub marked_stops: Vec<bool>,
    pub active_trip_patterns: Vec<Opt<SequnceIdx>>,

    pub target_tau_star: Option<ParetoLabel>,
    pub target_stops: Vec<(StopIdx, Duration)>,
    pub target_best_stop: Opt<StopIdx>,
    pub target_best_round: Option<usize>,

    pub parents: Vec<Option<Parent>>,

    stop_count: usize,
}

impl State {
    pub fn new(consumer: &Consumer) -> Self {
        Self {
            tau_star: vec![None; consumer.stops.len()],
            current_labels: vec![None; consumer.stops.len()],
            previous_labels: vec![None; consumer.stops.len()],
            marked_stops: vec![false; consumer.stops.len()],
            active_trip_patterns: vec![Opt::new(SequnceIdx::NONE); consumer.trip_patterns.len()],
            target_tau_star: None,
            target_stops: Vec::new(),
            target_best_stop: Opt::new(StopIdx::NONE),
            target_best_round: None,
            parents: vec![None; consumer.stops.len() * MAX_ROUNDS + 1],

            stop_count: consumer.stops.len(),
        }
    }

    pub fn reset(&mut self) {
        self.tau_star.fill(None);
        self.current_labels.fill(None);
        self.previous_labels.fill(None);
        self.marked_stops.fill(false);
        self.active_trip_patterns.fill(Opt::new(SequnceIdx::NONE));
        self.target_tau_star = None;
        self.target_stops.clear();
        self.target_best_stop = Opt::new(StopIdx::NONE);
        self.target_best_round = None;
        self.parents.fill(None);
    }

    pub fn apply_updates(&mut self, round: usize, updates: &[Update]) {
        for update in updates {
            let tau_star = self.tau_star[update.stop.as_usize()].unwrap_or(ParetoLabel::MAX);
            let target_tau_star = self.target_tau_star.unwrap_or(ParetoLabel::MAX);

            // Primary criterion: cost must strictly improve on both the stop's
            // best-known label and the overall target best.
            let improved = update.cost < tau_star.cost && update.cost < target_tau_star.cost;

            if improved {
                self.current_labels[update.stop.as_usize()] =
                    Some(ParetoLabel::new(update.time, update.cost));
                self.tau_star[update.stop.as_usize()] =
                    Some(ParetoLabel::new(update.time, update.cost));
                let parent_idx = self.calc_parent_idx(round, update.stop);
                self.parents[parent_idx] = Some(update.parent);
                self.marked_stops[update.stop.as_usize()] = true;
            }
        }
    }

    #[inline(always)]
    pub(crate) fn calc_parent_idx(&self, round: usize, stop: StopIdx) -> usize {
        (round * self.stop_count) + stop.as_usize()
    }
}
