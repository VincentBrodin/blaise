use gtfs_bin::{
    consumer::Consumer,
    models::{Distance, Opt, Time},
};

use crate::raptor::{
    LiveTime, Parent,
    query::{self, Location, RaptorQuery},
    state::State,
};

#[derive(Debug, Clone)]
pub struct Leg {
    pub from: Location,
    pub to: Location,
    pub departure_time: LiveTime,
    pub arrival_time: LiveTime,
    pub stops: Vec<LegStop>,
    pub leg_type: LegType,
}

#[derive(Debug, Clone, Copy)]
pub enum LegType {
    Transit,
    Transfer,
    Walk,
    Origin,
}

#[derive(Debug, Clone, Copy)]
pub struct LegStop {
    pub location: Location,
    pub departure_time: LiveTime,
    pub arrival_time: LiveTime,
    pub distance_traveled: Opt<Distance>,
}

#[derive(Debug, Clone)]
pub struct Itinerary {
    pub from: Location,
    pub to: Location,
    pub legs: Vec<Leg>,
}

impl Itinerary {
    pub(crate) fn new(
        query: &RaptorQuery,
        state: &State,
        consumer: &Consumer,
    ) -> Result<Self, crate::Error> {
        if let Some(best_stop) = state.target_best_stop.get()
            && let Some(best_round) = state.target_best_round
        {
            let mut legs: Vec<Leg> = Vec::with_capacity((best_round * 2) + 2);
            let mut current_stop = best_stop;
            let mut current_round = best_round;

            // ==========================================
            // 1. FIRST BOUNDARY (Outer Edge)
            // ==========================================
            let (first_leg_from, first_leg_to) = match query.time_direction {
                query::TimeDirection::Departure(_) => {
                    (Location::Stop(best_stop), query.destination)
                }
                query::TimeDirection::Arrival(_) => (query.origin, Location::Stop(best_stop)),
            };

            if first_leg_from != first_leg_to {
                let stop_time = state.tau_star[best_stop.as_usize()]
                    .get()
                    .unwrap_or(Time(0));
                let target_time = state.target_tau_star.get().unwrap_or(stop_time);

                // For Departure: Stop -> Destination (arr_t is target_tau_star)
                // For Arrival: Origin -> Stop (dep_t is target_tau_star)
                let (dep_t, arr_t) = match query.time_direction {
                    query::TimeDirection::Departure(_) => (stop_time, target_time),
                    query::TimeDirection::Arrival(_) => (target_time, stop_time),
                };

                legs.push(Leg {
                    from: first_leg_from,
                    to: first_leg_to,
                    departure_time: LiveTime::scheduled_only(dep_t),
                    arrival_time: LiveTime::scheduled_only(arr_t),
                    stops: vec![],
                    leg_type: LegType::Walk,
                });
            }

            // ==========================================
            // 2. THE RECONSTRUCTION LOOP
            // ==========================================
            loop {
                // FIX: THE "DUMB START" SHORT-CIRCUIT
                // If the stop we are currently at is reachable directly from the origin,
                // stop here. This prevents unnecessary walks/transfers from being included.
                let origin_parent_idx = state.calc_parent_idx(0, current_stop);
                if let Some(Parent::Origin) = state.parents[origin_parent_idx] {
                    break;
                }

                let parent_idx = state.calc_parent_idx(current_round, current_stop);
                let parent = state.parents[parent_idx].expect("Failed to get parent");

                match parent {
                    Parent::Transit {
                        boarding_p,
                        alighting_p,
                        trip,
                        departure_time,
                        arrival_time,
                    } => {
                        let trip_pattern = consumer.trip_pattern_by_trip(trip);
                        let stop_sequences: Vec<_> = consumer
                            .iter_stop_sequence_by_trip_pattern(trip_pattern.idx)
                            .collect();

                        let boarding_stop = stop_sequences[boarding_p.as_usize()];
                        let alighting_stop = stop_sequences[alighting_p.as_usize()];
                        let stop_times = consumer.stop_times_by_trip(trip);

                        let mut leg_stops = Vec::new();
                        for idx in boarding_p.as_usize()..=alighting_p.as_usize() {
                            let st = stop_times[idx];
                            leg_stops.push(LegStop {
                                location: Location::Stop(stop_sequences[idx].idx),
                                departure_time: LiveTime::scheduled_only(
                                    st.departure_time.get().unwrap_or(Time(0)),
                                ),
                                arrival_time: LiveTime::scheduled_only(
                                    st.arrival_time.get().unwrap_or(Time(0)),
                                ),
                                distance_traveled: st.distance_traveled,
                            });
                        }

                        legs.push(Leg {
                            from: Location::Stop(boarding_stop.idx),
                            to: Location::Stop(alighting_stop.idx),
                            departure_time,
                            arrival_time,
                            stops: leg_stops,
                            leg_type: LegType::Transit,
                        });

                        match query.time_direction {
                            query::TimeDirection::Arrival(_) => current_stop = alighting_stop.idx,
                            query::TimeDirection::Departure(_) => current_stop = boarding_stop.idx,
                        }

                        if current_round > 0 {
                            current_round -= 1;
                        } else {
                            break;
                        }
                    }
                    Parent::Transfer {
                        from_stop,
                        departure_time,
                        arrival_time,
                    }
                    | Parent::Walk {
                        from_stop,
                        departure_time,
                        arrival_time,
                    } => {
                        let (from_loc, to_loc) = match query.time_direction {
                            query::TimeDirection::Arrival(_) => (current_stop, from_stop),
                            query::TimeDirection::Departure(_) => (from_stop, current_stop),
                        };

                        legs.push(Leg {
                            from: Location::Stop(from_loc),
                            to: Location::Stop(to_loc),
                            departure_time,
                            arrival_time,
                            stops: vec![],
                            leg_type: match parent {
                                Parent::Transfer { .. } => LegType::Transfer,
                                _ => LegType::Walk,
                            },
                        });

                        current_stop = from_stop;
                    }
                    Parent::Origin => break,
                }
            }

            // ==========================================
            // 3. SECOND BOUNDARY (Inner Edge)
            // ==========================================
            let (last_leg_from, last_leg_to, last_dep, last_arr) = match query.time_direction {
                query::TimeDirection::Departure(dep_t) => {
                    let arr_t = state.tau_star[current_stop.as_usize()]
                        .get()
                        .unwrap_or(Time(0));
                    (query.origin, Location::Stop(current_stop), dep_t, arr_t)
                }
                query::TimeDirection::Arrival(arr_t) => {
                    let dep_t = state.tau_star[current_stop.as_usize()]
                        .get()
                        .unwrap_or(Time(0));
                    (
                        Location::Stop(current_stop),
                        query.destination,
                        dep_t,
                        arr_t,
                    )
                }
            };

            if last_leg_from != last_leg_to {
                legs.push(Leg {
                    from: last_leg_from,
                    to: last_leg_to,
                    departure_time: LiveTime::scheduled_only(last_dep),
                    arrival_time: LiveTime::scheduled_only(last_arr),
                    stops: vec![],
                    leg_type: LegType::Walk,
                });
            }

            // ==========================================
            // 4. CHRONOLOGICAL REVERSAL
            // ==========================================
            if matches!(query.time_direction, query::TimeDirection::Departure(_)) {
                legs.reverse();
            }

            Ok(Self {
                from: query.origin,
                to: query.destination,
                legs,
            })
        } else {
            Err(crate::Error::NoRoute)
        }
    }
}
