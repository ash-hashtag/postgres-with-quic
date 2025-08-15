
#!/bin/bash

cmd="dotenv -f .env.tcp cargo run --release"
pid=0

# CSV header
echo "timestamp,cpu_user_ticks,cpu_system_ticks,memory_bytes,rx_bytes,tx_bytes"

# Record initial network counters (all interfaces)
read rx0 tx0 <<< $(awk '/:/ {sum_rx+=$2; sum_tx+=$10} END {print sum_rx, sum_tx}' /proc/net/dev)

# Start process
$cmd >/tmp/pgclient-tcp-log 2>&1 &
pid=$!

page_size=$(getconf PAGESIZE)
while kill -0 $pid 2>/dev/null; do
    ts=$(date +%s)
    
    # CPU times
    read utime stime <<< $(awk '{print $14, $15}' /proc/$pid/stat)
    
    # Memory RSS in bytes
    rss=$(awk '{print $24}' /proc/$pid/stat)
    mem_bytes=$((rss * page_size))
    
    # Network I/O (system-wide delta)
    read rx1 tx1 <<< $(awk '/:/ {sum_rx+=$2; sum_tx+=$10} END {print sum_rx, sum_tx}' /proc/net/dev)
    rx_bytes=$((rx1 - rx0))
    tx_bytes=$((tx1 - tx0))
    
    echo "$ts,$utime,$stime,$mem_bytes,$rx_bytes,$tx_bytes"
    
    sleep 1
done
