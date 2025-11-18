#!/bin/bash

trap 'rm /var/run/spo-api/running' EXIT
trap 'kill -SIGINT $PID' INT
trap 'kill -SIGTERM $PID' TERM

touch /var/run/spo-api/running
spo-api &
PID=$!
wait $PID
