use crate::state::{AllocatorPool, AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use blaise::{gtfs::GtfsReader, repository::Repository};
use futures_util::StreamExt;
use reqwest::header::ACCEPT_ENCODING;
use std::{collections::HashMap, fs, path::Path, sync::Arc, time::Instant};
use tokio::{fs::File, io::AsyncWriteExt};
use tracing::{error, info};

pub async fn age(
    Query(_): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    if state.gtfs_data_path.exists() {
        let last_modifed = seconds_since_modified(&state.gtfs_data_path)?;
        Ok(last_modifed.to_string().into_response())
    } else {
        Err(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

fn seconds_since_modified<P: AsRef<Path>>(path: P) -> Result<u64, StatusCode> {
    let meta_data = fs::metadata(path).map_err(|err| {
        error!("Failed to get metadata: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let modified = meta_data.modified().map_err(|err| {
        error!("Failed to get modified: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let duration = modified.elapsed().map_err(|err| {
        error!("Failed to elapsed time since modified: {err}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(duration.as_secs())
}

pub async fn fetch_url(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    if let Some(q) = params.get("q") {
        info!("Downloading GTFS data...");
        let mut now = Instant::now();
        let response = reqwest::Client::new()
            .get(q)
            .header(ACCEPT_ENCODING, "gzip, deflate")
            .send()
            .await
            .map_err(|err| {
                error!("Failed to fetch: {err}");
                StatusCode::BAD_REQUEST
            })?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            error!("Response is not success: {body}");
            return Err(StatusCode::BAD_REQUEST);
        }

        let mut file = File::create(&state.gtfs_data_path).await.map_err(|err| {
            error!("Failed to create file: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let data = chunk.map_err(|err| {
                error!("Failed to fetch chunk: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            file.write_all(&data).await.map_err(|err| {
                error!("Failed to write to file: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        }

        file.flush().await.map_err(|err| {
            error!("Failed to flush file: {err}");
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        info!("Downloading GTFS data took {:?}", now.elapsed());

        info!("Reading GTFS data...");
        now = Instant::now();
        let data = GtfsReader::new()
            .from_zip(&state.gtfs_data_path)
            .map_err(|err| {
                error!("Failed create gtfs reader from zip: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?
            .par_read()
            .map_err(|err| {
                error!("Failed read gtfs data from gtfs reader: {err}");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        info!("Reading GTFS data took {:?}", now.elapsed());
        info!("Loading GTFS data...");
        now = Instant::now();
        let repo = Repository::new().load_gtfs(data);
        info!("Loading GTFS data took {:?}", now.elapsed());
        info!("Allocating {} pools...", state.allocator_count);
        now = Instant::now();
        let pool = AllocatorPool::new(state.allocator_count, &repo);
        info!(
            "Allocating {} pools took {:?}",
            state.allocator_count,
            now.elapsed()
        );
        let _ = state.allocator_pool.write().await.replace(pool);
        let _ = state.repository.write().await.replace(repo);
        Ok(().into_response())
    } else {
        Err(StatusCode::BAD_REQUEST)
    }
}
