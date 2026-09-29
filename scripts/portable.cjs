// 整理出「拷到别的电脑就能用」的最小目录：
//   portable/Lythara Convert/
//     lythara-convert.exe   服务 + 界面（内嵌）
//     binaries/             ffmpeg / ffprobe
//     LICENSE               
//     使用说明.txt
const fs = require('node:fs');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const release = path.join(root, 'server', 'target', 'release');
const sourceBinaries = path.join(root, 'server', 'binaries');
const out = path.join(root, 'portable', 'Lythara Convert');
const exe = path.join(release, 'lythara-convert.exe');

if (!fs.existsSync(exe)) {
  console.error(`没有找到 ${exe}\n先执行：npm run build:server`);
  process.exit(1);
}

fs.rmSync(out, { recursive: true, force: true });
fs.mkdirSync(path.join(out, 'binaries'), { recursive: true });

fs.copyFileSync(exe, path.join(out, 'lythara-convert.exe'));
let binaries = 0;
for (const name of fs.readdirSync(sourceBinaries)) {
  fs.copyFileSync(path.join(sourceBinaries, name), path.join(out, 'binaries', name));
  binaries += 1;
}
for (const extra of ['LICENSE', 'README.md']) {
  const from = path.join(root, extra);
  if (fs.existsSync(from)) fs.copyFileSync(from, path.join(out, extra));
}

fs.writeFileSync(path.join(out, '使用说明.txt'), [
  'Lythara Convert · 本地媒体转换工作台',
  '',
  '【怎么用】双击 lythara-convert.exe，浏览器会自动打开 http://127.0.0.1:8765/',
  '           这个黑窗口是服务本体，关掉它服务就停了。',
  '',
  '【需要什么】任意现代浏览器（Edge / Chrome 都行）。',
  '            不必安装 Node、Rust、VC++ 运行库，也不依赖网络。',
  '            媒体文件全程只在本机处理。',
  '',
  '【数据在哪】appdata\\ 目录（首次运行自动生成，就在 exe 旁边）：',
  '            settings.json  输出目录与主题设置',
  '            uploads\\       拖放上传的临时文件',
  '            tmp\\           转换中间文件，用完自动清理',
  '            想恢复默认设置，删掉 appdata\\settings.json 即可。',
  '',
  '【换端口】LYTHARA_PORT=9000 lythara-convert.exe',
  '【不开浏览器】lythara-convert.exe --no-open',
  '',
  '【首次运行被拦】Windows 可能提示「未知发布者」，点「更多信息 → 仍要运行」。',
  '',
  '【转换引擎】binaries\\ffmpeg.exe 与 ffprobe.exe，来自 FFmpeg（GPL v3 构建），',
  '            随本软件一起分发时请遵守其许可证；详见 LICENSE 与 FFmpeg 官网。',
  '',
  'Copyright © 2026 LytharaLab. MIT License.'
].join('\r\n'), 'utf8');

const size = dir => {
  let total = 0;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    total += entry.isDirectory() ? size(full) : fs.statSync(full).size;
  }
  return total;
};
const mb = value => (value / 1048576).toFixed(1);

console.log('可拷贝目录已生成：');
for (const entry of fs.readdirSync(out, { withFileTypes: true })) {
  const full = path.join(out, entry.name);
  console.log(`  ${entry.isDirectory() ? entry.name + '/' : entry.name}  ${mb(entry.isDirectory() ? size(full) : fs.statSync(full).size)} MB`);
}
console.log(`合计 ${mb(size(out))} MB（其中引擎 ${binaries} 个文件）`);
console.log(`位置：${out}`);
