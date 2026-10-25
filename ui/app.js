const { event, core, clipboardManager } = window.__TAURI__;

const STORAGE_KEY = 'clip-history-items-v2';
const SETTINGS_KEY = 'clip-history-settings-v1';
const PAUSED_KEY = 'clip-history-paused-v1';
const DB_NAME = 'clip-history-binary-v1';
const DB_STORE = 'images';
const DEFAULT_SETTINGS = { theme: 'system', size: 'standard', position: 'center', maxItems: 100, maxAgeDays: 30, maxStorageMb: 1024 };
const SIZE_PRESETS = { compact: [620, 420], standard: [760, 500], wide: [900, 650] };

const historyElement = document.querySelector('#history');
const countLabel = document.querySelector('#count-label');
const searchInput = document.querySelector('#search');
const pauseButton = document.querySelector('#pause');
const settingsStatus = document.querySelector('#settings-status');

let settings = loadSettings();
let items = loadItems();
let paused = localStorage.getItem(PAUSED_KEY) === 'true';
let imageUrls = [];

function loadItems() {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? localStorage.getItem('clip-history-items-v1') ?? '[]');
    return Array.isArray(saved) ? saved.map((savedItem) => {
      const item = { ...savedItem, kind: savedItem.kind ?? 'text' };
      // Migrate early image records, which incorrectly kept raw pixels in localStorage.
      if (item.kind === 'image') delete item.bytes;
      return item;
    }) : [];
  } catch { return []; }
}

function loadSettings() {
  try { return { ...DEFAULT_SETTINGS, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') }; }
  catch { return { ...DEFAULT_SETTINGS }; }
}

function database() {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(DB_STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

async function imageTransaction(mode, callback) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(DB_STORE, mode);
    const result = callback(transaction.objectStore(DB_STORE));
    transaction.oncomplete = () => { db.close(); resolve(result?.result); };
    transaction.onerror = () => { db.close(); reject(transaction.error); };
  });
}

async function saveImage(item) {
  const bytes = new Uint8ClampedArray(item.bytes);
  const canvas = document.createElement('canvas');
  canvas.width = item.width;
  canvas.height = item.height;
  canvas.getContext('2d').putImageData(new ImageData(bytes, item.width, item.height), 0, 0);
  const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/png'));
  if (!blob) throw new Error('Не удалось закодировать изображение');
  await imageTransaction('readwrite', (store) => store.put({ blob, width: item.width, height: item.height }, item.id));
  return blob.size;
}

async function loadImage(id) {
  const db = await database();
  return new Promise((resolve, reject) => {
    const transaction = db.transaction(DB_STORE, 'readonly');
    const request = transaction.objectStore(DB_STORE).get(id);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    transaction.oncomplete = () => db.close();
  });
}

async function deleteImages(ids) {
  if (!ids.length) return;
  await imageTransaction('readwrite', (store) => ids.forEach((id) => store.delete(id)));
}

async function deleteFiles(paths) {
  if (!paths.length) return;
  await core.invoke('delete_history_assets', { paths });
}

function saveItems() {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(items));
    localStorage.removeItem('clip-history-items-v1');
  } catch (error) {
    console.error('Не удалось сохранить индекс истории:', error);
    localStorage.removeItem('clip-history-items-v1');
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(items)); }
    catch (retryError) { console.error('Повторная запись индекса истории тоже не удалась:', retryError); }
  }
}

async function pruneItems() {
  const previousItems = items;
  const oldIds = new Set(previousItems.map((item) => item.id));
  const cutoff = settings.maxAgeDays > 0 ? Date.now() - settings.maxAgeDays * 86400000 : 0;
  const maxBytes = settings.maxStorageMb > 0 ? settings.maxStorageMb * 1024 * 1024 : Infinity;
  let usedBytes = 0;
  items = [];
  for (const item of previousItems) {
    if (cutoff && item.copiedAt < cutoff) continue;
    if (items.length >= Number(settings.maxItems)) break;
    const size = Number(item.storageBytes ?? (item.kind === 'text' ? new TextEncoder().encode(item.text).length : item.kind === 'image' ? item.width * item.height * 4 : 0));
    if (usedBytes + size > maxBytes) continue;
    item.storageBytes = size;
    items.push(item);
    usedBytes += size;
  }
  const keepIds = new Set(items.map((item) => item.id));
  await deleteImages([...oldIds].filter((id) => !keepIds.has(id)));
  await deleteFiles(previousItems.filter((item) => !keepIds.has(item.id) && item.kind === 'files').flatMap((item) => item.paths));
  saveItems();
}

async function addItem(payload) {
  if (paused || !payload || !['text', 'image', 'files'].includes(payload.kind)) return;
  if (payload.kind === 'text' && !payload.text?.trim()) return;
  const existingIndex = items.findIndex((item) => item.id === payload.id);
  const item = existingIndex >= 0 ? items.splice(existingIndex, 1)[0] : { ...payload };
  if (!Number.isFinite(item.storageBytes)) {
    item.storageBytes = item.kind === 'text' ? new TextEncoder().encode(item.text).length : item.kind === 'image' ? item.width * item.height * 4 : 0;
  }
  item.copiedAt = Date.now();
  try {
    if (item.kind === 'image') {
      item.storageBytes = await saveImage({ ...item, bytes: payload.bytes ?? item.bytes });
      // The binary data lives in IndexedDB; keeping its byte array in localStorage
      // can exceed WebKit's small quota and prevent every later item from saving.
      delete item.bytes;
    }
  } catch (error) {
    console.error('Не удалось сохранить изображение:', error);
    return;
  }
  items.unshift(item);
  await pruneItems();
  render();
}

function timeLabel(timestamp) {
  const date = new Date(timestamp);
  if (date.toDateString() === new Date().toDateString()) return `Сегодня, ${date.toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' })}`;
  return date.toLocaleString('ru-RU', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
}

function makeEmpty(title, hint) {
  const empty = document.createElement('div');
  empty.className = 'empty-state';
  const icon = document.createElement('div');
  icon.className = 'empty-icon';
  icon.innerHTML = '<svg viewBox="0 0 24 24"><rect x="6" y="5" width="12" height="16" rx="2"/><path d="M9 5V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v1M9 10h6M9 14h6"/></svg>';
  const heading = document.createElement('strong'); heading.textContent = title;
  const detail = document.createElement('span'); detail.textContent = hint;
  empty.append(icon, heading, detail);
  return empty;
}

function render() {
  imageUrls.forEach(URL.revokeObjectURL);
  imageUrls = [];
  const query = searchInput.value.trim().toLocaleLowerCase('ru-RU');
  const visibleItems = items.filter((item) => {
    const haystack = item.kind === 'text' ? item.text : item.kind === 'files' ? item.paths.join(' ') : 'изображение картинка image';
    return haystack.toLocaleLowerCase('ru-RU').includes(query);
  });
  countLabel.textContent = query ? `Найдено · ${visibleItems.length}` : `Последнее · ${items.length}`;
  pauseButton.classList.toggle('active', paused);
  pauseButton.querySelector('span:last-child').textContent = paused ? 'Продолжить' : 'Пауза';
  historyElement.replaceChildren();
  if (!visibleItems.length) {
    historyElement.append(makeEmpty(query ? 'Ничего не найдено' : paused ? 'История на паузе' : 'Здесь пока пусто', query ? 'Попробуй другой запрос.' : paused ? 'Нажми «Продолжить», чтобы сохранять новые копирования.' : 'Скопируй текст, изображение или файл — он появится здесь.'));
    return;
  }

  for (const item of visibleItems) {
    const card = document.createElement('button');
    card.className = 'clip-card'; card.type = 'button';
    const content = document.createElement('span'); content.className = 'clip-content';
    const text = document.createElement('span'); text.className = 'clip-text';
    const mark = document.createElement('span'); mark.className = 'clip-mark';
    let preview;
    if (item.kind === 'text') {
      mark.textContent = 'T'; text.textContent = item.text; card.title = 'Вернуть текст в буфер обмена';
    } else if (item.kind === 'files') {
      mark.textContent = '↗';
      const names = item.paths.map((path) => path.split('/').pop());
      const list = document.createElement('span'); list.className = 'file-list';
      names.slice(0, 3).forEach((name) => { const chip = document.createElement('span'); chip.className = 'file-chip'; chip.textContent = name; list.append(chip); });
      if (names.length > 3) { const more = document.createElement('span'); more.className = 'file-chip'; more.textContent = `+${names.length - 3}`; list.append(more); }
      text.append(list); card.title = `Вернуть ${names.length} файл(ов) в буфер обмена`;
    } else {
      mark.textContent = '▧'; text.textContent = `Изображение · ${item.width} × ${item.height}`; card.title = 'Вернуть изображение в буфер обмена';
      loadImage(item.id).then((saved) => {
        if (!saved || !card.isConnected) return;
        const imageUrl = URL.createObjectURL(saved.blob); imageUrls.push(imageUrl);
        preview = document.createElement('img'); preview.className = 'thumb'; preview.src = imageUrl; preview.alt = 'Скопированное изображение';
        card.insertBefore(preview, content);
      }).catch((error) => console.error('Не удалось прочитать изображение из истории:', error));
    }
    const meta = document.createElement('span'); meta.className = 'clip-meta'; meta.textContent = timeLabel(item.copiedAt);
    content.append(text, meta);
    const hint = document.createElement('span'); hint.className = 'copy-hint'; hint.textContent = '↵  В буфер';
    card.append(mark, content, hint);
    card.addEventListener('click', async () => {
      try {
        if (item.kind === 'text') await clipboardManager.writeText(item.text);
        else if (item.kind === 'files') await core.invoke('write_clipboard_files', { paths: item.paths });
        else {
          const saved = await loadImage(item.id);
          if (!saved) throw new Error('Файл изображения отсутствует');
          const canvas = document.createElement('canvas'); canvas.width = saved.width; canvas.height = saved.height;
          const context = canvas.getContext('2d');
          const bitmap = await createImageBitmap(saved.blob);
          context.drawImage(bitmap, 0, 0); bitmap.close();
          const rgba = context.getImageData(0, 0, saved.width, saved.height).data;
          await core.invoke('write_clipboard_image', { width: saved.width, height: saved.height, bytes: Array.from(rgba) });
        }
        await core.invoke('hide_panel');
      } catch (error) { console.error('Не удалось вернуть элемент в буфер:', error); }
    });
    historyElement.append(card);
  }
}

function applySettings() {
  document.documentElement.dataset.theme = settings.theme;
  document.querySelector('#theme-setting').value = settings.theme;
  document.querySelector('#size-setting').value = settings.size;
  document.querySelector('#position-setting').value = settings.position;
  document.querySelector('#count-setting').value = String(settings.maxItems);
  document.querySelector('#age-setting').value = String(settings.maxAgeDays);
  document.querySelector('#storage-setting').value = String(settings.maxStorageMb);
  const [width, height] = SIZE_PRESETS[settings.size] ?? SIZE_PRESETS.standard;
  core.invoke('set_window_size', { width, height }).catch((error) => console.error('Не удалось изменить размер окна:', error));
  core.invoke('set_window_position', { position: settings.position }).catch((error) => console.error('Не удалось изменить положение окна:', error));
}

async function updateSettings(changes) {
  settings = { ...settings, ...changes };
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  applySettings();
  await pruneItems();
  render();
}

document.querySelectorAll('.tab').forEach((tab) => tab.addEventListener('click', () => {
  document.querySelectorAll('.tab').forEach((item) => item.classList.toggle('active', item === tab));
  document.querySelectorAll('.view').forEach((view) => view.classList.toggle('active', view.id === tab.dataset.view));
  document.querySelector('#search-box').hidden = tab.dataset.view !== 'history-view';
}));
searchInput.addEventListener('input', render);
searchInput.addEventListener('keydown', (event) => { if (event.key === 'Escape') core.invoke('hide_panel'); if (event.key === 'Enter') historyElement.querySelector('.clip-card')?.click(); });
document.querySelector('#close').addEventListener('click', () => core.invoke('hide_panel'));
document.querySelector('#clear').addEventListener('click', async () => {
  await deleteImages(items.filter((item) => item.kind === 'image').map((item) => item.id));
  await deleteFiles(items.filter((item) => item.kind === 'files').flatMap((item) => item.paths));
  items = []; saveItems(); render();
});
pauseButton.addEventListener('click', async () => { paused = !paused; localStorage.setItem(PAUSED_KEY, String(paused)); await core.invoke('set_monitor_paused', { paused }); render(); });
document.querySelector('#theme-setting').addEventListener('change', (event) => updateSettings({ theme: event.target.value }));
document.querySelector('#size-setting').addEventListener('change', (event) => updateSettings({ size: event.target.value }));
document.querySelector('#position-setting').addEventListener('change', (event) => updateSettings({ position: event.target.value }));
document.querySelector('#count-setting').addEventListener('change', (event) => updateSettings({ maxItems: Number(event.target.value) }));
document.querySelector('#age-setting').addEventListener('change', (event) => updateSettings({ maxAgeDays: Number(event.target.value) }));
document.querySelector('#storage-setting').addEventListener('change', (event) => updateSettings({ maxStorageMb: Number(event.target.value) }));
document.querySelector('#autostart-setting').addEventListener('change', async (event) => {
  event.target.disabled = true; settingsStatus.textContent = 'Применяю настройку…';
  try { await core.invoke('set_autostart', { enabled: event.target.checked }); settingsStatus.textContent = event.target.checked ? 'Приложение будет запускаться при входе в macOS.' : 'Автозапуск выключен.'; }
  catch (error) { event.target.checked = !event.target.checked; settingsStatus.textContent = `Не удалось изменить автозапуск: ${error}`; }
  finally { event.target.disabled = false; }
});
document.addEventListener('keydown', (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'f') { event.preventDefault(); document.querySelector('[data-view="history-view"]').click(); searchInput.focus(); }
  else if (event.key === 'Escape') core.invoke('hide_panel');
});
event.listen('clipboard-item', (event) => addItem(event.payload).catch((error) => console.error('Не удалось добавить запись в историю:', error)));
event.listen('panel-opened', () => window.setTimeout(() => searchInput.focus(), 30));

async function initialize() {
  applySettings();
  await pruneItems();
  try { document.querySelector('#autostart-setting').checked = await core.invoke('get_autostart'); }
  catch (error) { settingsStatus.textContent = `Статус автозапуска недоступен: ${error}`; }
  await core.invoke('set_monitor_paused', { paused }).catch((error) => console.error('Не удалось настроить мониторинг буфера:', error));
  render();
}
initialize();
