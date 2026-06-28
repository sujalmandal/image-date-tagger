(function () {
  'use strict';

  // ===================== State =====================
  let files = [];
  let currentIndex = 0;
  let currentView = 'analyse';
  let saveTimer = null;
  let escHeld = false;
  let escReturnFocus = false;

  // ===================== DOM refs =====================
  const tabAnalyse = document.getElementById('tab-analyse');
  const tabDashboard = document.getElementById('tab-dashboard');
  const viewAnalyse = document.getElementById('view-analyse');
  const viewDashboard = document.getElementById('view-dashboard');
  const fileListEl = document.getElementById('analyse-file-list');
  const analyseCountEl = document.getElementById('analyse-count');
  const viewerImage = document.getElementById('viewer-image');
  const viewerEl = document.getElementById('image-viewer');
  const detailFilename = document.getElementById('detail-filename');
  const detailExtracted = document.getElementById('detail-extracted');
  const correctedInput = document.getElementById('corrected-date');
  const invalidFlag = document.getElementById('invalid-flag');
  const dateError = document.getElementById('date-error');
  const saveStatus = document.getElementById('save-status');
  const btnPrev = document.getElementById('btn-prev');
  const btnNext = document.getElementById('btn-next');
  const ocrProgress = document.getElementById('ocr-progress');
  const ocrNextBtn = document.getElementById('ocr-next-batch');
  const ocrAllBtn = document.getElementById('ocr-all');
  const dashboardDateList = document.getElementById('dashboard-date-list');
  const dashboardImages = document.getElementById('dashboard-images');
  const dashboardViewerWrap = document.getElementById('dashboard-viewer-wrap');
  const dashboardImage = document.getElementById('dashboard-image');
  const dashboardViewerEl = document.getElementById('dashboard-image-viewer');
  const dashboardDateLabel = document.getElementById('dashboard-date-label');
  const dashImgCounter = document.getElementById('dash-img-counter');
  const dashImgPrev = document.getElementById('dash-img-prev');
  const dashImgNext = document.getElementById('dash-img-next');
  const tabUpload = document.getElementById('tab-upload');
  const viewUpload = document.getElementById('view-upload');
  const uploadBox = document.getElementById('upload-box');
  const uploadInput = document.getElementById('upload-input');
  const uploadButton = document.getElementById('upload-button');
  const uploadQueue = document.getElementById('upload-queue');
  const uploadStatus = document.getElementById('upload-status');

  // ===================== Tabs =====================
  function switchView(view) {
    currentView = view;
    [viewAnalyse, viewDashboard, viewUpload].forEach(el => el.classList.remove('active'));
    [tabAnalyse, tabDashboard, tabUpload].forEach(el => el.classList.remove('active'));

    if (view === 'analyse') {
      viewAnalyse.classList.add('active');
      tabAnalyse.classList.add('active');
    } else if (view === 'dashboard') {
      viewDashboard.classList.add('active');
      tabDashboard.classList.add('active');
      loadDashboard();
    } else if (view === 'upload') {
      viewUpload.classList.add('active');
      tabUpload.classList.add('active');
    }
  }

  tabAnalyse.addEventListener('click', () => switchView('analyse'));
  tabDashboard.addEventListener('click', () => switchView('dashboard'));
  tabUpload.addEventListener('click', () => switchView('upload'));

  // ===================== API helpers =====================
  async function api(path, opts = {}) {
    const res = await fetch(path, opts);
    if (!res.ok) throw new Error(`${res.status}: ${res.statusText}`);
    return res.json();
  }

  async function loadFiles() {
    files = await api('/api/files');
    renderFileList();
    loadCurrentFile();
    updateOcrProgress();
  }

  async function updateFile(filename, payload) {
    await api(`/api/files/${encodeURIComponent(filename)}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload)
    });
  }

  async function updateOcrProgress() {
    try {
      const p = await api('/api/ocr-progress');
      ocrProgress.textContent = `OCR ${p.done}/${p.total} done`;
    } catch (e) {
      ocrProgress.textContent = '';
    }
  }

  // ===================== Analyse view =====================
  function renderFileList() {
    fileListEl.innerHTML = '';
    files.forEach((file, idx) => {
      const li = document.createElement('li');
      if (idx === currentIndex) li.classList.add('active');
      if (file.is_invalid) li.classList.add('invalid');

      const nameSpan = document.createElement('span');
      nameSpan.textContent = file.filename;

      const dateSpan = document.createElement('span');
      dateSpan.className = 'file-date';
      const d = file.corrected_date || file.extracted_date || 'NO_DATE';
      dateSpan.textContent = d;

      li.appendChild(nameSpan);
      li.appendChild(dateSpan);
      li.addEventListener('click', () => {
        currentIndex = idx;
        loadCurrentFile();
      });
      fileListEl.appendChild(li);
    });

    const activeLi = fileListEl.children[currentIndex];
    if (activeLi) activeLi.scrollIntoView({ block: 'nearest' });

    analyseCountEl.textContent = `${currentIndex + 1} / ${files.length}`;
  }

  function loadCurrentFile() {
    const file = files[currentIndex];
    if (!file) return;

    viewerImage.src = `/api/images/${encodeURIComponent(file.filename)}`;
    detailFilename.textContent = file.filename;
    detailExtracted.textContent = file.extracted_date || 'NO_DATE';
    correctedInput.value = file.corrected_date || '';
    invalidFlag.checked = !!file.is_invalid;
    dateError.textContent = '';
    saveStatus.textContent = '';
    saveStatus.className = 'save-status';

    analyseViewer.resetZoom();
    renderFileList();
    updateNavButtons();
  }

  function updateNavButtons() {
    btnPrev.disabled = currentIndex === 0;
    btnNext.disabled = currentIndex === files.length - 1;
  }

  function focusInput() {
    correctedInput.focus();
    correctedInput.select();
  }

  function prevFile() {
    if (currentIndex > 0) {
      currentIndex--;
      loadCurrentFile();
    }
  }

  function nextFile() {
    if (currentIndex < files.length - 1) {
      currentIndex++;
      loadCurrentFile();
    }
  }

  function saveCurrent() {
    const file = files[currentIndex];
    if (!file) return;

    const raw = correctedInput.value.trim();
    const valid = validateDate(raw);
    const payload = {
      corrected_date: raw || null,
      is_invalid: invalidFlag.checked
    };

    updateFile(file.filename, payload)
      .then(() => {
        file.corrected_date = payload.corrected_date;
        file.is_invalid = payload.is_invalid;
        saveStatus.textContent = 'Saved';
        saveStatus.className = 'save-status saved';
        if (!valid) {
          saveStatus.textContent = 'Saved (date format not DD-MM-YYYY)';
          saveStatus.className = 'save-status error';
        }
        renderFileList();
      })
      .catch(err => {
        saveStatus.textContent = `Error: ${err.message}`;
        saveStatus.className = 'save-status error';
      });
  }

  function debouncedSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveStatus.textContent = 'Saving...';
    saveStatus.className = 'save-status';
    saveTimer = setTimeout(saveCurrent, 400);
  }

  function validateDate(str) {
    if (!str) return true;
    if (!/^\d{2}-\d{2}-\d{4}$/.test(str)) {
      dateError.textContent = 'Expected DD-MM-YYYY';
      return false;
    }
    dateError.textContent = '';
    return true;
  }

  correctedInput.addEventListener('input', () => {
    validateDate(correctedInput.value.trim());
    debouncedSave();
  });
  correctedInput.addEventListener('change', saveCurrent);
  correctedInput.addEventListener('focus', () => { escReturnFocus = false; });
  invalidFlag.addEventListener('change', () => { saveCurrent(); renderFileList(); });
  btnPrev.addEventListener('click', prevFile);
  btnNext.addEventListener('click', nextFile);

  // ===================== Keyboard =====================
  document.addEventListener('keydown', (e) => {
    if (currentView !== 'analyse') return;

    if (e.key === 'Escape') {
      e.preventDefault();
      escHeld = true;
      escReturnFocus = (document.activeElement === correctedInput);
      if (escReturnFocus) {
        validateDate(correctedInput.value.trim());
        saveCurrent();
        correctedInput.blur();
      }
      return;
    }

    if (escHeld && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
      e.preventDefault();
      if (e.key === 'ArrowLeft') prevFile();
      else nextFile();
      return;
    }

    if (document.activeElement === correctedInput) {
      if (e.key === 'Enter') {
        e.preventDefault();
        validateDate(correctedInput.value.trim());
        saveCurrent();
        correctedInput.blur();
        nextFile();
        focusInput();
        return;
      }
      return;
    }

    if (e.key === 'ArrowLeft') {
      e.preventDefault();
      prevFile();
      focusInput();
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      nextFile();
      focusInput();
    } else if (e.key.toLowerCase() === 'i') {
      e.preventDefault();
      invalidFlag.checked = !invalidFlag.checked;
      saveCurrent();
      renderFileList();
    }
  });

  document.addEventListener('keyup', (e) => {
    if (e.key === 'Escape') {
      escHeld = false;
      if (escReturnFocus) {
        escReturnFocus = false;
        focusInput();
      }
    }
  });

  // ===================== Image viewer pan/zoom =====================
  function createViewer(container, imgEl) {
    let scale = 1;
    let baseScale = 1;
    let translateX = 0;
    let translateY = 0;
    let isDragging = false;
    let startX = 0;
    let startY = 0;

    function updateTransform() {
      imgEl.style.transform = `translate(${translateX}px, ${translateY}px) scale(${scale})`;
    }

    function fitToScreen() {
      // Wait a tick so the browser has laid out the container
      requestAnimationFrame(() => {
        const cW = container.clientWidth;
        const cH = container.clientHeight;
        const iW = imgEl.naturalWidth || imgEl.width || cW;
        const iH = imgEl.naturalHeight || imgEl.height || cH;
        if (!iW || !iH || !cW || !cH) return;
        const ratio = Math.min(cW / iW, cH / iH);
        baseScale = ratio;
        scale = baseScale;
        translateX = 0;
        translateY = 0;
        updateTransform();
      });
    }

    function resetZoom() {
      scale = baseScale;
      translateX = 0;
      translateY = 0;
      updateTransform();
    }

    function onImageLoad() {
      fitToScreen();
    }

    imgEl.addEventListener('load', onImageLoad);
    if (imgEl.complete && imgEl.naturalWidth) {
      fitToScreen();
    }

    container.addEventListener('wheel', (e) => {
      e.preventDefault();
      const factor = e.deltaY > 0 ? 0.9 : 1.1;
      const newScale = Math.min(Math.max(scale * factor, 0.1), 6);
      scale = newScale;
      updateTransform();
    }, { passive: false });

    container.addEventListener('mousedown', (e) => {
      if (e.button !== 0) return;
      isDragging = true;
      startX = e.clientX - translateX;
      startY = e.clientY - translateY;
      container.classList.add('active');
    });

    function onMove(e) {
      if (!isDragging) return;
      translateX = e.clientX - startX;
      translateY = e.clientY - startY;
      updateTransform();
    }

    function onUp() {
      isDragging = false;
      container.classList.remove('active');
    }

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    window.addEventListener('resize', fitToScreen);

    container.addEventListener('dblclick', resetZoom);

    return {
      setImage(src) {
        imgEl.src = src;
      },
      resetZoom,
      destroy() {
        imgEl.removeEventListener('load', onImageLoad);
        window.removeEventListener('mousemove', onMove);
        window.removeEventListener('mouseup', onUp);
        window.removeEventListener('resize', fitToScreen);
      }
    };
  }

  const analyseViewer = createViewer(viewerEl, viewerImage);

  // ===================== OCR buttons =====================
  async function runOcrBatch(count, all = false) {
    const pending = files.filter(f => !f.extracted_date && !f.is_invalid);
    const batch = pending.slice(0, count);
    if (!batch.length) {
      alert('No unprocessed files left.');
      return;
    }

    const names = batch.map(f => f.filename);
    const max = all ? names.length : 7;
    const toRun = names.slice(0, max);
    if (!toRun.length) return;

    setOcrRunning(true);
    try {
      const results = await api('/api/ocr-batch', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(toRun)
      });
      results.forEach(r => {
        const f = files.find(x => x.filename === r.filename);
        if (f) f.extracted_date = r.extracted_date;
      });
      loadCurrentFile();
      renderFileList();
      updateOcrProgress();
    } catch (e) {
      alert('OCR batch failed: ' + e.message);
    } finally {
      setOcrRunning(false);
    }
  }

  function setOcrRunning(running) {
    ocrNextBtn.disabled = running;
    ocrAllBtn.disabled = running;
    if (running) {
      ocrProgress.innerHTML = '<span class="spinner"></span> Running OCR...';
    }
  }

  ocrNextBtn.addEventListener('click', () => runOcrBatch(7, false));
  ocrAllBtn.addEventListener('click', () => {
    if (confirm('Run OCR on all remaining unprocessed images? This may take a while.')) {
      runOcrBatch(files.length, true);
    }
  });

  // ===================== Dashboard =====================
  let dashboardDateFiles = [];
  let dashboardImageIndex = 0;
  const dashboardViewer = createViewer(dashboardViewerEl, dashboardImage);

  async function loadDashboard() {
    const dates = await api('/api/dates');
    dashboardDateList.innerHTML = '';

    if (!dates.length) {
      const empty = document.createElement('div');
      empty.className = 'empty-state';
      empty.textContent = 'No corrected dates yet. Go to Analyse tab first.';
      dashboardDateList.appendChild(empty);
      hideDashboardViewer();
      return;
    }

    dates.forEach(entry => {
      const div = document.createElement('div');
      div.className = 'date-item';
      div.dataset.date = entry.date;

      const label = document.createElement('span');
      label.className = 'date-label';
      label.textContent = entry.date;

      const count = document.createElement('span');
      count.className = 'date-count';
      count.textContent = `${entry.files.length} image${entry.files.length > 1 ? 's' : ''}`;

      div.appendChild(label);
      div.appendChild(count);
      div.addEventListener('click', () => showDashboardDate(entry.date, entry.files));
      dashboardDateList.appendChild(div);
    });
  }

  function hideDashboardViewer() {
    dashboardImages.querySelector('.empty-state')?.classList.remove('hidden');
    dashboardViewerWrap.classList.add('hidden');
  }

  function showDashboardViewer() {
    dashboardImages.querySelector('.empty-state')?.classList.add('hidden');
    dashboardViewerWrap.classList.remove('hidden');
  }

  function updateDashboardImage() {
    const filename = dashboardDateFiles[dashboardImageIndex];
    dashboardImage.src = `/api/images/${encodeURIComponent(filename)}`;
    dashboardViewer.resetZoom();
    dashImgCounter.textContent = `${dashboardImageIndex + 1} / ${dashboardDateFiles.length}`;
    dashImgPrev.disabled = dashboardImageIndex === 0;
    dashImgNext.disabled = dashboardImageIndex === dashboardDateFiles.length - 1;
  }

  async function showDashboardDate(date, filenames) {
    document.querySelectorAll('#dashboard-date-list .date-item').forEach(el => {
      el.classList.toggle('active', el.dataset.date === date);
    });

    dashboardDateFiles = filenames;
    dashboardImageIndex = 0;
    dashboardDateLabel.textContent = date;
    showDashboardViewer();
    updateDashboardImage();
  }

  function dashPrevImage() {
    if (dashboardImageIndex > 0) {
      dashboardImageIndex--;
      updateDashboardImage();
    }
  }

  function dashNextImage() {
    if (dashboardImageIndex < dashboardDateFiles.length - 1) {
      dashboardImageIndex++;
      updateDashboardImage();
    }
  }

  dashImgPrev.addEventListener('click', dashPrevImage);
  dashImgNext.addEventListener('click', dashNextImage);

  // ===================== Upload =====================
  function createQueueItem(file) {
    const div = document.createElement('div');
    div.className = 'upload-item';
    div.innerHTML = `
      <span class="name">${file.name}</span>
      <span class="status">pending</span>
    `;
    return div;
  }

  async function uploadFiles(fileList) {
    if (!fileList || fileList.length === 0) return;
    uploadStatus.textContent = '';
    const items = [];
    for (const file of fileList) {
      const item = createQueueItem(file);
      uploadQueue.appendChild(item);
      items.push({ file, item });
    }

    const formData = new FormData();
    items.forEach(({ file }) => formData.append('files', file));

    try {
      const res = await fetch('/api/upload', { method: 'POST', body: formData });
      if (!res.ok) throw new Error(`${res.status}: ${res.statusText}`);
      const data = await res.json();

      const savedSet = new Set(data.saved || []);
      items.forEach(({ file, item }) => {
        const status = item.querySelector('.status');
        if (savedSet.has(file.name)) {
          status.textContent = 'saved';
          status.className = 'status done';
        } else {
          status.textContent = 'skipped';
          status.className = 'status error';
        }
      });
      uploadStatus.textContent = `${data.count} file${data.count === 1 ? '' : 's'} uploaded. ${data.skipped.length ? data.skipped.length + ' skipped.' : ''}`;
      await loadFiles();
      await updateOcrProgress();
    } catch (e) {
      uploadStatus.textContent = `Upload failed: ${e.message}`;
      items.forEach(({ item }) => {
        const status = item.querySelector('.status');
        status.textContent = 'error';
        status.className = 'status error';
      });
    }
  }

  uploadInput.addEventListener('change', (e) => {
    uploadQueue.innerHTML = '';
    uploadFiles(e.target.files);
    uploadInput.value = '';
  });

  uploadBox.addEventListener('dragover', (e) => {
    e.preventDefault();
    uploadBox.classList.add('dragover');
  });
  uploadBox.addEventListener('dragleave', () => {
    uploadBox.classList.remove('dragover');
  });
  uploadBox.addEventListener('drop', (e) => {
    e.preventDefault();
    uploadBox.classList.remove('dragover');
    uploadQueue.innerHTML = '';
    uploadFiles(e.dataTransfer.files);
  });

  uploadButton.addEventListener('click', (e) => {
    e.preventDefault();
    uploadInput.click();
  });

  // ===================== Init =====================
  loadFiles();
})();
