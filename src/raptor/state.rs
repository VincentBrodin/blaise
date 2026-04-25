use gtfs_bin::{
    consumer::Consumer,
    models::{Duration, Opt, Sentinel, StopIdx},
};

use crate::raptor::{MAX_ROUNDS, Parent, ParetoFront, ParetoLabel, SequnceIdx, Update};

pub struct State {
    /// Global best Pareto front per stop, accumulated across all rounds.
    pub tau_star: Vec<ParetoFront>,
    /// Labels found in the current round.
    pub current_labels: Vec<ParetoFront>,
    /// Labels found in the previous round (used to find new boarding opportunities).
    pub previous_labels: Vec<ParetoFront>,

    pub marked_stops: Vec<bool>,
    pub active_trip_patterns: Vec<Opt<SequnceIdx>>,

    /// Pareto front of (time, cost) labels reaching the target.
    pub target_tau_star: ParetoFront,
    pub target_stops: Vec<(StopIdx, Duration)>,
    /// Stop that led to the minimum-cost label in `target_tau_star`.
    pub target_best_stop: Opt<StopIdx>,
    /// Round that led to the minimum-cost label in `target_tau_star`.
    pub target_best_round: Option<usize>,

    pub parents: Vec<Option<Parent>>,

    stop_count: usize,
}

impl State {
    pub fn new(consumer: &Consumer) -> Self {
        let n = consumer.stops.len();
        Self {
            tau_star: (0..n).map(|_| ParetoFront::new()).collect(),
            current_labels: (0..n).map(|_| ParetoFront::new()).collect(),
            previous_labels: (0..n).map(|_| ParetoFront::new()).collect(),
            marked_stops: vec![false; n],
            active_trip_patterns: vec![Opt::new(SequnceIdx::NONE); consumer.trip_patterns.len()],
            target_tau_star: ParetoFront::new(),
            target_stops: Vec::new(),
            target_best_stop: Opt::new(StopIdx::NONE),
            target_best_round: None,
            parents: vec![None; n * MAX_ROUNDS + 1],
            stop_count: n,
        }
    }

    pub fn reset(&mut self) {
        self.tau_star.iter_mut().for_each(|f| f.clear());
        self.current_labels.iter_mut().for_each(|f| f.clear());
        self.previous_labels.iter_mut().for_each(|f| f.clear());
        self.marked_stops.fill(false);
        self.active_trip_patterns.fill(Opt::new(SequnceIdx::NONE));
        self.target_tau_star.clear();
        self.target_stops.clear();
        self.target_best_stop = Opt::new(StopIdx::NONE);
        self.target_best_round = None;
        self.parents.fill(None);
    }

    pub fn apply_updates(&mut self, round: usize, is_arrival: bool, updates: &[Update]) {
        for update in updates {
            let label = ParetoLabel::new(update.time, update.cost);

            // Prune: skip if dominated by the per-stop tau_star OR the target front.
            if self.tau_star[update.stop.as_usize()].is_dominated(update.time, update.cost, is_arrival) {
                continue;
            }
            if self.target_tau_star.is_dominated(update.time, update.cost, is_arrival) {
                continue;
            }

            // Track the current best cost before adding, so we know whether this label
            // becomes the new minimum-cost label for this stop in this round.
            let prev_best_cost = self.current_labels[update.stop.as_usize()].best_cost();

            let added = self.tau_star[update.stop.as_usize()].add(label, is_arrival);
            if added {
                self.current_labels[update.stop.as_usize()].add(label, is_arrival);

                // Update parent slot if this is the best-cost label seen this round.
                if update.cost <= prev_best_cost {
                    let parent_idx = self.calc_parent_idx(round, update.stop);
                    self.parents[parent_idx] = Some(update.parent);
                }
                self.marked_stops[update.stop.as_usize()] = true;
            }
        }
    }

    #[inline(always)]
    pub(crate) fn calc_parent_idx(&self, round: usize, stop: StopIdx) -> usize {
        (round * self.stop_count) + stop.as_usize()
    }
}
