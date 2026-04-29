use dialoguer::{FuzzySelect, theme::ColorfulTheme};
use std::{env, fs::File, time::Instant};

use blaise::{
    raptor::{
        itinerary::{Itinerary, LegType},
        query::{Location, QueryLocation, RaptorQuery},
        solve,
        state::State,
    },
    spatial::SpatialHash,
};
use gtfs_bin::{consumer::Consumer, models::Time};
use memmap2::MmapOptions;

pub fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        panic!("Missing args");
    }

    let path = args[1].as_str();
    let file = File::open(path).expect("Failed to open file");
    let mmap = unsafe { MmapOptions::new().map(&file).expect("Failed to map memory") };

    let consumer = Consumer::new(&mmap).expect("Failed to parse files header");
    let spatial_hash = SpatialHash::new(&consumer);

    let stop_names: Vec<&str> = consumer
        .search_stops
        .iter()
        .map(|search| consumer.string(search.name))
        .collect();

    let from = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("From")
        .default(0)
        .items(stop_names.iter())
        .interact()
        .unwrap();

    let to = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt("To")
        .default(0)
        .items(stop_names.iter())
        .interact()
        .unwrap();

    let query = RaptorQuery::new(
        QueryLocation::Stops(
            consumer
                .iter_stops_by_search(from.into())
                .map(|stop| stop.idx)
                .collect(),
        ),
        QueryLocation::Stops(
            consumer
                .iter_stops_by_search(to.into())
                .map(|stop| stop.idx)
                .collect(),
        ),
    )
    .with_departure(Time::from_hms("08:00:00").unwrap());

    let mut state = State::new(&consumer);
    let now = Instant::now();
    let itinerary =
        solve(query, &consumer, &spatial_hash, &mut state).expect("Failed to find route");
    println!("Solvig route query took {:?}", now.elapsed());

    print_itinerary(&itinerary, &consumer);
}

fn format_location(loc: &Location, consumer: &Consumer) -> String {
    match loc {
        Location::Stop(stop_idx) => {
            let stop = consumer.stop(*stop_idx);
            if let Some(name) = stop.name.get() {
                consumer.string(name).to_string()
            } else {
                consumer.stop_id(stop.id).to_string()
            }
        }
        Location::Coordinate(coord) => {
            format!("Location ({}, {})", coord.lat_f64(), coord.lon_f64())
        }
    }
}

pub fn print_itinerary(itinerary: &Itinerary, consumer: &Consumer) {
    // Clear screen for a fresh view
    print!("{esc}c", esc = 27 as char);

    println!("\n=========================================================");
    println!("🗺️  \x1b[1mJOURNEY ITINERARY\x1b[0m");
    println!("=========================================================\n");

    if itinerary.legs.is_empty() {
        println!("No travel needed. You are already at your destination!");
        return;
    }

    let start_time = itinerary.legs.first().unwrap().departure_time.scheduled;
    let end_time = itinerary.legs.last().unwrap().arrival_time.scheduled;
    let origin_name = format_location(&itinerary.from, consumer);
    let dest_name = format_location(&itinerary.to, consumer);

    println!("📍 \x1b[1mFrom:\x1b[0m  {}", origin_name);
    println!("📍 \x1b[1mTo:\x1b[0m    {}", dest_name);
    println!("⏱️ \x1b[1mTime:\x1b[0m  {} -> {}", start_time, end_time);
    println!("---------------------------------------------------------\n");

    for leg in itinerary.legs.iter() {
        let dep_time = leg.departure_time.scheduled;
        let arr_time = leg.arrival_time.scheduled;
        let from_name = format_location(&leg.from, consumer);
        let to_name = format_location(&leg.to, consumer);

        match leg.leg_type {
            LegType::Transit => {
                println!("🚌 \x1b[1;34mTRANSIT\x1b[0m");

                if leg.stops.is_empty() {
                    println!("  {} ┌ Board at \x1b[1m{}\x1b[0m", dep_time, from_name);
                    println!("         │");
                    println!("  {} └ Alight at \x1b[1m{}\x1b[0m\n", arr_time, to_name);
                    continue;
                }

                for (i, stop) in leg.stops.iter().enumerate() {
                    let name = format_location(&stop.location, consumer);
                    if i == 0 {
                        let dep = stop.departure_time.scheduled;
                        println!("  {} ┌ Board at \x1b[1m{}\x1b[0m", dep, name);
                    } else if i == leg.stops.len() - 1 {
                        let arr = stop.arrival_time.scheduled;
                        println!("  {} └ Alight at \x1b[1m{}\x1b[0m\n", arr, name);
                    } else {
                        let arr = stop.arrival_time.scheduled;
                        println!("  {} ├  {}", arr, name);
                    }
                }
            }
            LegType::Transfer | LegType::Walk | LegType::Origin => {
                let (icon, title) = match leg.leg_type {
                    LegType::Transfer => ("🔄", "\x1b[1;33mTRANSFER\x1b[0m"),
                    LegType::Walk => ("🚶", "\x1b[1;32mWALK\x1b[0m"),
                    LegType::Origin => ("🚶", "\x1b[1;32mSTARTING WALK\x1b[0m"),
                    _ => unreachable!(),
                };

                println!("{} {}", icon, title);
                println!("  {} ┌ Leave \x1b[1m{}\x1b[0m", dep_time, from_name);
                println!("  {} └ Arrive at \x1b[1m{}\x1b[0m\n", arr_time, to_name);
            }
        }
    }

    println!("=========================================================");
    println!("🎉 \x1b[1mArrived at Destination at {}\x1b[0m", end_time);
    println!("=========================================================\n");
}
