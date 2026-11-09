#!/bin/sh
# Run a command with at most 2 GB of memory and no swap, at low priority, so a mistake
# cannot take the machine's memory: the kernel stops the command instead.
exec systemd-run --user --scope -q -p MemoryMax=${CAP:-2G} -p MemorySwapMax=0 nice -n 19 ionice -c3 "$@"
