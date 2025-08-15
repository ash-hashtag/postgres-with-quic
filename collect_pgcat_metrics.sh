#!/bin/bash


container_name="pgcat-with-quic"
cid=$(docker inspect $container_name --format '{{.Id}}')

if [ -z "$cid" ]; then
    echo "Container not found"
    exit 1
fi

CGROUP="/sys/fs/cgroup/system.slice/docker-${cid}.scope"
# Header
echo "timestamp,CPU_usec,Mem_used_bytes"


while true; do
  # Data
  CPU=$(awk '/usage_usec/ {print $2}' $CGROUP/cpu.stat)
  MEM=$(cat $CGROUP/memory.current)
  TS=$(date +%s)
  echo "$TS,$CPU,$MEM"

  sleep 1

done
