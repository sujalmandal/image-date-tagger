from fastapi import FastAPI, HTTPException, UploadFile, File
from fastapi.staticfiles import StaticFiles
from fastapi.responses import FileResponse, HTMLResponse, JSONResponse
from pydantic import BaseModel
from pathlib import Path
from typing import Optional
import json
import os
import re
from datetime import datetime
import base64
import requests
from dotenv import load_dotenv

APP_DIR = Path(__file__).resolve().parent
DATA_DIR = APP_DIR / "data"
ANNOTATIONS_FILE = DATA_DIR / "annotations.json"
ROOT_DIR = APP_DIR / "data" / "uploads"  # where the .jpg files live
STATIC_DIR = APP_DIR / "static"
TEMPLATES_DIR = APP_DIR / "templates"

# Load environment variables from .env if present
load_dotenv(APP_DIR / ".env")

OCR_MODEL = os.environ.get("OCR_MODEL", "gemma4-26b-a4b-qat-uncensored-hauhaucs-balanced-mtp")
OCR_URL = os.environ.get("OCR_URL", "http://localhost:1234/v1")
OCR_API_KEY = os.environ.get("OCR_API_KEY", "")
OCR_PROMPT = "Read the date from this sales receipt image. Reply with ONLY the date in DD-MM-YYYY format. If you cannot find a date, reply with NO_DATE."

app = FastAPI(title="Sales Book Image Date Annotator")
app.mount("/static", StaticFiles(directory=str(STATIC_DIR)), name="static")

class AnnotationUpdate(BaseModel):
    corrected_date: Optional[str] = None
    is_invalid: Optional[bool] = None


def ensure_annotations():
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    if not ANNOTATIONS_FILE.exists():
        ANNOTATIONS_FILE.write_text(json.dumps({"files": []}, indent=2))


def load_annotations() -> dict:
    ensure_annotations()
    try:
        return json.loads(ANNOTATIONS_FILE.read_text())
    except Exception:
        return {"files": []}


def save_annotations(data: dict):
    DATA_DIR.mkdir(parents=True, exist_ok=True)
    tmp = ANNOTATIONS_FILE.with_suffix(".tmp")
    tmp.write_text(json.dumps(data, indent=2, ensure_ascii=False))
    tmp.replace(ANNOTATIONS_FILE)


def parse_date(text: str) -> Optional[str]:
    text = text.strip()
    # DD-MM-YYYY or DD/MM/YYYY
    m = re.search(r'\b(\d{1,2})[-/](\d{1,2})[-/](\d{4})\b', text)
    if m:
        d, mo, y = m.group(1).zfill(2), m.group(2).zfill(2), m.group(3)
        if 1 <= int(d) <= 31 and 1 <= int(mo) <= 12:
            return f"{d}-{mo}-{y}"
    # DD-MM-YY or DD/MM/YY
    m = re.search(r'\b(\d{1,2})[-/](\d{1,2})[-/](\d{2})\b', text)
    if m:
        d, mo, y = m.group(1).zfill(2), m.group(2).zfill(2), f"20{m.group(3)}"
        if 1 <= int(d) <= 31 and 1 <= int(mo) <= 12:
            return f"{d}-{mo}-{y}"
    if re.search(r'NO_DATE|no date', text, re.I):
        return None
    return None


def list_image_files():
    files = []
    if ROOT_DIR.exists():
        files = [f for f in os.listdir(ROOT_DIR) if f.lower().endswith(".jpg")]
    return sorted(files)


def sync_annotations():
    """Make sure every image file has an annotation entry, keeping existing data."""
    data = load_annotations()
    existing = {entry["filename"]: entry for entry in data.get("files", [])}
    files = list_image_files()
    new_files = []
    for idx, fname in enumerate(files):
        entry = existing.get(fname, {})
        entry.setdefault("filename", fname)
        entry.setdefault("extracted_date", None)
        entry.setdefault("corrected_date", None)
        entry.setdefault("is_invalid", False)
        entry.setdefault("sort_index", idx)
        new_files.append(entry)
    data["files"] = new_files
    save_annotations(data)


@app.on_event("startup")
def startup():
    sync_annotations()


@app.get("/", response_class=HTMLResponse)
def index():
    return HTMLResponse(content=(TEMPLATES_DIR / "index.html").read_text())


@app.get("/api/files")
def get_files():
    sync_annotations()
    data = load_annotations()
    return data["files"]


@app.put("/api/files/{filename}")
def update_file(filename: str, update: AnnotationUpdate):
    data = load_annotations()
    entry = next((e for e in data["files"] if e["filename"] == filename), None)
    if entry is None:
        raise HTTPException(status_code=404, detail="File not found")
    if update.corrected_date is not None:
        entry["corrected_date"] = update.corrected_date or None
    if update.is_invalid is not None:
        entry["is_invalid"] = bool(update.is_invalid)
    save_annotations(data)
    return entry


@app.get("/api/images/{filename}")
def get_image(filename: str):
    path = ROOT_DIR / filename
    if not path.exists():
        raise HTTPException(status_code=404, detail="Image not found")
    return FileResponse(path)


@app.get("/api/dates")
def get_dates():
    data = load_annotations()
    date_map = {}
    for entry in data["files"]:
        date = entry.get("corrected_date") or entry.get("extracted_date")
        if not date:
            continue
        if entry.get("is_invalid"):
            continue
        date_map.setdefault(date, []).append(entry["filename"])
    sorted_dates = sorted(date_map.keys(), key=lambda d: datetime.strptime(d, "%d-%m-%Y"))
    return [{"date": d, "files": date_map[d]} for d in sorted_dates]


def mime_type_from_path(path: Path) -> str:
    ext = path.suffix.lower()
    mapping = {
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
        ".png": "image/png",
        ".webp": "image/webp",
    }
    return mapping.get(ext, "image/jpeg")


def ocr_with_python(path: Path) -> str:
    """Call the vision model directly via OpenAI-compatible chat completions."""
    image_bytes = path.read_bytes()
    b64 = base64.b64encode(image_bytes).decode("utf-8")
    mime = mime_type_from_path(path)
    url = OCR_URL.rstrip("/") + "/chat/completions"
    headers = {"Content-Type": "application/json"}
    if OCR_API_KEY:
        headers["Authorization"] = f"Bearer {OCR_API_KEY}"
    payload = {
        "model": OCR_MODEL,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": OCR_PROMPT},
                    {"type": "image_url", "image_url": {"url": f"data:{mime};base64,{b64}"}},
                ],
            }
        ],
        "max_tokens": 50,
        "temperature": 0.1,
    }
    try:
        r = requests.post(url, headers=headers, json=payload, timeout=120)
        r.raise_for_status()
        data = r.json()
        choices = data.get("choices", [])
        if choices:
            return choices[0].get("message", {}).get("content", "")
        return "ERROR: no choices in response"
    except requests.exceptions.Timeout:
        return "ERROR: timeout"
    except requests.exceptions.RequestException as e:
        return f"ERROR: {e}"
    except Exception as e:
        return f"ERROR: {e}"


@app.post("/api/ocr/{filename}")
def run_ocr(filename: str):
    """Run OCR on a single file and update its extracted_date."""
    path = ROOT_DIR / filename
    if not path.exists():
        raise HTTPException(status_code=404, detail="Image not found")
    try:
        output = ocr_with_python(path)
    except Exception as e:
        output = f"ERROR: {e}"

    extracted = parse_date(output)
    data = load_annotations()
    entry = next((e for e in data["files"] if e["filename"] == filename), None)
    if entry is None:
        raise HTTPException(status_code=404, detail="File not found")
    entry["extracted_date"] = extracted
    save_annotations(data)
    return {"filename": filename, "raw": output, "extracted_date": extracted}


@app.post("/api/ocr-batch")
def run_ocr_batch(filenames: list[str]):
    """Run OCR on a list of files."""
    results = []
    for fname in filenames:
        try:
            results.append(run_ocr(fname))
        except Exception as e:
            results.append({"filename": fname, "raw": f"ERROR: {e}", "extracted_date": None})
    return results


@app.get("/api/ocr-progress")
def ocr_progress():
    """Return count of files with/without extracted_date."""
    data = load_annotations()
    files = data.get("files", [])
    done = sum(1 for e in files if e.get("extracted_date"))
    return {"total": len(files), "done": done, "remaining": len(files) - done}


@app.post("/api/upload")
def upload_files(files: list[UploadFile] = File(...)):
    """Upload images to data/uploads and sync annotations."""
    ROOT_DIR.mkdir(parents=True, exist_ok=True)
    saved = []
    skipped = []
    for upload in files:
        if not upload.filename:
            continue
        ext = Path(upload.filename).suffix.lower()
        if ext not in (".jpg", ".jpeg", ".png", ".webp"):
            skipped.append(upload.filename)
            continue
        dest = ROOT_DIR / upload.filename
        with dest.open("wb") as f:
            f.write(upload.file.read())
        saved.append(upload.filename)
    sync_annotations()
    return {"saved": saved, "skipped": skipped, "count": len(saved)}
