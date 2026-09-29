// 一条命令拉起开发环境：本机转换服务（Rust）+ Vite 前端。
// 前端在 5173，接口经 Vite 代理转发到 8765。
const { spawn } = require('node:child_process');
const path = require('node:path');

const root = path.resolve(__dirname, '..');
const children = [];

function run(label, command, args, options = {}) {
  const child = spawn(command, args, { cwd: root, stdio: ['ignore', 'pipe', 'pipe'], ...options });
  const prefix = `[${label}] `;
  const pipe = stream => {
    stream.setEncoding('utf8');
    let buffer = '';
    stream.on('data', chunk => {
      buffer += chunk;
      const lines = buffer.split(/\r?\n/);
      buffer = lines.pop() ?? '';
      for (const line of lines) if (line.trim()) process.stdout.write(prefix + line + '\n');
    });
  };
  pipe(child.stdout);
  pipe(child.stderr);
  child.on('exit', code => {
    process.stdout.write(`${prefix}进程退出（code=${code}）\n`);
    shutdown(code ?? 0);
  });
  children.push(child);
  return child;
}

let closing = false;
function shutdown(code = 0) {
  if (closing) return;
  closing = true;
  for (const child of children) {
    if (!child.killed) {
      try { child.kill(); } catch { /* 已经退出 */ }
    }
  }
  setTimeout(() => process.exit(code), 300);
}

process.on('SIGINT', () => shutdown(0));
process.on('SIGTERM', () => shutdown(0));

console.log('启动 Lythara Convert 开发环境（Ctrl+C 结束）');
run('server', 'cargo', ['run', '--manifest-path', path.join('server', 'Cargo.toml'), '--', '--no-open']);
run('web', process.execPath, [path.join('node_modules', 'vite', 'bin', 'vite.js')]);
