#!/bin/sh
echo "PORT set to " $PORT
/app/unleash-edge --port 3063 edge & nginx -g "daemon off;"
