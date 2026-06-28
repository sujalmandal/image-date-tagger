#!/bin/bash
set -e

if [ "$1" == "--cached" ]; then
  echo "Starting with existing images..."
  docker compose up -d
else
  echo "Stopping existing containers and removing old images..."
  docker compose down --rmi local 2>/dev/null || docker compose down

  echo "Building and starting fresh..."
  docker compose up --build -d
fi
