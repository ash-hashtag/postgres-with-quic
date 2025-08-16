#!/bin/bash


# Run client metrics in background
./collect_client_metrics.sh &
pid1=$!
# Run server metrics in background
./collect_pgcat_metrics.sh &
pid2=$!

# Wait for either to exit
wait -n $pid1 $pid2

# Kill whichever is still running
kill $pid1 2>/dev/null
kill $pid2 2>/dev/null

