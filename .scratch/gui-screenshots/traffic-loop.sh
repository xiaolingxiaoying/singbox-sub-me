#!/bin/bash
# Continuous direct traffic through the sbgui mixed port so the dashboard,
# connections and logs pages have live data while capturing.
head -c 8388608 /dev/urandom > /tmp/sbgui-up.bin
(
  while true; do
    curl -s -x http://127.0.0.1:2080 --limit-rate 900k \
      "http://speed.cloudflare.com/__down?bytes=20000000" -o /dev/null
    sleep 1
  done
) &
(
  while true; do
    curl -s -x http://127.0.0.1:2080 --limit-rate 300k \
      --data-binary @/tmp/sbgui-up.bin "https://speed.cloudflare.com/__up" -o /dev/null
    sleep 1
  done
) &
wait
