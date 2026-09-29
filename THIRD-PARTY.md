# 第三方组件与许可

本仓库**自身的代码**以 MIT 协议发布，见 [LICENSE](LICENSE)。
但运行时依赖的 FFmpeg **不是 MIT**，打包分发绿色版时必须单独满足它的许可要求。
这份文件说明用了什么、为什么是那个协议、以及分发时要做到什么。

> 以下是针对 GPLv3 条款的工程层面整理，不构成法律意见。对外正式分发前请自行确认。

## FFmpeg

| 项 | 内容 |
| --- | --- |
| 组件 | `ffmpeg.exe`、`ffprobe.exe` |
| 来源 | [gyan.dev](https://www.gyan.dev/ffmpeg/builds/) 的 Windows 静态构建 |
| 开发期使用的版本 | `6.1.1-essentials_build-www.gyan.dev` |
| 许可证 | **GPLv3** —— 全文见 [licenses/GPL-3.0.txt](licenses/GPL-3.0.txt) |
| 版权 | 归 FFmpeg 开发者所有 · <https://ffmpeg.org/> |
| 对应源码 | <https://github.com/FFmpeg/FFmpeg>；开发期版本对应 Git 标签 `n6.1.1` |
| 源码获取入口 | <https://ffmpeg.org/download.html#get-sources> |

### 为什么是 GPL 而不是 LGPL

FFmpeg 核心本身是 LGPLv2.1+，但它包含若干可选部分；一旦启用 `--enable-gpl`，
**整个 FFmpeg 就转为 GPL**。`libx264`、`libx265`、`libxvid` 都属于这类，
而 gyan.dev 的构建必然启用它们（否则就没有 H.264 / H.265 编码能力）——
所以这里没有 LGPL 的余地，gyan.dev 页面也明确标注 "licensed as GPLv3"。

想要 LGPL 版本，只能自己编译一份不含这些编码器的 FFmpeg，
那样也就失去 H.264 / H.265 编码能力了，对这个项目没有意义。

### 为什么 MIT 代码和 GPL 组件可以并存

本项目通过**命令行子进程**调用 `ffmpeg.exe`，没有链接任何 `libav*` 库，
两者是**聚合（aggregate）**关系而非衍生作品。因此：

- 本仓库自己的代码（Vue 界面 + Rust 服务）继续以 MIT 发布；
- FFmpeg 二进制部分仍受 GPLv3 约束，分发时两套条款各自的义务都要履行。

### 分发绿色版时的义务

GPLv3 对分发**二进制**的要求（§4–§6），落到这个项目上至少是：

1. **附上许可证全文** —— 打包目录中必须有 `licenses/GPL-3.0.txt`。
2. **给出对应源码的获取方式** —— 说明用的是哪个 FFmpeg 版本（含 commit）、去哪里拿。
   上游压缩包里的 `README.txt` 正好带这些信息（版本号、源码 commit、完整 configure 参数）：
   [`scripts/fetch-ffmpeg.ps1`](scripts/fetch-ffmpeg.ps1) 会把它一并放进 `server/binaries/`，
   [`scripts/portable.cjs`](scripts/portable.cjs) 再把它拷进绿色版目录。
3. **保留版权与许可声明** —— 不要删掉 `THIRD-PARTY.md`、`licenses/`
   以及 `binaries/` 里的说明文件。
4. **不得附加额外限制** —— 不能对 FFmpeg 部分施加比 GPLv3 更严格的条款，
   也不能声称对其拥有所有权。

### 商标与专利

- **FFmpeg 是 Fabrice Bellard 的商标。** 书写时请保持原样（两个大写 `F`、小写 `mpeg`），
  也不要暗示 FFmpeg 官方为本项目背书。
- 专利与许可证是两回事：H.264 / H.265 等格式可能涉及第三方专利，
  是否需要在特定司法辖区取得授权，由分发者自行评估。本项目不提供任何担保。

## 仓库里为什么没有这些二进制

`ffmpeg.exe` + `ffprobe.exe` 合计约 140 MB，放进 Git 会让克隆变得很重，
也不方便单独升级引擎。所以仓库只保存获取脚本，二进制按需下载：

```powershell
pwsh scripts/fetch-ffmpeg.ps1
```
