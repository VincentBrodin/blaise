pub(crate) mod fuzzy;
pub mod geo;
pub mod time;
pub(crate) mod utils;

pub use geo::*;
pub use time::*;

use rayon::prelude::*;
use std::cmp::Ordering;

use crate::repository::{Repository, Slice};

pub trait Identifiable {
    fn id(&self) -> &str;
    fn name_slice(&self) -> &Slice;
}

/// Generic fuzzy search function built for multithreaded searching.
pub fn search<'a, T>(needle: &'a str, haystack: &'a [T], repository: &Repository) -> Vec<&'a T>
where
    T: Send + Sync + Identifiable,
{
    let normalized_needle = needle.to_lowercase();
    let mut results: Vec<(&T, f64)> = haystack
        .par_iter()
        .filter_map(|hay| {
            let hay_name = repository.str_by_slice(hay.name_slice());
            let score = fuzzy::score(&normalized_needle, hay_name);
            if score > 0.1 {
                Some((hay, score))
            } else {
                None
            }
        })
        .collect();

    results.par_sort_unstable_by(|(_, a): &(_, f64), (_, b): &(_, f64)| {
        b.partial_cmp(a).unwrap_or(Ordering::Equal)
    });
    results.into_iter().map(|(entity, _)| entity).collect()
}
