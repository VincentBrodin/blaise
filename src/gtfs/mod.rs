use memmap2::MmapOptions;
use rayon::{iter::ParallelIterator, slice::ParallelSlice};
use serde::de::DeserializeOwned;
use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
    time::Instant,
};
use thiserror::Error;
use tracing::{debug, info};

mod config;
mod data;
mod models;

pub use config::*;
pub use data::*;
pub use models::*;

// 10MB
const MAX_PAR_FILE_READ: u64 = 10 * 1024 * 1024;

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Csv error: {0}")]
    Csv(#[from] csv::Error),
    #[error("File did not match the expected format")]
    InvalidFile,
    #[error("Could not find file with name: {0}")]
    FileNotFound(String),
    #[error("Missing any source to pull data from")]
    MissingSource,
}

#[derive(Default, Debug)]
pub struct GtfsReader {
    config: Config,
    dir_path: PathBuf,
}

impl GtfsReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    pub fn from_zip<P: AsRef<Path>>(mut self, path: P) -> Result<Self, self::Error> {
        let dir_path = GtfsReader::create_cache_dir(path)?;
        self.dir_path = dir_path;
        Ok(self)
    }

    pub fn from_zip_cache<P: AsRef<Path>>(mut self, path: P) -> Result<Self, self::Error> {
        let dir_path = GtfsReader::get_or_create_cache_dir(&path)?;
        self.dir_path = dir_path;
        Ok(self)
    }

    pub fn from_directory<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.dir_path = path.as_ref().to_path_buf();
        self
    }

    fn get_or_create_cache_dir<P: AsRef<Path>>(zip_path: P) -> Result<PathBuf, self::Error> {
        let zip_path = zip_path.as_ref();

        let mut target_dir = PathBuf::from(zip_path);
        target_dir.set_extension("");

        if !target_dir.exists() {
            target_dir = GtfsReader::create_cache_dir(zip_path)?;
        } else {
            info!("Using existing GTFS cache at {:?}", target_dir);
        }

        Ok(target_dir)
    }

    fn create_cache_dir<P: AsRef<Path>>(zip_path: P) -> Result<PathBuf, self::Error> {
        let zip_path = zip_path.as_ref();
        let mut target_dir = PathBuf::from(zip_path);
        target_dir.set_extension("");
        info!("Extracting GTFS to {:?}...", target_dir);
        fs::create_dir_all(&target_dir)?;
        let file = fs::File::open(zip_path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        archive.extract(&target_dir)?;
        Ok(target_dir)
    }

    /// Reads all the gtfs data into a `GtfsData` struct in parallel
    pub fn par_read(&self) -> Result<GtfsData, self::Error> {
        const TABLES_COUNT: usize = 8;
        let mut tables: Vec<GtfsTable> = Vec::with_capacity(TABLES_COUNT);

        let table: Vec<GtfsShape> = self.read_table(&self.config.shapes_path)?;
        tables.push(GtfsTable::Shapes(table));

        let table: Vec<GtfsStopTime> = self.read_table(&self.config.stop_times_path)?;
        tables.push(GtfsTable::StopTimes(table));

        let table: Vec<GtfsArea> = self.read_table(&self.config.areas_path)?;
        tables.push(GtfsTable::Areas(table));

        let table: Vec<GtfsStop> = self.read_table(&self.config.stops_path)?;
        tables.push(GtfsTable::Stops(table));

        let table: Vec<GtfsStopArea> = self.read_table(&self.config.stop_areas_path)?;
        tables.push(GtfsTable::StopAreas(table));

        let table: Vec<GtfsRoute> = self.read_table(&self.config.routes_path)?;
        tables.push(GtfsTable::Routes(table));

        let table: Vec<GtfsTrip> = self.read_table(&self.config.trips_path)?;
        tables.push(GtfsTable::Trips(table));

        let table: Vec<GtfsTransfer> = self.read_table(&self.config.transfers_path)?;
        tables.push(GtfsTable::Transfers(table));

        Ok(GtfsData::from(tables))
    }

    fn read_table<T>(&self, file_name: &str) -> Result<Vec<T>, self::Error>
    where
        T: DeserializeOwned + Send + Sync,
    {
        let path = self.dir_path.join(file_name);
        if !path.exists() {
            return Ok(vec![]);
        }

        let metadata = std::fs::metadata(&path)?;

        if metadata.len() < MAX_PAR_FILE_READ {
            debug!("Parsing {file_name} in seq...");
            let now = Instant::now();
            let data = parse_file(&path);
            debug!("Parsing {file_name} in seq took {:?}", now.elapsed());
            data
        } else {
            debug!("Parsing {file_name} in par...");
            let now = Instant::now();
            let data = par_parse_file(&path);
            debug!("Parsing {file_name} in par took {:?}", now.elapsed());
            data
        }
    }
}

fn parse_file<P, T>(path: P) -> Result<Vec<T>, self::Error>
where
    P: AsRef<Path>,
    T: DeserializeOwned,
{
    let mut reader = csv::Reader::from_path(path)?;
    Ok(reader
        .deserialize::<T>()
        .collect::<Result<Vec<T>, csv::Error>>()?)
}

fn par_parse_file<P, T>(path: P) -> Result<Vec<T>, self::Error>
where
    P: AsRef<Path>,
    T: DeserializeOwned + Sync + Send,
{
    let file = File::open(path)?;
    let mmap = unsafe { MmapOptions::new().map(&file)? };

    let len = mmap.len();
    let num_threads = rayon::current_num_threads();
    let chunk_size = len / num_threads;

    let header_len = mmap
        .iter()
        .position(|&b| b == b'\n')
        .ok_or(self::Error::InvalidFile)?
        + 1;
    let headers = &mmap[0..header_len];
    let mut offsets = Vec::with_capacity(num_threads + 1);
    offsets.push(header_len);

    for i in 1..num_threads {
        let mut target = i * chunk_size;
        while target < len && mmap[target] != b'\n' {
            target += 1;
        }
        offsets.push(target + 1);
    }
    offsets.push(len);

    let results: Result<Vec<Vec<T>>, csv::Error> = offsets
        .par_windows(2)
        .map(|window| {
            let start = window[0];
            let end = window[1];
            let slice = &mmap[start..end];

            let chain = headers.chain(slice);
            let mut csv = csv::ReaderBuilder::new()
                .has_headers(true)
                .from_reader(chain);

            csv.deserialize().collect()
        })
        .collect();

    Ok(results?.into_iter().flatten().collect())
}
