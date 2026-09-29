# Lythara Convert

LytharaLab 出品的本地媒体转换工作台：**Vue 3 界面 + Rust 本地服务 + FFmpeg 引擎**（MIT 协议）。

浏览器里跑界面，本机跑一个只监听 `127.0.0.1` 的转换服务，媒体引擎是 FFmpeg。媒体文件全程只在本机处理，不出网，也不依赖任何云服务。不带 Electron / Tauri，不自带浏览器内核，不碰显卡驱动。

## 跑起来

仓库**不携带** FFmpeg 二进制（`ffmpeg.exe` + `ffprobe.exe` 合计约 140 MB，不适合放进 Git），克隆后先补一次引擎：

```powershell
git clone https://github.com/LytharaLab/LytharaConvert.git
cd lythara-convert

pwsh scripts/fetch-ffmpeg.ps1   # 下载 FFmpeg 到 server\binaries\（约 115 MB）

npm install                     # 前端依赖（Vue / Vite）
npm run dist                    # 构建前端 + 编译服务 + 整理成可拷走的绿色目录
```

只想开发调试：

```powershell
npm run dev      # Vite 起在 5173，接口代理到 8765，服务自动重启
npm start        # 只起转换服务并自动打开浏览器
npm test         # 集成测试（会真实调用 ffmpeg，需要引擎已就位）
npm run build    # 只构建前端
```

> **构建顺序有讲究**：前端产物 `dist\` 由 Rust 侧在编译期整目录内嵌进 exe（`include_dir!`），
> 该目录不存在会直接编译失败。所以 `npm run build` 必须排在 `cargo build` 之前 ——
> `npm run dev`、`npm run dist` 已经按正确顺序串好；单独调 `npm run build:server` 时请自行注意。

## 引擎从哪来

`ffmpeg` / `ffprobe` 的解析顺序（界面会显示最终生效的路径与来源，并带「检测」按钮实际跑一次 `-version`）：

1. 设置里的 `ffmpegPath` / `ffprobePath`（自定义路径）
2. 环境变量 `LYTHARA_FFMPEG` / `LYTHARA_FFPROBE`
3. exe 旁的 `binaries\` —— **`fetch-ffmpeg.ps1` 就是往这里放**
4. exe 同级目录
5. （仅开发构建）`server\binaries\`
6. `PATH`

一个都找不到也不会崩：界面明确显示「FFmpeg 未就绪、转换不可用」，不会假装能用。

`scripts/fetch-ffmpeg.ps1` 默认取 [gyan.dev](https://www.gyan.dev/ffmpeg/builds/) 的最新 release essentials 构建（GPLv3、64 位静态）。要锁版本、走内网或校验，见 `Get-Help .\scripts\fetch-ffmpeg.ps1 -Full`：

```powershell
# 用已经下好的压缩包（离线 / 内网）
pwsh scripts/fetch-ffmpeg.ps1 -From D:\downloads\ffmpeg-release-essentials.zip

# 锁定版本 + 校验 SHA-256
pwsh scripts/fetch-ffmpeg.ps1 `
  -Url https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip `
  -Sha256 60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba
```

## 打绿色版

```powershell
npm run dist       # 构建前端 + 编译服务 + 整理
npm run portable   # 只重新整理已经编译好的产物
```

编译产物在 `server\target\release\`，`npm run portable` 会把要发的东西收拾进 `portable\Lythara Convert\`：

```text
lythara-convert.exe    约 1.6 MB，界面已内嵌在 exe 里
binaries\              ffmpeg.exe / ffprobe.exe，外加引擎的许可与构建说明
LICENSE / README.md / THIRD-PARTY.md / 使用说明.txt
licenses\GPL-3.0.txt   FFmpeg 的 GPLv3 全文（再分发时必须随附）
appdata\               首次运行自动生成：settings.json / uploads / tmp
```

**分发规则：把 `lythara-convert.exe` 和 `binaries\` 一起拷走即可**（拷到 U 盘、别的机器都行）。
目标机不需要装 Node、Rust 或 VC++ 运行库，也不依赖网络。
只拷 exe 也能启动、界面正常，但没有 `binaries\` 时会明确显示「FFmpeg 未就绪、转换不可用」。

`dist\` 放在 exe 旁边是**可选覆盖层**：想临时改界面，用它盖掉内嵌版本。

服务参数：`--no-open` 不自动打开浏览器；`--wait-for-port` 先等旧实例让出端口（重启时自动加）。
环境变量 `LYTHARA_PORT` 覆盖设置里的端口。端口被占用时会直接打开已有实例，等同于单实例。

## 配置

界面右上角齿轮打开偏好设置，全部选项都落在 **`<exe 同级>\appdata\settings.json`**
（也可以直接手改文件，重启服务生效）：

| 字段 | 默认 | 说明 |
| --- | --- | --- |
| `outputDir` | `%USERPROFILE%\Downloads\Lythara Convert` | 转换结果保存位置 |
| `theme` | `system` | `system` / `light` / `dark` |
| `port` | `8765` | 服务端口（1024–65535），**改动需重启服务** |
| `openBrowser` | `true` | 启动时自动打开浏览器 |
| `ffmpegPath` | `""` | 自定义 ffmpeg 可执行文件；留空 = 自动探测 exe 旁的 `binaries\` |
| `ffprobePath` | `""` | 自定义 ffprobe；同上 |
| `concurrency` | `1` | 同时处理的任务数（1–8） |
| `notifyOnFinish` | `false` | 任务结束后发浏览器通知 |
| `keepUploads` | `false` | 保留拖放上传到 `appdata\uploads\` 的原文件 |

字段缺失或非法一律退回默认值，所以老配置文件、手写的半截配置都能正常启动（端口低于 1024 会被掰回 8765，并发数会被夹到 1–8）。

exe 所在目录不可写时（比如装在 `Program Files` 下），数据目录会退回 `%APPDATA%\Lythara Convert`，
日志里会打印实际使用的路径。想恢复默认设置，删掉 `appdata\settings.json` 即可。

## 使用

1. 左侧选择格式转换、媒体压缩或 GIF 制作器。
2. **添加文件**：点「添加文件」用内置文件浏览器选本机文件（可多选），点「添加文件夹」整目录批量导入（最多 3000 项、10 层深）。也可以把文件直接拖进页面——浏览器拿不到本地路径，拖入的会先复制到系统临时目录再转换。
3. 选择输出格式、质量、目标大小和保存位置。点击按钮后，队列逐项开始处理，进度实时刷新。
4. 完成后点「查看文件」在资源管理器里定位结果，「打开输出文件夹」直达输出目录。

图片格式预设：PNG、JPG、WebP、AVIF、GIF、APNG、TIFF、BMP、ICO、HEIC、JXL。
视频格式预设：MP4、MKV、MOV、WebM、AVI、M4V、TS、FLV、WMV、3GP。
音频格式预设：MP3、WAV、FLAC、OGG、Opus、AAC、M4A、WMA、AIFF、ALAC。

也可以输入自定义扩展名，由 FFmpeg 尝试编码。**列表不是格式兼容性保证**：源文件必须可解码，目标容器必须有可用编码器，失败原因会显示在队列里。部分格式（如 HEIC / JXL）取决于所用 FFmpeg 构建的编译选项——essentials 构建**没有** HEIC / JXL 编码器，选它们会明确报错，不会产出假结果；需要这些编码器请自行换成 full 构建并指向自定义路径。

动图或视频转静态图片默认创建文件夹写入所有帧，可改为只取第一帧。多张图片合成 GIF 时可设每帧秒数、最大宽度、帧率与色彩质量；单张图片可制成循环 GIF。媒体压缩沿用原扩展名，新文件名加 `_compressed`，不覆盖原文件，重名自动编号。

目标 MB 为**尽量接近**：图片尝试质量与尺寸组合，GIF 迭代画幅/帧率/色数，视频按时长估算码率。无损格式无法靠质量参数无限缩小。

## 项目布局

```text
index.html              Vite 入口
vite.config.js          开发端口与 /api 代理
package.json            脚本与前端依赖
LICENSE                 本项目代码的 MIT 协议
THIRD-PARTY.md          第三方组件（FFmpeg）声明与分发义务
licenses/               第三方许可证全文（GPL-3.0.txt）
src/                    Vue 3 界面
  api.js                接口客户端（同源请求）
  store.js              全局状态与动作
  components/           侧边栏 / 顶栏 / 编辑区 / 队列 / 设置 / 文件浏览器
  assets/               样式（沿用旧版设计令牌）与图标
scripts/
  dev.cjs               一条命令拉起前端 + 服务
  portable.cjs          整理出可拷走的绿色目录
  fetch-ffmpeg.ps1      下载 FFmpeg 到 server\binaries\
  make_icon.py          生成 assets\ 下的图标（需要 Pillow + numpy）
server/                 Rust 本地转换服务
  Cargo.toml            依赖与 release 编译配置
  build.rs              把图标与版本信息写进 exe 资源段
  src/main.rs           进程入口
  src/lib.rs            路由、静态资源、启动与打开浏览器
  src/api.rs            HTTP 接口
  src/state.rs          队列与 SSE 广播
  src/converter.rs      FFmpeg 转换管线（转换 / 压缩 / GIF）
  src/catalog.rs        格式目录与编解码器映射
  src/settings.rs       设置读写
  tests/converter.rs    集成测试（真实跑 ffmpeg）
  binaries/             随附的 ffmpeg.exe / ffprobe.exe（不进版本库）
assets/                 图标源文件（scripts/make_icon.py 生成）
.cargo/config.toml      静态链接 C 运行库，目标机免装 VC++ 运行库
```

## 接口一览（只服务本机）

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| GET | `/api/state` | 队列、设置、格式表、引擎信息、当前端口 |
| GET | `/api/events` | SSE 队列推送 |
| POST | `/api/settings` | 改任意配置项（返回是否需重启） |
| GET | `/api/browse?path=` | 目录浏览（`path` 可为文件，会回传 `focus`） |
| POST | `/api/scan` | 文件夹展开成文件列表 |
| POST | `/api/jobs` | 加入转换任务 |
| POST | `/api/jobs/{id}/cancel` | 取消任务 |
| POST | `/api/jobs/clear` | 清除已结束任务 |
| POST | `/api/open` | 资源管理器定位/打开 |
| POST | `/api/upload` | 拖放上传（落到 `appdata\uploads`） |
| POST | `/api/engine/test` | 检测 ffmpeg / ffprobe 可执行文件 |
| POST | `/api/restart` | 用新配置重启服务 |

服务不发送任何 CORS 头，只绑定 `127.0.0.1`，因此外部网站无法读取接口内容。

## 说明

- 转换完全在本机进行，媒体文件不会上传到任何服务器（拖放的文件也只是复制到本机临时目录）。
- 队列串行执行，控制 CPU 与磁盘压力；每个结果先写临时文件再改名提交，取消/失败会清理临时输出。
- 输入为仅音频却要视频输出、输入无音轨却要音频输出，都会明确报错。
- 界面主题「跟随系统/浅色/深色」由设置钉住，存在浏览器 localStorage 里做首帧防闪。

## 许可

本仓库**自身的代码**（Vue 界面 + Rust 服务）以 [MIT](LICENSE) 协议发布，Copyright © 2026 LytharaLab。

但**运行时依赖的 FFmpeg 不是 MIT**：随附或另行下载的 `ffmpeg.exe` / `ffprobe.exe`
以 **GPLv3** 发布，版权归 FFmpeg 开发者所有（<https://ffmpeg.org/>）。

本项目通过命令行子进程调用 FFmpeg，没有链接任何 `libav*` 库，两者是聚合关系而非衍生作品，
所以 MIT 与 GPLv3 可以并存 —— 但分发打包好的绿色版时，**两套条款的义务都要履行**：

- 附上 GPLv3 全文：[licenses/GPL-3.0.txt](licenses/GPL-3.0.txt)
- 给出对应源码的版本与获取方式
- 保留版权与许可声明，且不得对 FFmpeg 部分附加额外限制

完整说明、义务清单以及商标与专利提示见 **[THIRD-PARTY.md](THIRD-PARTY.md)**。
上面这些都是工程层面的整理，不构成法律意见，正式对外分发前请自行确认。
