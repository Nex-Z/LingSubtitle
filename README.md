# 灵幕 (LingSubtitle)

灵幕是一款基于系统音频的实时双语字幕桌面应用，优先面向 Windows 场景。它可以捕获系统正在播放的声音，或只捕获某个指定应用的音频，再通过阿里云 Gummy 实时输出原文与译文，并在主界面与悬浮窗中同步显示。

当前仓库已经具备一条可用的完整链路：

`系统/应用音频捕获 -> PCM 16kHz 单声道处理 -> Gummy 实时识别/翻译 -> 主界面字幕流 -> 悬浮字幕窗 -> 会话文本保存`

## 当前状态

- 当前版本：`0.1.0`
- 桌面框架：`Tauri 2`
- 前端：`React 19 + TypeScript + Vite`
- 后端：`Rust`
- 优先平台：`Windows 10 1903+ / Windows 11`
- 开发状态：持续迭代中，核心实时字幕能力已打通

## 已实现能力

### 音频捕获

- 支持系统全局音频捕获（WASAPI Loopback）
- 支持指定应用音频捕获（Windows 进程级 loopback）
- 可从运行中的音频应用列表中选择目标进程
- 录制链路会将音频统一处理为 `PCM 16-bit / 16kHz / 单声道`

### 实时识别与翻译

- 已接入阿里云 `gummy-realtime-v1`
- 识别与翻译共用一条实时任务流，不再拆分为两段式链路
- 支持自动识别或手动指定识别语种
- 支持基于 Gummy 语言矩阵约束的目标语言选择
- 翻译开启时会校验语言对，避免提交无效任务
- 支持 VAD 句尾静音阈值配置
- 支持热词表 ID 配置
- 支持过滤独立出现的语气词

### 字幕展示

- 主界面实时显示字幕流
- 翻译开启时优先展示译文，保留原文参考
- 支持独立悬浮字幕窗口
- 悬浮窗支持字号、透明度、是否显示原文等设置
- 悬浮窗设置保存在本地 `localStorage`

### 保存与配置

- 支持按录制会话自动保存字幕
- 默认保存到用户文档目录下的 `LingSubtitle` 文件夹
- 保存文件名格式为 `字幕_YYYY-MM-DD_HH-mm-ss.txt`
- 应用当前支持保存以下配置：
  - Gummy WebSocket 地址
  - API Key
  - 模型名称
  - 识别语言
  - 句尾静音阈值 / VAD 预设
  - 热词表 ID
  - 自动保存开关
  - 保存路径

## 技术栈

| 层级 | 技术 |
| --- | --- |
| 桌面容器 | Tauri 2 |
| 前端 UI | React 19 + TypeScript + Vite |
| 桌面后端 | Rust |
| 音频采集 | WASAPI Loopback / Process Loopback |
| 实时服务 | 阿里云 Gummy WebSocket |
| 数据持久化 | 本地 `config.json` + 字幕 txt 文件 |

## 运行环境

建议在以下环境开发和运行：

- Windows 10 1903+ 或 Windows 11
- Node.js LTS
- Rust stable
- Tauri 2 所需的 Windows 构建环境
- 可用的阿里云 Gummy API Key

说明：

- 指定应用音频捕获依赖 Windows 的进程级 loopback 能力
- 目前仓库以 Windows 为主，尚未完成 macOS 适配验证

## 快速开始

### 1. 安装依赖

```bash
npm install
```

### 2. 启动开发环境

```bash
npm run tauri dev
```

### 3. 首次启动后配置

进入应用的“设置中心”，至少补齐以下项目：

- `WebSocket 地址`
- `API Key`
- `模型名称`，默认可使用 `gummy-realtime-v1`

如果你只做识别，源语言可保留为 `auto`。

如果你要启用翻译，请注意：

- 源语言不能为 `auto`
- 目标语言必须在 Gummy 支持的语言对矩阵内

### 4. 产物构建

```bash
npm run build
npm run tauri build
```

## 使用说明

### 录制流程

1. 在首页选择捕获模式：`系统音频` 或 `指定应用`
2. 若选择指定应用，先从运行中的音频应用列表中选定目标进程
3. 选择识别语言
4. 按需开启翻译，并选择目标语言
5. 开始录制后，字幕会实时刷新到主界面
6. 如需悬浮字幕，可额外打开悬浮窗

### 字幕保存格式

自动保存时，每条最终字幕会写入当前会话文件，格式类似：

```text
[14:30:15] Hello, welcome to the meeting.
[14:30:15] 你好，欢迎参加会议。

[14:30:22] Let's start with today's agenda.
[14:30:22] 让我们从今天的议程开始。
```

## 关键配置项

| 配置项 | 说明 |
| --- | --- |
| `asr.base_url` | Gummy WebSocket 地址，默认是阿里云官方实时入口 |
| `asr.api_key` | Gummy 调用凭证 |
| `asr.model` | 实时识别模型，当前默认 `gummy-realtime-v1` |
| `asr.language` | 识别语种，支持 `auto` 和多种明确语种 |
| `asr.vad_silence_ms` | 句尾静音阈值 |
| `asr.vocabulary_id` | 热词表 ID，可留空 |
| `translation.enabled` | 是否开启实时翻译 |
| `translation.target_language` | 翻译目标语种 |
| `save.auto_save` | 是否自动保存字幕文件 |
| `save.save_path` | 字幕文件保存目录 |
| `capture.source` | 捕获模式，`system` 或 `app` |
| `capture.app_pid` | 指定应用捕获时的目标进程 ID |
| `filter_fillers` | 是否过滤独立语气词 |

应用配置会保存在 Tauri 应用数据目录下的 `config.json` 中。

## 项目结构

```text
LingSubtitle/
├── src/
│   ├── App.tsx
│   ├── floating.tsx
│   ├── components/
│   │   ├── SubtitleView.tsx
│   │   ├── FloatingSubtitle.tsx
│   │   └── Settings.tsx
│   └── types/
├── src-tauri/
│   ├── src/
│   │   ├── audio.rs
│   │   ├── asr.rs
│   │   ├── gummy.rs
│   │   ├── subtitle.rs
│   │   ├── config.rs
│   │   └── lib.rs
│   └── tauri.conf.json
├── floating.html
├── package.json
└── README.md
```

各模块职责简述：

- `src/components/SubtitleView.tsx`：主工作台，负责录制控制、语言选择、字幕流展示
- `src/components/FloatingSubtitle.tsx`：悬浮字幕窗口与展示策略
- `src/components/Settings.tsx`：识别与保存设置页
- `src-tauri/src/audio.rs`：系统音频与指定应用音频采集
- `src-tauri/src/asr.rs`：Gummy 实时任务与消息收发
- `src-tauri/src/gummy.rs`：语言能力矩阵与参数校验
- `src-tauri/src/subtitle.rs`：会话文件创建与字幕落盘
- `src-tauri/src/config.rs`：本地配置读写与迁移
- `src-tauri/src/lib.rs`：整体状态机与 Tauri command 入口

## 当前限制

- 目前仍以 Windows 为主，未完成 macOS 适配
- 翻译开启时不能使用自动识别源语言
- 历史记录页面尚未落地，当前主要提供会话文本保存
- 离线识别/离线翻译尚未接入
- 长时间稳定性和安装包分发还需要继续打磨

## 后续计划

- 完善历史记录浏览与管理
- 优化长时间录制稳定性与错误恢复
- 增加系统托盘、快捷键与启动项能力
- 补齐安装包、签名与发布流程
- 评估离线语音识别与离线翻译方案

## 仓库内参考资料

- [阿里云实时语音接入文档](./阿里云实时语音接入文档.md)
- [阿里云文本生成接口文档](./阿里云文本生成接口文档.md)

如果你在推进这个项目，推荐优先阅读 `src-tauri/src/lib.rs`、`src-tauri/src/audio.rs` 和 `src/components/SubtitleView.tsx`，这三处基本覆盖了核心链路。
