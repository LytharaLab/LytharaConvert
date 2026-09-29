// 全局状态与动作：界面组件只读这里的 state，交互一律走这里的函数。
import { computed, reactive } from 'vue';
import { api } from './api.js';

const THEME_KEY = 'lythara-theme';
const GIF_IMAGE_EXT = ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'tiff', 'tif', 'avif', 'heic'];
const UPLOAD_WARN_BYTES = 2 * 1024 * 1024 * 1024;

export const modes = {
  convert: {
    title: '让格式转换，<br/><em>回归简单。</em>',
    desc: '图片、视频、音频，都在一个工作台。<br/>选择文件，设定输出，其余交给我们。',
    label: '格式转换',
    action: '添加到队列',
    tip: '把 GIF 转为 PNG，会自动将每一帧保存到独立文件夹。'
  },
  compress: {
    title: '让文件轻一点，<br/><em>精彩不减。</em>',
    desc: '压缩图片、GIF、视频和音频。<br/>设定目标大小，找到质量与体积的平衡。',
    label: '媒体压缩',
    action: '开始压缩',
    tip: '目标大小是近似值，复杂画面与编码器会影响最终体积。'
  },
  gif: {
    title: '把每一帧，<br/><em>变成故事。</em>',
    desc: '按顺序加入一张或多张图片。<br/>帧率、画幅、时长，都由你决定。',
    label: 'GIF 制作器',
    action: '制作 GIF',
    tip: 'GIF 制作器支持单图循环，也支持多图按指定顺序播放。'
  }
};

export const state = reactive({
  ready: false,
  mode: 'convert',
  category: 'image',
  selected: [],
  jobs: [],
  formats: {},
  settings: {
    outputDir: '',
    theme: 'system',
    port: 8765,
    openBrowser: true,
    ffmpegPath: '',
    ffprobePath: '',
    concurrency: 1,
    notifyOnFinish: false,
    keepUploads: false
  },
  engine: { ready: false, probeReady: false, ffmpeg: '', ffprobe: '', ffmpegSource: '', ffprobeSource: '' },
  port: 8765,
  restartRequired: false,
  restarting: false,
  engineTest: { kind: '', running: false, ok: null, message: '' },
  dragging: false,
  settingsOpen: false,
  toast: { message: '', visible: false },
  transfer: { active: false, note: '', progress: 0 },
  options: {
    format: 'png',
    customFormat: '',
    compression: true,
    quality: 78,
    targetMb: '',
    frames: 'all',
    gifFps: 12,
    frameSeconds: 0.25,
    maxWidth: 960,
    singleDuration: 3
  },
  browser: {
    open: false,
    purpose: 'files',
    path: '',
    draft: '',
    parent: null,
    roots: [],
    entries: [],
    picked: [],
    loading: false,
    error: ''
  }
});

export const currentFormats = computed(() => state.formats[state.category] || []);
export const config = computed(() => modes[state.mode]);
export const pendingCount = computed(
  () => state.jobs.filter(job => job.status === 'pending' || job.status === 'running').length
);

// ── 小工具 ────────────────────────────────────────────────────────────────

export const displayName = path => String(path).replaceAll('\\', '/').split('/').pop();
export const extOf = path => displayName(path).split('.').pop().toLowerCase();
export const hasExtension = path => displayName(path).includes('.');

export function size(bytes) {
  if (bytes == null) return '—';
  if (bytes >= 1048576) return `${(bytes / 1048576).toFixed(1)} MB`;
  return `${Math.max(1, bytes / 1024).toFixed(0)} KB`;
}

let toastTimer;
export function toast(message) {
  state.toast.message = message;
  state.toast.visible = true;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => { state.toast.visible = false; }, 4200);
}

function reportError(err) {
  toast(err?.message ? err.message : String(err));
}

// ── 主题 ──────────────────────────────────────────────────────────────────

const systemDark = () => matchMedia('(prefers-color-scheme: dark)').matches;

export function applyTheme(mode) {
  const resolved = !mode || mode === 'system' ? (systemDark() ? 'dark' : 'light') : mode;
  document.documentElement.dataset.theme = resolved;
  try { localStorage.setItem(THEME_KEY, mode || 'system'); } catch { /* 隐私模式下忽略 */ }
}

export const effectiveTheme = () => document.documentElement.dataset.theme;

export async function setTheme(mode) {
  // 先乐观更新：按钮立刻跟上用户的选择，不等服务端往返。
  state.settings = { ...state.settings, theme: mode };
  applyTheme(mode);
  const ok = await updateSettings({ theme: mode });
  if (!ok) toast('主题已切换，但保存失败');
}

export function toggleTheme() {
  return setTheme(effectiveTheme() === 'dark' ? 'light' : 'dark');
}

// ── 启动与队列 ────────────────────────────────────────────────────────────

export async function boot() {
  try {
    const data = await api.state();
    state.jobs = data.jobs || [];
    state.formats = data.formats || {};
    state.settings = { ...state.settings, ...(data.settings || {}) };
    state.engine = data.engine || state.engine;
    state.port = data.port || state.settings.port;
    state.category = state.formats.image ? 'image' : state.category;
    state.options.format = (state.formats[state.category] || [])[0] || 'png';
    applyTheme(state.settings.theme);
    if (!state.engine.ready) toast('未找到 FFmpeg，请在偏好设置里指定引擎路径');
    state.ready = true;
  } catch (err) {
    reportError(err);
  }
}

/// 统一的设置写入：主题、目录、引擎、端口、并发都走这里。
export async function updateSettings(patch) {
  const previous = { ...state.settings };
  state.settings = { ...state.settings, ...patch };
  try {
    const data = await api.settings(patch);
    state.settings = { ...state.settings, ...(data.settings || {}) };
    state.engine = data.engine || state.engine;
    state.restartRequired = !!data.restart_required;
    if (patch.theme) applyTheme(state.settings.theme);
    return true;
  } catch (err) {
    state.settings = previous;
    reportError(err);
    return false;
  }
}

export async function testEngine(kind, path) {
  state.engineTest = { kind, running: true, ok: null, message: '' };
  try {
    const result = await api.engineTest(kind, path);
    state.engineTest = {
      kind,
      running: false,
      ok: result.ok,
      message: result.ok ? result.version || '可用' : result.error || '不可用'
    };
  } catch (err) {
    state.engineTest = { kind, running: false, ok: false, message: err.message };
  }
  return state.engineTest;
}

export async function applyEnginePaths() {
  const ok = await updateSettings({
    ffmpegPath: state.settings.ffmpegPath || '',
    ffprobePath: state.settings.ffprobePath || ''
  });
  if (ok) toast(state.engine.ready ? '引擎已更新' : '已保存，但该路径下的 FFmpeg 不可用');
}

export async function restartServer() {
  state.restarting = true;
  try {
    await api.restart();
  } catch {
    /* 服务正在退出，请求失败是正常的 */
  }
  // 轮询到新进程可用为止，然后整页刷新。
  for (let attempt = 0; attempt < 40; attempt += 1) {
    await new Promise(resolve => setTimeout(resolve, 400));
    try {
      await api.state();
      location.reload();
      return;
    } catch {
      /* 还没起来，继续等 */
    }
  }
  state.restarting = false;
  toast('服务重启超时，请手动启动 lythara-convert.exe');
}

export function subscribeQueue() {
  const previous = new Map(state.jobs.map(job => [job.id, job.status]));
  return api.events(jobs => {
    const finished = jobs.filter(job => {
      const before = previous.get(job.id);
      return before && before !== job.status && ['done', 'error'].includes(job.status);
    });
    for (const job of jobs) previous.set(job.id, job.status);
    state.jobs = jobs;
    if (!finished.length || !state.settings.notifyOnFinish) return;
    if (!('Notification' in window) || Notification.permission !== 'granted') return;
    const ok = finished.filter(job => job.status === 'done').length;
    const failed = finished.length - ok;
    new Notification('Lythara Convert', {
      body: `已完成 ${ok} 项${failed ? `，失败 ${failed} 项` : ''}`
    });
  });
}

export async function requestNotifyPermission(enabled) {
  if (!enabled || !('Notification' in window)) return;
  if (Notification.permission === 'default') {
    try { await Notification.requestPermission(); } catch { /* 用户拒绝就算了 */ }
  }
}

export async function setMountedCategory(category) {
  state.category = category;
  const list = state.formats[category] || [];
  if (list.length) state.options.format = list[0];
}

// ── 选择文件 ──────────────────────────────────────────────────────────────

export function addFiles(paths) {
  const values = (paths || []).filter(path => typeof path === 'string' && path && !state.selected.includes(path));
  if (!values.length) return;
  let kept = values;
  if (state.mode === 'gif') {
    kept = values.filter(path => GIF_IMAGE_EXT.includes(extOf(path)));
    if (kept.length !== values.length) toast('GIF 制作器只接受静态图片；已跳过其他文件');
  }
  state.selected.push(...kept);
}

export function setMode(mode) {
  state.mode = mode;
  if (mode === 'gif') state.selected = state.selected.filter(path => GIF_IMAGE_EXT.includes(extOf(path)));
}

export function removeSelected(index) {
  state.selected.splice(index, 1);
}

export function moveSelected(index, delta) {
  const target = index + delta;
  if (target < 0 || target >= state.selected.length) return;
  const list = state.selected;
  [list[index], list[target]] = [list[target], list[index]];
}

export function clearSelected() {
  state.selected = [];
}

// ── 任务 ──────────────────────────────────────────────────────────────────

export async function start() {
  if (!state.selected.length) return toast('请先添加文件');
  const options = state.options;
  let format = 'gif';
  if (state.mode === 'convert') {
    format = state.category === 'custom'
      ? String(options.customFormat || '').trim().replace(/^\./, '').toLowerCase()
      : options.format;
    if (!/^[a-z0-9]{1,12}$/.test(format)) return toast('请输入有效的目标格式扩展名');
  }
  if (state.mode === 'compress') format = '';
  const target = Number(options.targetMb);
  if (String(options.targetMb).trim() !== '' && (!Number.isFinite(target) || target <= 0)) {
    return toast('目标大小必须大于 0 MB');
  }
  const compression = !!options.compression || state.mode === 'compress';
  const payload = {
    files: state.selected.slice(),
    mode: state.mode,
    format,
    name: 'my_animation',
    options: {
      quality: compression ? Number(options.quality) : 100,
      targetMb: compression ? target : 0,
      frames: options.frames,
      gifFps: Number(options.gifFps),
      frameSeconds: Number(options.frameSeconds),
      maxWidth: Number(options.maxWidth),
      singleDuration: Number(options.singleDuration)
    }
  };
  try {
    state.jobs = await api.addJobs(payload);
    toast(state.mode === 'gif' ? 'GIF 制作任务已开始' : `已添加 ${state.selected.length} 个任务`);
    state.selected = [];
  } catch (err) {
    reportError(err);
  }
}

export async function cancelJob(id) {
  try { state.jobs = await api.cancel(id); } catch (err) { reportError(err); }
}

export async function clearFinished() {
  try { state.jobs = await api.clear(); } catch (err) { reportError(err); }
}

export async function openOutput(path) {
  try { await api.open(path || null, Boolean(path)); } catch (err) { reportError(err); }
}

// ── 浏览器拖放：文件没有本地路径，先上传到本机临时目录 ────────────────────

export async function uploadFiles(fileList) {
  const files = Array.from(fileList || []);
  if (!files.length) return;
  const total = files.reduce((sum, file) => sum + (file.size || 0), 0);
  if (total > UPLOAD_WARN_BYTES) {
    toast('文件较大，拖放会先复制一份到临时目录；建议改用「添加文件」直接选本地路径');
  }
  state.transfer = { active: true, note: `正在复制 ${files.length} 个文件到本机临时目录…`, progress: 0 };
  try {
    const result = await api.upload(files, ratio => {
      state.transfer.progress = Math.round(ratio * 100);
    });
    addFiles(result.files || []);
    toast(`已接收 ${(result.files || []).length} 个文件`);
  } catch (err) {
    reportError(err);
  } finally {
    state.transfer = { active: false, note: '', progress: 0 };
  }
}

// ── 服务端文件浏览 ────────────────────────────────────────────────────────

export async function openBrowser(purpose) {
  state.browser.open = true;
  state.browser.purpose = purpose;
  state.browser.picked = [];
  state.browser.error = '';
  await browseTo('');
}

export async function browseTo(path) {
  state.browser.loading = true;
  state.browser.error = '';
  try {
    const data = await api.browse(path || '');
    state.browser.path = data.path;
    state.browser.draft = data.path;
    state.browser.parent = data.parent;
    state.browser.roots = data.roots || [];
    state.browser.entries = data.entries || [];
    if (data.truncated) toast('目录项过多，仅显示前 2000 项');
    // 粘贴的是文件：自动勾选它。
    if (data.focus && state.browser.purpose === 'files' && !state.browser.picked.includes(data.focus)) {
      state.browser.picked.push(data.focus);
    }
  } catch (err) {
    state.browser.error = err.message;
    state.browser.entries = [];
    state.browser.path = path || state.browser.path;
  } finally {
    state.browser.loading = false;
  }
}

/// 地址栏回车：支持绝对路径、带引号的路径、正斜杠，也支持直接粘文件路径。
export function jumpTo(path) {
  const value = String(path ?? state.browser.draft ?? '').trim().replace(/^"|"$/g, '').trim();
  if (!value) return toast('请输入文件夹或文件的绝对路径');
  return browseTo(value.replace(/\//g, '\\'));
}

export function togglePick(entry) {
  if (entry.isDir) return;
  const index = state.browser.picked.indexOf(entry.path);
  if (index >= 0) state.browser.picked.splice(index, 1);
  else state.browser.picked.push(entry.path);
}

export function closeBrowser() {
  state.browser.open = false;
  state.browser.picked = [];
}

export async function confirmBrowser() {
  const browser = state.browser;
  if (browser.purpose === 'output') {
    const target = browser.path;
    // 同样先乐观更新，避免「界面没反应」的观感。
    state.settings = { ...state.settings, outputDir: target };
    if (await updateSettings({ outputDir: target })) {
      toast('保存位置已更新');
    }
    closeBrowser();
    return;
  }
  if (browser.purpose === 'folder') {
    try {
      state.transfer = { active: true, note: '正在读取文件夹…', progress: 0 };
      const files = await api.scan(browser.path);
      if (files.length >= 3000) toast('文件夹最多读取 3000 项');
      addFiles(files);
      toast(`从文件夹加入 ${files.length} 个文件`);
      closeBrowser();
    } catch (err) {
      reportError(err);
    } finally {
      state.transfer = { active: false, note: '', progress: 0 };
    }
    return;
  }
  if (!browser.picked.length) return toast('请先选择文件');
  addFiles(browser.picked);
  toast(`已加入 ${browser.picked.length} 个文件`);
  closeBrowser();
}
