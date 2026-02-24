mod allocator;
mod discovery;
mod explorer;
mod itinerary;
mod location;
mod path;
mod state;

use std::mem;

pub use allocator::*;
pub(crate) use discovery::*;
pub use itinerary::*;
pub use location::*;
pub(crate) use path::*;
pub(crate) use state::*;

use crate::{
    raptor::explorer::{
        explore_routes, explore_routes_reverse, explore_transfers, explore_transfers_reverse,
    },
    realtime::Realtime,
    repository::Repository,
    shared::time::{self, Time},
};
use thiserror::Error;
use tracing::{trace, warn};

pub const MAX_ROUNDS: usize = 15;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Area id does not match any entry")]
    InvalidAreaID,
    #[error("Stop id does not match any entry")]
    InvalidStopID,
    #[error("A route was found but failed to build it")]
    FailedToBuildRoute,
    #[error("Could not find a route")]
    NoRouteFound,
}

#[derive(Debug, Clone, Copy)]
pub enum TimeConstraint {
    Arrival(Time),
    Departure(Time),
}

impl TimeConstraint {
    pub fn time(&self) -> Time {
        match *self {
            TimeConstraint::Arrival(time) => time,
            TimeConstraint::Departure(time) => time,
        }
    }
}

/// The execution engine for the Round-Based Public Transit Routing (RAPTOR) algorithm.
///
/// This struct holds the search parameters and a reference to the underlying transit
/// [`Repository`]. It is designed to be short-lived, typically created via
/// [`Repository::router`].
///
/// # Search Logic
/// RAPTOR explores the network in "rounds." Round `K` finds all stops reachable
/// with exactly `K` trips. This structure ensures that we only explore the
/// necessary graph edges based on the `departure` time and `walk_distance` constraints.
pub struct Raptor<'a> {
    repository: &'a Repository,
    realtime: &'a Realtime,
    from: Location,
    to: Location,
    time_constraint: TimeConstraint,
    allow_walks: bool,
}

impl<'a> Raptor<'a> {
    /// Creates a new RAPTOR search instance for a specific origin and destination.
    ///
    /// By default, the search uses the current system time for departure and
    /// a standard walking distance. These can be customized using the builder
    /// methods before calling solve.
    ///
    /// # Arguments
    /// * `repository` - A reference to the static transit data.
    /// * `from` - The starting location (Stop, Area, or Coordinate).
    /// * `to` - The target destination.
    pub fn new(
        repository: &'a Repository,
        realtime: &'a Realtime,
        from: Location,
        to: Location,
    ) -> Self {
        Self {
            repository,
            realtime,
            from,
            to,
            time_constraint: TimeConstraint::Departure(Time::now()),
            allow_walks: true,
        }
    }

    /// Sets the earliest time the journey can begin.
    ///
    /// The algorithm will only consider trips that depart at or after this time.
    /// Note that earlier departure times may result in different optimal paths
    /// even for the same origin/destination.
    pub fn departure_at(mut self, departure: Time) -> Self {
        self.time_constraint = TimeConstraint::Departure(departure);
        self
    }

    /// Sets the latest time the journey can arrive.
    ///
    /// The algorithm will only consider trips that arrive at or before this time.
    /// Note that latest arrival times may result in different optimal paths
    /// even for the same origin/destination.
    pub fn arrival_at(mut self, arrival: Time) -> Self {
        self.time_constraint = TimeConstraint::Arrival(arrival);
        self
    }

    pub fn with_time_constraint(mut self, constrait: TimeConstraint) -> Self {
        self.time_constraint = constrait;
        self
    }

    /// If the raptor algorithm is allowed to walk to stops, this can/will improve travel time in most cases.
    pub fn with_allow_walks(mut self, value: bool) -> Self {
        self.allow_walks = value;
        self
    }

    /// Wrapper around slove_with_allocator but creates the allocator internally.
    ///
    /// Executes the multi-criteria search and returns the optimal itinerary.
    ///
    /// This is the most computationally expensive part of the process, involving
    /// parallelized route scanning and transfer calculations.
    ///
    /// # Returns
    /// * `Ok(Itinerary)` - The best path found based on arrival time.
    /// * `Err(Error)` - Returns an error if no path exists or if the search
    ///   parameters are invalid.
    ///
    /// # Performance
    /// This method leverages the parallel optimizations in the underlying [`Repository`].
    /// Execution time typically scales with the number of possible routes between
    /// the origin and destination.
    pub fn solve(self) -> Result<Itinerary, self::Error> {
        let mut allocator = Allocator::new(self.repository);
        self.solve_with_allocator(&mut allocator)
    }

    /// Executes the multi-criteria search and returns the optimal itinerary.
    ///
    /// This is the most computationally expensive part of the process, involving
    /// parallelized route scanning and transfer calculations.
    ///
    /// # Returns
    /// * `Ok(Itinerary)` - The best path found based on arrival time.
    /// * `Err(Error)` - Returns an error if no path exists or if the search
    ///   parameters are invalid.
    ///
    /// # Performance
    /// This method leverages the parallel optimizations in the underlying [`Repository`].
    /// Execution time typically scales with the number of possible routes between
    /// the origin and destination.
    pub fn solve_with_allocator(self, allocator: &mut Allocator) -> Result<Itinerary, self::Error> {
        let from_stops = stops_by_location(self.repository, &self.from)?;
        let to_stops = stops_by_location(self.repository, &self.to)?;

        match self.time_constraint {
            TimeConstraint::Arrival(time) => {
                to_stops.into_iter().for_each(|stop| {
                    allocator.marked_stops.set(stop.index as usize, true);
                    allocator.curr_labels[stop.index as usize] = Some(time);
                });
                allocator.target.stops = from_stops.into_iter().map(|stop| stop.index).collect();
                allocator.target.tau_star = time::MIN;
                allocator.active.fill(u32::MIN);
            }
            TimeConstraint::Departure(time) => {
                from_stops.into_iter().for_each(|stop| {
                    allocator.marked_stops.set(stop.index as usize, true);
                    allocator.curr_labels[stop.index as usize] = Some(time);
                });
                allocator.target.stops = to_stops.into_iter().map(|stop| stop.index).collect();
                allocator.target.tau_star = time::MAX;
                allocator.active.fill(u32::MAX);
            }
        }

        allocator.round = 0;
        loop {
            if allocator.round >= MAX_ROUNDS {
                warn!("Hit round limit!");
                break;
            }
            allocator.swap_labels();

            // Pre process

            if allocator.marked_stops.not_any() {
                break;
            }

            let mut marked_stops = mem::take(&mut allocator.marked_stops);
            trace!(
                "Found {} in round {}",
                marked_stops.iter_ones().count(),
                allocator.round
            );

            // allocator.active.fill(u32::MAX);
            allocator.active_mask.fill(false);
            marked_stops.iter_ones().for_each(|stop_idx| {
                // We look at all the routes that serve a stop
                // for each route that serve a route we store the earliest stop in that route
                // that we serve
                // Example: This is a the stops in a route
                // we have marked 1 3 6 as improvments
                // so we want to make sure that we only exlopre this route once and from the earliest stop
                // in this case it will be 1
                // 0 1 2 3 4 5 6 7 8
                //   ^   ^     ^
                routes_serving_stop(self.repository, stop_idx as u32, allocator);
                for route in allocator.routes_serving_stops.iter() {
                    let r_idx = route.route_idx as usize;
                    let p_idx = route.idx_in_route;
                    match self.time_constraint {
                        TimeConstraint::Departure(_) => {
                            // Forward: Default active to u32::MAX, Keep MIN
                            let p_idx_to_beat = allocator
                                .active_mask
                                .get(r_idx)
                                .map(|_| allocator.active[r_idx])
                                .unwrap_or(u32::MAX);

                            if p_idx < p_idx_to_beat {
                                allocator.active[r_idx] = p_idx;
                                allocator.active_mask.set(r_idx, true);
                            }
                        }
                        TimeConstraint::Arrival(_) => {
                            // Reverse: Default active to 0, Keep MAX
                            let p_idx_to_beat = allocator
                                .active_mask
                                .get(r_idx)
                                .map(|_| allocator.active[r_idx])
                                .unwrap_or(0);

                            if p_idx > p_idx_to_beat {
                                allocator.active[r_idx] = p_idx;
                                allocator.active_mask.set(r_idx, true);
                            }
                        }
                    }
                }
            });

            marked_stops.fill(false);
            allocator.marked_stops = mem::take(&mut marked_stops);

            match self.time_constraint {
                TimeConstraint::Arrival(_) => {
                    explore_routes_reverse(self.repository, self.realtime, allocator);
                    allocator.run_updates_reverse();

                    explore_transfers_reverse(
                        self.allow_walks,
                        self.repository,
                        self.realtime,
                        allocator,
                    );
                    allocator.run_updates_reverse();
                }
                TimeConstraint::Departure(_) => {
                    explore_routes(self.repository, self.realtime, allocator);
                    allocator.run_updates();

                    explore_transfers(self.allow_walks, self.repository, self.realtime, allocator);
                    allocator.run_updates();
                }
            }

            allocator
                .target
                .stops
                .iter()
                .filter_map(|stop_idx| {
                    let tau_star = allocator.tau_star[*stop_idx as usize];
                    tau_star.map(|tau_star| (stop_idx, tau_star))
                })
                .for_each(|(stop_idx, tau_star)| {
                    let improvement = match self.time_constraint {
                        TimeConstraint::Arrival(_) => tau_star > allocator.target.tau_star,
                        TimeConstraint::Departure(_) => tau_star < allocator.target.tau_star,
                    };
                    if improvement {
                        allocator.target.tau_star = tau_star;
                        allocator.target.best_stop = Some(*stop_idx);
                        allocator.target.best_round = Some(allocator.round);
                    }
                });
            allocator.next_round();
        }

        if let Some(target_stop) = allocator.target.best_stop
            && let Some(target_round) = allocator.target.best_round
        {
            let path = backtrack(
                self.repository,
                allocator,
                target_stop,
                target_round,
                self.time_constraint,
            )?;
            Ok(Itinerary::new(self.from, self.to, path, self.repository))
        } else {
            Err(self::Error::NoRouteFound)
        }
    }
}
