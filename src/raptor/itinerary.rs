use gtfs_bin::{
    consumer::Consumer,
    models::{Delay, Distance, Opt, Time},
    rt::Realtime,
};

use crate::raptor::{
    Parent,
    query::{Location, RaptorQuery},
    state::State,
};

#[derive(Debug, Clone, Copy)]
pub struct LiveTime {
    pub scheduled: Time,
    pub actual: Time,
}

impl LiveTime {
    pub fn delay(&self) -> Delay {
        Delay((self.actual.0 as i64 - self.scheduled.0 as i64) as i16)
    }

    pub fn scheduled_only(time: Time) -> Self {
        Self {
            scheduled: time,
            actual: time,
        }
    }
}

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
        _query: &RaptorQuery,
        state: &State,
        consumer: &Consumer,
        _realtime: &Realtime,
    ) -> Result<Self, crate::Error> {
        if let Some(best_stop) = state.target_best_stop.get()
            && let Some(best_round) = state.target_best_round
        {
            let mut legs: Vec<Leg> = Vec::with_capacity((best_round * 2) + 1);
            let mut current_stop = best_stop;
            let mut current_round = best_round;

            loop {
                println!(
                    "Exploring round {current_round} - {}",
                    current_stop.as_usize()
                );
                let parent_idx = state.calc_parent_idx(current_round, current_stop);
                let parent = state.parents[parent_idx].expect("Failed to get parent");
                match parent {
                    Parent::Transit { boarding_p, trip } => {
                        let trip_pattern = consumer.trip_pattern_by_trip(trip);
                        let stop_sequences: Vec<_> = consumer
                            .iter_stop_sequence_by_trip_pattern(trip_pattern.idx)
                            .collect();

                        let boarding_stop = stop_sequences[boarding_p.as_usize()];
                        let stop_times = consumer.stop_times_by_trip(trip);

                        let departure_time = LiveTime::scheduled_only(
                            stop_times[boarding_p.as_usize()]
                                .departure_time
                                .get()
                                .unwrap(),
                        );

                        let alighting_p_idx = stop_sequences
                            .iter()
                            .enumerate()
                            .position(|(i, s)| s.idx == current_stop && i > boarding_p.as_usize())
                            .unwrap();
                        let arrival_time = LiveTime::scheduled_only(
                            stop_times[alighting_p_idx].arrival_time.get().unwrap(),
                        );

                        let mut leg_stops = Vec::new();
                        for idx in boarding_p.as_usize()..=alighting_p_idx {
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
                            to: Location::Stop(current_stop),
                            departure_time,
                            arrival_time,
                            stops: leg_stops,
                            leg_type: LegType::Transit,
                        });

                        current_stop = boarding_stop.idx;

                        if current_round == 0 {
                            break;
                        }
                        current_round -= 1;
                    }
                    Parent::Transfer { from_stop } => {
                        let arrival_time = state.tau_star[current_stop.as_usize()].get().unwrap();
                        let departure_time = state.tau_star[from_stop.as_usize()].get().unwrap();

                        legs.push(Leg {
                            from: Location::Stop(from_stop),
                            to: Location::Stop(current_stop),
                            departure_time: LiveTime::scheduled_only(departure_time),
                            arrival_time: LiveTime::scheduled_only(arrival_time),
                            stops: vec![],
                            leg_type: LegType::Transfer,
                        });

                        current_stop = from_stop;
                    }
                    Parent::Origin => break,
                }
            }
            legs.reverse();

            let from_loc = legs
                .first()
                .map(|l| l.from)
                .unwrap_or(Location::Stop(best_stop));

            Ok(Self {
                from: from_loc,
                to: Location::Stop(best_stop),
                legs,
            })
        } else {
            Err(crate::Error::NoRoute)
        }
    }
}
