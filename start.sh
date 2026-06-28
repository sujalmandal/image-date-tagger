#!/bin/zsh
set -e

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGE_NAME="image-date-tagger"
CONTAINER_NAME="image-date-tagger"
VOLUME_NAME="image-date-tagger-data"
ENV_FILE="${APP_DIR}/.env"

# Parse optional flags
ATTACH=false
EXPORT=false
if [[ "${1:-}" == "--attach" || "${1:-}" == "-f" ]]; then
  ATTACH=true
elif [[ "${1:-}" == "--export" ]]; then
  EXPORT=true
fi

cd "$APP_DIR"

# Export mode: copy data from the named volume back to host via tar stream
if [[ "$EXPORT" == true ]]; then
  echo "Exporting volume data back to ${APP_DIR}/data-export ..."
  mkdir -p "${APP_DIR}/data-export"
  docker run --rm -v "${VOLUME_NAME}:/app/data" alpine tar -C /app/data -cf - . \
    | tar -C "${APP_DIR}/data-export" -xf -
  echo "Exported to ${APP_DIR}/data-export"
  exit 0
fi

# Ensure data directories exist
mkdir -p data/uploads

# Load .env if it exists, otherwise use defaults
if [[ -f "$ENV_FILE" ]]; then
  echo "Loading config from ${ENV_FILE}"
  set -a
  source "$ENV_FILE"
  set +a
fi

# Docker-friendly defaults
export OCR_URL="${OCR_URL:-http://host.docker.internal:1234/v1}"
export OCR_MODEL="${OCR_MODEL:-gemma4-26b-a4b-qat-uncensored-hauhaucs-balanced-mtp}"
export OCR_API_KEY="${OCR_API_KEY:-}"

echo "Checking Docker daemon (timeout 10s)..."
if ! .venv/bin/python3.13 - << 'PY'
import subprocess, sys
try:
    subprocess.run(['docker','info'], capture_output=True, timeout=10, check=True)
    sys.exit(0)
except Exception:
    sys.exit(1)
PY
then
  echo "ERROR: Docker daemon is not responding."
  echo "Please start Docker Desktop first, then re-run this script."
  exit 1
fi

echo "Building Docker image ${IMAGE_NAME}..."
docker build --progress=plain -t "${IMAGE_NAME}" .

# Stop and remove any existing container
if docker ps -a --format '{{.Names}}' | grep -qx "${CONTAINER_NAME}"; then
  echo "Stopping existing container ${CONTAINER_NAME}..."
  docker stop "${CONTAINER_NAME}" >/dev/null || true
  docker rm "${CONTAINER_NAME}" >/dev/null || true
fi

# Create/ensure the named volume exists
docker volume inspect "${VOLUME_NAME}" >/dev/null 2>&1 || docker volume create "${VOLUME_NAME}"

# Seed the volume with existing host data if it is empty (tar stream avoids bind mounts)
SEED_MARKER=$(docker run --rm -v "${VOLUME_NAME}:/app/data" alpine ls -A /app/data 2>/dev/null || true)
if [[ -z "$SEED_MARKER" ]]; then
  echo "Seeding volume from ${APP_DIR}/data ..."
  tar -C "${APP_DIR}/data" -cf - . | docker run -i --rm -v "${VOLUME_NAME}:/app/data" alpine tar -C /app/data -xf -
fi

echo "Starting app at http://127.0.0.1:8000"
docker run -d \
  --name "${CONTAINER_NAME}" \
  -p 8000:8000 \
  -v "${VOLUME_NAME}:/app/data" \
  -e OCR_URL \
  -e OCR_MODEL \
  -e OCR_API_KEY \
  "${IMAGE_NAME}"

echo "Container ${CONTAINER_NAME} started."
echo "Data is stored in Docker volume '${VOLUME_NAME}'."
echo "Run './start.sh --export' to copy it back to the host."

if [[ "$ATTACH" == true ]]; then
  echo "Following logs (Ctrl+C to detach)..."
  docker logs -f "${CONTAINER_NAME}"
else
  echo "Run './start.sh --attach' to follow logs."
fi
