// 与本地转换服务通信的唯一入口。全部同源请求：开发时由 Vite 代理到 8765。
async function toJson(response) {
  const data = await response.json().catch(() => ({}));
  if (!response.ok) throw new Error(data.error || `请求失败（HTTP ${response.status}）`);
  return data;
}

const post = (url, body) =>
  fetch(url, {
    method: 'POST',
    headers: body === undefined ? undefined : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body)
  }).then(toJson);

export const api = {
  state: () => fetch('/api/state').then(toJson),
  settings: patch => post('/api/settings', patch),
  browse: path => fetch(`/api/browse${path ? `?path=${encodeURIComponent(path)}` : ''}`).then(toJson),
  scan: path => post('/api/scan', { path }),
  addJobs: payload => post('/api/jobs', payload),
  cancel: id => post(`/api/jobs/${encodeURIComponent(id)}/cancel`),
  clear: () => post('/api/jobs/clear'),
  open: (path, reveal = true) => post('/api/open', { path, reveal }),
  engineTest: (kind, path) => post('/api/engine/test', { kind, path: path || '' }),
  restart: () => post('/api/restart'),

  /// 浏览器拖进来的文件没有本地路径，先复制到本机临时目录；用 XHR 才能拿到进度。
  upload(files, onProgress) {
    return new Promise((resolve, reject) => {
      const form = new FormData();
      for (const file of files) form.append('files', file, file.name);
      const request = new XMLHttpRequest();
      request.open('POST', '/api/upload');
      request.upload.onprogress = event => {
        if (event.lengthComputable && onProgress) onProgress(event.loaded / event.total);
      };
      request.onerror = () => reject(new Error('上传失败：无法连接本机服务'));
      request.onload = () => {
        try {
          const data = JSON.parse(request.responseText || '{}');
          if (request.status >= 200 && request.status < 300) resolve(data);
          else reject(new Error(data.error || `上传失败（HTTP ${request.status}）`));
        } catch (err) {
          reject(new Error(`上传响应无法解析：${err.message}`));
        }
      };
      request.send(form);
    });
  },

  events(onQueue) {
    const source = new EventSource('/api/events');
    source.onmessage = event => {
      try {
        const payload = JSON.parse(event.data);
        if (payload.type === 'queue') onQueue(payload.jobs || []);
      } catch {
        /* 忽略无法解析的心跳 */
      }
    };
    return () => source.close();
  }
};
