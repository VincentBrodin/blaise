use crate::{
    dto::{AreaDto, StopDto},
    state::AppState,
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use blaise::shared::{AVERAGE_STOP_DISTANCE, Coordinate, Distance};
use std::{collections::HashMap, str::FromStr, sync::Arc};
use tracing::warn;

pub async fn search_areas(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    if let Some(repository) = &*state.repository.read().await {
        if let Some(query) = params.get("q") {
            let count: usize = match params.get("count") {
                Some(value) => match value.parse() {
                    Ok(value) => value,
                    Err(_) => return Err(StatusCode::BAD_REQUEST),
                },
                None => 5,
            };
            let result: Vec<_> = repository
                .search_areas_by_name(query)
                .into_iter()
                .take(count)
                .map(|area| AreaDto::from(area, repository))
                .collect();
            Ok(Json(result).into_response())
        } else {
            Err(StatusCode::BAD_REQUEST)
        }
    } else {
        warn!("Missing repository");
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

pub async fn search_stops(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    if let Some(repository) = &*state.repository.read().await {
        if let Some(query) = params.get("q") {
            let count: usize = match params.get("count") {
                Some(value) => match value.parse() {
                    Ok(value) => value,
                    Err(_) => return Err(StatusCode::BAD_REQUEST),
                },
                None => 5,
            };
            let result: Vec<_> = repository
                .search_stops_by_name(query)
                .into_iter()
                .filter(|stop| repository.stop_idx_has_trips(stop.index))
                .take(count)
                .map(StopDto::from)
                .collect();
            Ok(Json(result).into_response())
        } else {
            Err(StatusCode::BAD_REQUEST)
        }
    } else {
        warn!("Missing repository");
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

pub async fn near_areas(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    if let Some(repository) = &*state.repository.read().await {
        if let Some(query) = params.get("q") {
            let distance: Distance = match params.get("distance") {
                Some(value) => match value.parse::<f32>() {
                    Ok(value) => Distance::from_meters(value),
                    Err(_) => return Err(StatusCode::BAD_REQUEST),
                },
                None => AVERAGE_STOP_DISTANCE,
            };
            let coordinate = Coordinate::from_str(query).map_err(|_| StatusCode::BAD_REQUEST)?;
            let mut result: Vec<_> = repository
                .areas_by_coordinate(&coordinate, distance)
                .into_iter()
                .map(|area| AreaDto::from(area, repository))
                .collect();
            result.sort_by(|a, b| {
                a.coordinate
                    .network_distance(&coordinate)
                    .as_meters()
                    .total_cmp(&b.coordinate.network_distance(&coordinate).as_meters())
            });
            Ok(Json(result).into_response())
        } else {
            Err(StatusCode::BAD_REQUEST)
        }
    } else {
        warn!("Missing repository");
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

pub async fn near_stops(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    if let Some(repository) = &*state.repository.read().await {
        if let Some(query) = params.get("q") {
            let distance: Distance = match params.get("distance") {
                Some(value) => match value.parse::<f32>() {
                    Ok(value) => Distance::from_meters(value),
                    Err(_) => return Err(StatusCode::BAD_REQUEST),
                },
                None => AVERAGE_STOP_DISTANCE,
            };
            let coordinate = Coordinate::from_str(query).map_err(|_| StatusCode::BAD_REQUEST)?;
            let mut result: Vec<_> = repository
                .stops_by_coordinate(&coordinate, distance)
                .into_iter()
                .map(StopDto::from)
                .collect();
            result.sort_by(|a, b| {
                a.coordinate
                    .network_distance(&coordinate)
                    .as_meters()
                    .total_cmp(&b.coordinate.network_distance(&coordinate).as_meters())
            });
            Ok(Json(result).into_response())
        } else {
            Err(StatusCode::BAD_REQUEST)
        }
    } else {
        warn!("Missing repository");
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}
