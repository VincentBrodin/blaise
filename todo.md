# TODO

- **1**: Rewrite GTFS Reader to improve performance by reading all files in paralell and then consume them after. Current implementation streams each file in sync, this makes the GTFS Reader very IO bound. In theory we should grante a 50% reduction in time since the 2 main blokcing jobs are stop_times.txt and shapes.txt, and thus reading them in parallel would half the time.
