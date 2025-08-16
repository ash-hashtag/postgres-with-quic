#!/bin/bash
set -e

OUTFILE="stats.csv"

if [[ "$1" == "-f" ]]; then
    OUTFILE="$2"
    shift 2
fi

"$@" &
PID=$!


trap "kill -INT $PID 2>/dev/null; wait $PID; exit" INT
trap "kill -TERM $PID 2>/dev/null; wait $PID; exit" TERM

CLK_TCK=$(getconf CLK_TCK)  # clock ticks per second

prev_cpu=0

echo "timestamp_ms,cpu_ms_per_sec,memory_bytes" > $OUTFILE

while kill -0 $PID 2>/dev/null; do
    # CPU time in ticks
    read utime stime < <(awk '{print $14, $15}' /proc/$PID/stat)
    cpu_total=$((utime + stime))
    cpu_diff=$((cpu_total - prev_cpu))
    CPU_MS=$((cpu_diff * 1000 / CLK_TCK))
    prev_cpu=$cpu_total

    # Memory in bytes
    RSS_KB=$(awk '/VmRSS/ {print $2}' /proc/$PID/status)
    MEM_BYTES=$((RSS_KB * 1024))

    # echo "CPU: ${CPU_MS} ms/s | MEM: ${MEM_BYTES} B"

    TS=$(date +%s)

    echo "${TS},${CPU_MS},${MEM_BYTES}" >>  $OUTFILE
    
    sleep 1
done
