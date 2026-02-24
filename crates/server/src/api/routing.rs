use crate::{dto::ItineraryDto, state::AppState};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use blaise::{
    raptor::{LegType, Location, Raptor, TimeConstraint},
    repository::Repository,
    shared::{Coordinate, Time},
};
use std::{
    collections::HashMap,
    str::{self, FromStr},
    sync::Arc,
};
use tracing::debug;

pub async fn routing(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    let gaurd = state.repository.read().await;
    let repository = gaurd.as_ref().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let gaurd = state.realtime.read().await;
    let realtime = gaurd.as_ref().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let gaurd = state.allocator_pool.read().await;
    let pool = gaurd.as_ref().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let from = if let Some(from) = params.get("from") {
        location_from_str(repository, from)?
    } else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let to = if let Some(to) = params.get("to") {
        location_from_str(repository, to)?
    } else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let departure_at = params
        .get("departure_at")
        .map(|departure_at| Time::from_hms(departure_at).ok_or(StatusCode::BAD_REQUEST));

    let arrive_at = params
        .get("arrive_at")
        .map(|arrive_at| Time::from_hms(arrive_at).ok_or(StatusCode::BAD_REQUEST));

    let allow_walks = params
        .get("allow_walk")
        .map(|shapes| bool::from_str(shapes).map_err(|_| StatusCode::BAD_REQUEST))
        .unwrap_or(Ok(true))?;

    let include_shapes = params
        .get("shapes")
        .map(|shapes| bool::from_str(shapes).map_err(|_| StatusCode::BAD_REQUEST))
        .unwrap_or(Ok(false))?;

    let time_constrait = if let Some(arrive_at) = arrive_at {
        TimeConstraint::Arrival(arrive_at?)
    } else if let Some(departure_at) = departure_at {
        TimeConstraint::Departure(departure_at?)
    } else {
        TimeConstraint::Departure(Time::now())
    };

    let mut gaurd = pool.get_safe(repository);
    let allocator = gaurd.allocator.as_mut().expect("This should never fail");
    debug!(
        "Looking for a route from {:?} to {:?} | time constraint: {:?} | allowing walks: {} | sending shapes: {}",
        from, to, time_constrait, allow_walks, include_shapes
    );

    let raptor = Raptor::new(repository, realtime, from, to)
        .with_time_constraint(time_constrait)
        .with_allow_walks(allow_walks);
    let itinerary = raptor
        .solve_with_allocator(allocator)
        .expect("Failed to unwrap allocator");
    itinerary.legs.iter().for_each(|leg| {
        let leg_type = leg_type_str(&leg.leg_type, repository);
        if let Location::Stop(from_stop) = &leg.from
            && let Location::Stop(to_stop) = &leg.to
        {
            let from = repository.stop_by_id(from_stop).unwrap();
            let to = repository.stop_by_id(to_stop).unwrap();
            let mut delay = String::from("no delay");
            if let LegType::Transit(trip_idx) = leg.leg_type
                && let Some(update_idx) = realtime.trip_updates[trip_idx as usize]
            {
                let update = &realtime.updates[update_idx as usize];
                if let Some(trip_update) = &update.trip_update {
                    delay = format!("{} delay", trip_update.delay())
                }
            }
            debug!(
                "{leg_type} {} -> {} @ {} -> {} | {}",
                from.name,
                to.name,
                leg.departue_time.to_hms_string(),
                leg.arrival_time.to_hms_string(),
                delay
            );
            leg.stops.iter().for_each(|leg_stop| {
                if let Location::Stop(stop_id) = &leg_stop.location {
                    let stop = repository.stop_by_id(stop_id).unwrap();
                    debug!(
                        "| {} @ {} -> {}",
                        stop.name,
                        leg_stop.arrival_time.to_hms_string(),
                        leg_stop.departure_time.to_hms_string(),
                    );
                }
            });
        } else if let Location::Coordinate(from_coord) = &leg.from
            && let Location::Stop(to_stop) = &leg.to
        {
            let to = repository.stop_by_id(to_stop).unwrap();
            debug!(
                "{leg_type} {} -> {} @ {} -> {}",
                from_coord,
                to.name,
                leg.departue_time.to_hms_string(),
                leg.arrival_time.to_hms_string(),
            );
        } else if let Location::Stop(from_stop) = &leg.from
            && let Location::Coordinate(to_coord) = &leg.to
        {
            let from = repository.stop_by_id(from_stop).unwrap();
            debug!(
                "{leg_type} {} -> {} @ {} -> {}",
                from.name,
                to_coord,
                leg.departue_time.to_hms_string(),
                leg.arrival_time.to_hms_string()
            );
        }
    });
    let mut dto =
        ItineraryDto::from(itinerary, repository).ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    if !include_shapes {
        dto.legs.iter_mut().for_each(|leg| {
            leg.shapes = None;
        });
    }
    Ok(Json(dto).into_response())
}

fn location_from_str(repository: &Repository, str: &str) -> Result<Location, StatusCode> {
    if str.contains(',') {
        let coordinate = Coordinate::from_str(str).map_err(|_| StatusCode::BAD_REQUEST)?;
        Ok(coordinate.into())
    } else if let Some(area) = repository.area_by_id(str) {
        Ok(area.into())
    } else if let Some(stop) = repository.stop_by_id(str) {
        Ok(stop.into())
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}

fn leg_type_str(parent_type: &LegType, repository: &Repository) -> String {
    match parent_type {
        LegType::Transit(trip_idx) => {
            let trip = &repository.trips[*trip_idx as usize];
            let route = &repository.routes[trip.route_idx as usize];
            let long_name = &route.long_name.clone().unwrap_or("UNKOWN".into());
            let short_name = &route.short_name.clone().unwrap_or("UNKOWN".into());
            format!("Travel with {}({})", long_name, short_name)
        }
        LegType::Transfer => "Transfer".into(),
        LegType::Walk => "Walk".into(),
    }
}
