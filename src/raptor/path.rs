use tracing::trace;

use crate::{
    raptor::{self, Allocator, Parent, Point, TimeConstraint},
    repository::Repository,
};

pub fn backtrack(
    repository: &Repository,
    allocator: &Allocator,
    target_stop: u32,
    target_round: usize,
    time_constraint: TimeConstraint,
) -> Result<Vec<Parent>, raptor::Error> {
    let mut path: Vec<Parent> = Vec::new();
    let mut current_point: Point = target_stop.into();
    let mut current_round = target_round;

    let is_arrival = matches!(time_constraint, TimeConstraint::Arrival(_));

    while let Point::Stop(current_stop) = current_point {
        let stop = &repository.stops[current_stop as usize];
        trace!(
            "Looking at stop: [{}] {} in round {current_round}",
            repository.stop_str_by_slice(&stop.id_slice),
            repository.stop_str_by_slice(&stop.name_slice)
        );
        if let Some(parent) = &allocator.get_parents(current_round)[current_stop as usize] {
            path.push(*parent);
            current_point = if is_arrival { parent.to } else { parent.from };

            // If we are on a transit we decrese the round else we don't since
            // transfers does not count as a round switch
            if parent.parent_type.is_transit() {
                if current_round == 0 {
                    break;
                } else {
                    current_round -= 1;
                }
            }
        } else {
            break;
        }
    }

    if !is_arrival {
        path.reverse();
    }

    if path.is_empty() {
        return Err(raptor::Error::FailedToBuildRoute);
    }

    Ok(path)
}
