use gtfs_bin::{
    consumer::Consumer,
    models::{Distance, Opt, Time, TripIdx},
};

use crate::raptor::{
    LiveTime, Parent, ParetoLabel,
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
    Transit(TripIdx),
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
                query::TimeDirection::Departure(_) => (
                    Location::Stop(best_stop),
                    query.destination.resolve(best_stop),
                ),
                query::TimeDirection::Arrival(_) => {
                    (query.origin.resolve(best_stop), Location::Stop(best_stop))
                }
            };

            if first_leg_from != first_leg_to {
                let stop_time = state.tau_star[best_stop.as_usize()]
                    .iter()
                    .min_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap())
                    .copied()
                    .unwrap_or(ParetoLabel::MIN);
                let target_time = state
                    .target_tau_star
                    .iter()
                    .min_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap())
                    .copied()
                    .unwrap_or(stop_time);

                // For Departure: Stop -> Destination (arr_t is target_tau_star)
                // For Arrival: Origin -> Stop (dep_t is target_tau_star)
                let (dep_t, arr_t) = match query.time_direction {
                    query::TimeDirection::Departure(_) => (stop_time, target_time),
                    query::TimeDirection::Arrival(_) => (target_time, stop_time),
                };

                legs.push(Leg {
                    from: first_leg_from,
                    to: first_leg_to,
                    departure_time: LiveTime::scheduled_only(dep_t.time),
                    arrival_time: LiveTime::scheduled_only(arr_t.time),
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
                        let stop_sequences =
                            consumer.stop_sequence_by_trip_pattern(trip_pattern.idx);

                        let boarding_stop = stop_sequences[boarding_p.as_usize()];
                        let alighting_stop = stop_sequences[alighting_p.as_usize()];
                        let num_stops = alighting_stop
                            .as_usize()
                            .saturating_sub(boarding_stop.as_usize())
                            + 1;
                        let stop_times = consumer.stop_times_by_trip(trip);

                        let mut leg_stops = Vec::with_capacity(num_stops);
                        for idx in boarding_p.as_usize()..=alighting_p.as_usize() {
                            let st = stop_times[idx];
                            leg_stops.push(LegStop {
                                location: Location::Stop(stop_sequences[idx]),
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
                            from: Location::Stop(boarding_stop),
                            to: Location::Stop(alighting_stop),
                            departure_time,
                            arrival_time,
                            stops: leg_stops,
                            leg_type: LegType::Transit(trip),
                        });

                        match query.time_direction {
                            query::TimeDirection::Arrival(_) => current_stop = alighting_stop,
                            query::TimeDirection::Departure(_) => current_stop = boarding_stop,
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
                        .iter()
                        .min_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap())
                        .copied()
                        .unwrap_or(ParetoLabel::MIN);
                    (
                        query.origin.resolve(current_stop),
                        Location::Stop(current_stop),
                        dep_t,
                        arr_t.time,
                    )
                }
                query::TimeDirection::Arrival(arr_t) => {
                    let dep_t = state.tau_star[current_stop.as_usize()]
                        .iter()
                        .min_by(|a, b| a.cost.partial_cmp(&b.cost).unwrap())
                        .copied()
                        .unwrap_or(ParetoLabel::MIN);
                    (
                        Location::Stop(current_stop),
                        query.destination.resolve(current_stop),
                        dep_t.time,
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

            // ==========================================
            // 5. MERGE CONSECUTIVE WALKS (IN-PLACE)
            // ==========================================
            if !legs.is_empty() {
                let mut write_idx = 0;

                for read_idx in 1..legs.len() {
                    let is_mergeable = matches!(legs[write_idx].leg_type, LegType::Walk)
                        && matches!(legs[read_idx].leg_type, LegType::Walk);

                    if is_mergeable {
                        legs[write_idx].to = legs[read_idx].to.clone();
                        legs[write_idx].arrival_time = legs[read_idx].arrival_time;
                    } else {
                        write_idx += 1;
                        legs.swap(write_idx, read_idx);
                    }
                }
                legs.truncate(write_idx + 1);
            }

            let overall_from = match query.time_direction {
                query::TimeDirection::Departure(_) => query.origin.resolve(current_stop),
                query::TimeDirection::Arrival(_) => query.origin.resolve(best_stop),
            };

            let overall_to = match query.time_direction {
                query::TimeDirection::Departure(_) => query.destination.resolve(best_stop),
                query::TimeDirection::Arrival(_) => query.destination.resolve(current_stop),
            };

            Ok(Self {
                from: overall_from,
                to: overall_to,
                legs,
            })
        } else {
            Err(crate::Error::NoRoute)
        }
    }
}
