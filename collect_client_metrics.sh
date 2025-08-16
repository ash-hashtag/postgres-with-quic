
#!/bin/bash

csv_out="metrics/client_${CONN_MODE}_metrics.csv"
dotenv -f .env.${CONN_MODE} ./monitor.sh -f "${csv_out}" cargo run --release




# # Start process
# $cmd &
# # pid=$! doesn't work because of dotenv
# pid=$(pgrep -f postgres-with-quic)

# # CSV header
# echo "timestamp,cpu_user_ticks,cpu_system_ticks,memory_bytes,rx_bytes,tx_bytes" > $csv_out


# page_size=$(getconf PAGESIZE)
# while kill -0 $pid 2>/dev/null; do
#     ts=$(date +%s)
    
#     # CPU times
#     read utime stime <<< $(awk '{print $14, $15}' /proc/$pid/stat)
    
#     # Memory RSS in bytes
#     rss=$(awk '{print $24}' /proc/$pid/stat)
#     mem_bytes=$((rss * page_size))
    
#     # Network I/O (system-wide delta)
#     read rx1 tx1 <<< $(awk '/:/ {sum_rx+=$2; sum_tx+=$10} END {print sum_rx, sum_tx}' /proc/net/dev)
#     rx_bytes=$((rx1 - rx0))
#     tx_bytes=$((tx1 - tx0))
    
#     echo "$ts,$utime,$stime,$mem_bytes,$rx_bytes,$tx_bytes" >> $csv_out
    
#     sleep 1
# done
