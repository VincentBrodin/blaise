use std::{env, fs::File, time::Instant};

use blaise::{
    raptor::{
        itinerary::{Itinerary, LegType},
        query::{Location, RaptorQuery},
        solve,
        state::State,
    },
    spatial::SpatialHash,
};
use gtfs_bin::{consumer::Consumer, models::Coordinate};
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

    let query = RaptorQuery::new(
        Coordinate::new(59.18690599771732, 17.837204172152255).into(),
        Coordinate::new(65.58345592681158, 22.16246562088225).into(),
    );

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
        Location::Coordinate(coord) => format!("Location ({:?})", coord),
    }
}

pub fn print_itinerary(itinerary: &Itinerary, consumer: &Consumer) {
    println!("\n=========================================================");
    println!("🗺️  JOURNEY ITINERARY");
    println!("=========================================================\n");

    if itinerary.legs.is_empty() {
        println!("No travel needed. You are already at your destination!");
        return;
    }

    // Relying entirely on your Display implementation for Time!
    let start_time = itinerary.legs.first().unwrap().departure_time.scheduled;
    let end_time = itinerary.legs.last().unwrap().arrival_time.scheduled;
    let origin_name = format_location(&itinerary.from, consumer);
    let dest_name = format_location(&itinerary.to, consumer);

    println!("📍 From:  {}", origin_name);
    println!("📍 To:    {}", dest_name);
    println!("🕒 Time:  {} -> {}", start_time, end_time);
    println!("---------------------------------------------------------\n");

    for leg in itinerary.legs.iter() {
        let dep_time = leg.departure_time.scheduled;
        let arr_time = leg.arrival_time.scheduled;
        let from_name = format_location(&leg.from, consumer);
        let to_name = format_location(&leg.to, consumer);

        match leg.leg_type {
            LegType::Transit => {
                println!("🚌 TRANSIT");
                println!("  {}  Board at {}", dep_time, from_name);

                let stops_count = leg.stops.len();
                if stops_count > 2 {
                    println!("   |     ... {} intermediate stops", stops_count - 2);
                } else {
                    println!("   |     ... direct routing");
                }

                println!("  {}  Alight at {}\n", arr_time, to_name);
            }
            LegType::Transfer => {
                println!("🚶 WALK / TRANSFER");
                println!("  {}  Leave {}", dep_time, from_name);
                println!("   |     ... ");
                println!("  {}  Arrive at {}\n", arr_time, to_name);
            }
            LegType::Origin => {
                println!("🚶 STARTING WALK");
                println!("  {}  Leave {}", dep_time, from_name);
                println!("   |     ... ");
                println!("  {}  Arrive at {}\n", arr_time, to_name);
            }
        }
    }

    println!("=========================================================");
    println!("🎉 Arrived at Destination at {}", end_time);
    println!("=========================================================\n");
}
