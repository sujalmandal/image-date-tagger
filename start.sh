#!/bin/zsh
set -e

APP_DIR="$(cd "$(dirname "$0")" && pwd)"
IMAGE_NAME="image-date-tagger"
CONTAINER_NAME="image-date-tagger"
ENV_FILE="${APP_DIR}/.env"

# Parse optional flags
ATTACH=false
if [[ "${1:-}" == "--attach" || "${1:-}" == "-f" ]]; then
  ATTACH=true
fi

cd "$APP_DIR"

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

echo "Starting app at http://127.0.0.1:8000"
docker run -d \
  --name "${CONTAINER_NAME}" \
  -p 8000:8000 \
  -v "${APP_DIR}/data:/app/data" \
  -e OCR_URL \
  -e OCR_MODEL \
  -e OCR_API_KEY \
  "${IMAGE_NAME}"

echo "Container ${CONTAINER_NAME} started."

if [[ "$ATTACH" == true ]]; then
  echo "Following logs (Ctrl+C to detach)..."
  docker logs -f "${CONTAINER_NAME}"
else
  echo "Run './start.sh --attach' to follow logs."
fi
