# BiliCommentBot-Rust

B站评论自动回复机器人 **Rust + Tauri 桌面版**（Windows GUI）

基于 [BiliCommentBot (Python Web UI版)](https://github.com/Janson20/BiliCommentBot) 完整移植。

---

## 功能特性

- 🖥️ **原生 Windows GUI**：Rust + Tauri 1.x + Svelte 构建，轻量 < 8MB 安装包
- 🤖 自动监控 B 站视频的新增评论
- 🧠 **双 AI 引擎**：DeepSeek API + **Ollama 本地模型**，可自由切换
- 📝 **链式回复支持（楼中楼）**：递归获取并回复多层级子评论
- 🔑 **扫码登录**：B 站 APP 扫码获取 Cookie，支持手动输入 + 自动刷新
- 👍 回复后自动点赞评论 / 点赞用户最新视频（可选）
- 👥 仅给关注了你的用户视频点赞（可选）
- 🧹 **评论过滤**：关键词黑白名单、评论长度、用户 UID 黑白名单三重过滤，精准控制回复范围
- 🌐 **新手教程向导**：首次启动 5 步分步引导 — B站登录 → AI引擎 → 回复设置 → 安全设置 → 完成，也支持一键导入 Python 版所有配置和历史
- 📊 实时仪表盘：运行状态、已回复统计、最近日志，支持「立即检查」手动触发一轮评论扫描
- ⚙️ **配置热更新**：修改后立即生效无需重启
- 📜 实时日志查看 + 分级别过滤 + 搜索 + 导出
- 📋 回复历史记录查看 + 分页 + 清除（SQLite 存储，首次自动迁移旧 JSON 数据）
- 🔒 登录密码保护（启动锁屏，bcrypt 哈希，兼容旧版 SHA-256 自动升级）
- 🖥️ **系统托盘常驻**：关闭窗口时可选择「最小化到托盘」让机器人继续后台运行，托盘菜单可显示窗口 / 退出程序
- 🚀 **开机自启**：一键写入注册表 Run 项，开机后静默启动到托盘（不弹窗）。配置完整（已登录 + 已配好 AI）时**机器人会自动开始工作**，无需手动点「启动」
- 📁 **数据统一存放**：配置、历史、Cookie、缓存、日志都在 `%APPDATA%\BiliCommentBot-RS`，升级/重装不丢；旧版本遗留在程序目录的数据会在首次启动时自动迁移
- 📝 **日志落盘**：按 `[logging]` 写入 `logs/bot.log`，超过 5MB 自动轮转（正式版没有控制台窗口，日志文件是唯一的排查手段）
- 🔂 **请求重试**：所有 B站 请求与 AI 调用按 `[rate_limit]` 的 `max_retries` / `retry_delay` 做指数退避重试，并遵守 `Retry-After`
- 🔒 **单实例保护**：重复启动会提示并退出，避免两个实例抢同一份历史库而重复回复
- 🔑 **安全的凭证写入**：手动填写的 Cookie 会**先校验再保存**，校验失败时完整保留原有登录
- 🗑️ **一键清空**：只清理数据目录内已知的运行时文件（会先把清单展示给你），需输入确认文字
- 🐳 体积轻量：前端 ~44KB gzipped，Rust 后端无运行时

---

## 快速开始

### Windows 安装

1. 下载最新 `.msi` 安装包：[Releases](https://github.com/Janson20/BiliCommentBot-Rust/releases)
2. 双击安装，桌面自动创建快捷方式
3. 首次启动自动弹出**新手教程**，分步完成配置

### 新手教程流程

首次启动时会自动弹出向导。整个流程只需 **2~3 分钟**：

| 步骤 | 内容 | 可跳过 |
|------|------|--------|
| 1. 登录 B 站 | B站 APP 扫码 或 手动输入 Cookie，自动验证 | 否 |
| 2. 选择 AI 引擎 | DeepSeek（云端，需 API Key）或 Ollama（本地，可自动检测） | 否 |
| 3. 回复设置 | 前缀、每次处理数、楼中楼、点赞等 | 是 |
| 4. 安全设置 | 设置访问密码保护（SHA-256） | 是 |
| 5. 完成 | 配置摘要确认 → 进入仪表盘 | — |

> 向导完成后，可随时在侧边栏「配置」页面调整所有设置，「设置」页面可修改密码和检测 Ollama。

### 从 Python 版迁移

1. 在新手教程首页选择「📂 从旧版迁移」
2. 选择 Python 版 `BiliCommentBot` 项目文件夹（包含 `config.toml`、`history.json`、`bilibili_cookie.json`、`video_cache.json`）
3. 一键导入所有配置和历史数据，无需重新扫码

### 本地编译（开发者）

**环境要求：**
- Rust >= 1.75（安装 `rustup`）
- Node.js >= 18
- Windows SDK（Visual Studio Build Tools）

```bash
# 安装依赖
npm install

# 开发模式（热更新）
npm run tauri dev

# 生产构建
npm run tauri build
# 输出:
#   MSI 安装包:  src-tauri/target/release/bundle/msi/
#   NSIS 安装包: src-tauri/target/release/bundle/nsis/
```

### 发版流程（自动化 CI/CD）

通过 `release.py` 脚本一键发版，自动同步版本号、提交、打 tag 并推送触发 GitHub Action 构建和发布 Release。

```bash
# 预览模式（不执行任何操作）
python release.py x.x.x --dry-run

# 正式发版（同步版本号 → git commit → git tag → git push）
python release.py x.x.x
```

**自动化流程：**
1. `release.py` 校验版本号、检查工作区状态
2. 自动同步更新 `package.json` / `tauri.conf.json` / `Cargo.toml` 中的版本号
3. 提交 `chore: bump version to x.x.x` 并推送 `vx.x.x` tag
4. GitHub Action 监听 `v*` tag 自动触发：
   - 构建 Svelte 前端 + Rust 后端
   - 生成 `.msi` + NSIS `.exe` 安装包
   - 按[约定式提交](https://www.conventionalcommits.org/zh-hans/)自动生成 Changelog
   - 创建 GitHub Release 并上传安装包

---

## 项目结构

```
BiliCommentBot-RS/
├── src/                    # Svelte 前端
│   ├── App.svelte          # 主入口 + 路由
│   ├── main.js
│   ├── lib/
│   │   ├── api.js          # Tauri invoke 封装
│   │   └── stores.js       # Svelte stores 共享状态
│   ├── components/
│   │   ├── Sidebar.svelte  # 导航侧边栏
│   │   └── Toast.svelte    # 通知提示
│   └── pages/
│       ├── Dashboard.svelte # 仪表盘
│       ├── Wizard.svelte    # 新手迁移向导
│       ├── Login.svelte     # 扫码登录
│       ├── Config.svelte    # 配置编辑器
│       ├── Logs.svelte      # 日志查看
│       ├── History.svelte   # 回复历史
│       └── Settings.svelte  # 系统设置
├── src-tauri/              # Rust 后端
│   ├── src/
│   │   ├── main.rs         # Tauri 入口 + BotState 初始化 + 启动即运行
│   │   ├── commands.rs     # Tauri 命令（前端唯一交互面）
│   │   ├── paths.rs        # 数据目录解析 + 旧数据自动迁移
│   │   ├── logger.rs       # 文件日志（级别热更新 + 5MB 轮转）
│   │   ├── single_instance.rs # 单实例保护（文件锁，崩溃自动释放）
│   │   ├── desktop.rs      # 系统托盘 + 关闭窗口行为 + 退出流程
│   │   ├── autostart.rs    # 开机自启（注册表 Run 项）
│   │   ├── bot.rs          # 机器人主循环编排
│   │   ├── config.rs       # TOML 配置管理
│   │   ├── cookie.rs       # Cookie 扫码/刷新/验证
│   │   ├── app_sign.rs     # BiliDroid 签名算法
│   │   ├── bvid.rs         # BVID ↔ AID 转换
│   │   ├── http_client.rs  # UA 轮换 + APP 参数
│   │   ├── rate_limiter.rs # 频率控制 + 重试策略（RetryPolicy）
│   │   ├── video_fetcher.rs # 视频列表获取
│   │   ├── comment_fetcher.rs # 评论+楼中楼获取
│   │   ├── deepseek.rs     # DeepSeek API
│   │   ├── ollama.rs       # Ollama 本地模型
│   │   ├── reply.rs        # 回复/点赞/粉丝检查
│   │   └── history.rs      # 历史记录管理（SQLite）
│   ├── config.example.toml # 配置字段参考（可提交，不含凭证）
│   ├── .cargo/config.toml  # 开发/测试的数据目录隔离
│   ├── Cargo.toml
│   └── tauri.conf.json
├── .github/workflows/
│   ├── ci.yml              # push/PR：clippy -D warnings + 测试 + 前端构建
│   └── release.yml         # tag：构建并发布安装包
├── package.json
├── index.html
└── .gitignore
```

---

## 配置说明

配置文件 `config.toml` 位于**用户数据目录**（见下节），格式**完全兼容** Python 版，可直接迁移。
完整字段说明与默认值见 [`src-tauri/config.example.toml`](src-tauri/config.example.toml)。

```toml
[bilibili]
cookie = ""            # B站 Cookie 或留空扫码
uid = ""               # 你的 B站 UID（登录后自动填充）
check_interval = 60    # 检查间隔（秒）
cookie_refresh_interval = 30   # Cookie 状态检查的最小间隔（分钟）

[rate_limit]
min_request_interval = 2.0     # 最小请求间隔（秒）
max_retries = 3                # 单个请求失败后的重试次数
retry_delay = 5                # 重试基础延迟（秒），实际 = retry_delay * 2^n + 抖动

[deepseek]
api_key = "sk-xxx"     # DeepSeek API 密钥
model = "deepseek-v4-flash"

[ollama]
base_url = "http://127.0.0.1:11434"
model = "qwen2.5:7b"
system_prompt = ""     # 留空则沿用 [deepseek].system_prompt
max_tokens = 200
temperature = 0.7

[reply]
enabled = true         # 自动回复总开关（关闭后只抓取不回复）
prefix = ""            # 回复前缀
max_process = 10       # 每轮最多回复多少条（跨所有视频累计）
reply_delay = 2        # 回复成功后的间隔（秒）
chained_reply_enabled = true
max_reply_depth = 3    # 楼中楼递归层级（与 Python 版语义一致）

# 关键词过滤（[reply.keyword_filter]）
# 启用后，评论内容命中黑名单则跳过；白名单非空时需匹配白名单才回复
# mode: any（任一匹配）/ all（全部匹配）；match_case: 是否区分大小写

# 评论长度过滤（[reply.length_filter]）
# min_length / max_length，0 表示不限制（按 UTF-8 字符数计数）

# 用户过滤（[reply.user_filter]）
# whitelist / blacklist 为逗号分隔的 UID 列表

[ai]
provider = "deepseek"  # "deepseek" 或 "ollama"

[logging]
level = "INFO"         # ERROR / WARN / INFO / DEBUG / TRACE
file = "logs/bot.log"  # 相对路径基于数据目录；超过 5MB 自动轮转为 .1
console = true         # 是否同时输出到标准输出

[app]
autostart = false      # 开机自启（Windows 注册表 Run 项，开机后静默进入托盘）
auto_start_bot = true  # 启动时配置完整就自动开始运行机器人
close_action = "ask"   # 关闭主窗口时：ask（每次询问）/ tray（最小化到托盘）/ exit（直接退出）
```

> **提示**：`[cache]` 段（HTTP 响应缓存）在 Rust 版中**未实现**，仅为兼容旧配置保留解析，
> 界面也不再展示；`reply.only_new` 同样不产生行为（去重始终生效）。手改配置时写入的
> 未知字段不会在保存时保留。

---

## 数据目录与备份

所有运行时数据都在同一个目录，**不在程序安装目录、也不在当前工作目录**：

| 平台 | 路径 |
|------|------|
| Windows | `%APPDATA%\BiliCommentBot-RS`（即 `C:\Users\<你>\AppData\Roaming\BiliCommentBot-RS`） |
| 其它 | 可执行文件同级的 `BiliCommentBot-RS/` |

可用环境变量 `BILICOMMENTBOT_DATA_DIR` 覆盖（开发模式下已指向 `src-tauri/target/dev-data`，
免得数据混进源码目录）。

```
%APPDATA%\BiliCommentBot-RS\
├── config.toml            # 配置（含 Cookie 与 API Key）
├── bilibili_cookie.json   # 登录凭证
├── history.db             # 回复历史 / 去重依据（SQLite）
├── video_cache.json       # 视频列表缓存
├── app.lock               # 单实例锁
└── logs\bot.log           # 日志（轮转文件为 bot.log.1）
```

**备份**就是复制上面这些文件；**迁移到新机器**把整个目录拷过去即可。

> ⚠️ **不要把这些文件提交到 git**：真实 `config.toml` 里含有 B站 会话 Cookie 与
> DeepSeek API Key，泄露即等于账号被接管。仓库的 `.gitignore` 已覆盖这些文件名，
> CI 也会校验它们没有被跟踪。

### 从旧版本升级

旧版本把数据放在程序的工作目录（开发模式下是 `src-tauri/`，安装后是安装目录）。
新版本首次启动时会**自动把旧文件搬进数据目录**（只在目标不存在时迁移，不覆盖），
无需手动操作。历史 JSON 也会被继续自动迁移进 SQLite。

---

## 卸载

卸载程序**不会**删除 `%APPDATA%\BiliCommentBot-RS`（这是刻意的，避免误删历史）。
想彻底清理，先在应用内用「设置 → 清空所有数据」，或卸载后手动删除该目录。

---

## AI 提供商

| 特性 | DeepSeek | Ollama |
|------|----------|--------|
| 部署方式 | 云端 API | 本地运行 |
| 隐私性 | 评论发送到云端 | 完全本地 |
| 速度 | 取决于网络 | 取决于硬件 |
| 费用 | API 调用计费 | 免费 |
| 切换方式 | 配置 `[ai] provider` 字段 | — |

---

## 与 Python 版对比

| 特性 | Python Web UI 版 | Rust Tauri 版 |
|------|-----------------|---------------|
| 启动方式 | `python main.py` → 浏览器 | 双击 EXE |
| 包体大小 | ~50MB (Docker) | **< 8MB** (.msi) |
| 内存占用 | ~50-100MB | **~25-40MB** |
| 启动速度 | 2-5s | < 1s |
| 新增功能 | — | 新手教程向导、Ollama 支持、双 AI 引擎 |

---

## 更新记录

- **🔐 安全修复：数据不再写入工作目录**（重要）
  - `config.toml`、`bilibili_cookie.json`、`history.db`、`video_cache.json`、`logs/` 统一改到用户数据目录（`%APPDATA%\BiliCommentBot-RS`）。此前它们写在进程的当前工作目录，而快捷方式/注册表自启的 CWD 并不可控，既可能读到另一份配置，也可能因为安装目录无写权限而保存失败。
  - 直接后果是旧版把含 Cookie 与 API Key 的 `config.toml` 写进了 `src-tauri/`，并被提交到公开仓库。现已从版本控制移除、`.gitignore` 改为不带根锚定的匹配、并新增 CI 校验；**你仍然必须吊销当时泄露的 B站 会话与 API Key**。
  - 旧数据会在首次启动时自动迁移到新位置；`history.db` 打不开时不再 panic，而是禁用启动机器人（避免重复回复）并给出提示。
- **新增日志落盘**：`[logging]` 现在真正生效（级别 / 文件 / 控制台，支持热更新），超过 5MB 自动轮转为 `bot.log.1`。此前只用 `env_logger` 写 stderr，而正式版 `windows_subsystem = "windows"` 没有控制台，等于**日志被全部丢弃**、事后无法排查。
- **补上重试机制**：`rate_limit.max_retries` / `retry_delay` 由「只存不读」变为真实生效 —— 新增 `RetryPolicy`，所有 B站 请求与 DeepSeek/Ollama 调用都会按指数退避重试，并遵守 `Retry-After`；限流判定补齐了 `+412` 并覆盖全部接口（此前只在视频列表处判断一次）。
- **修复「启用自动回复」总开关**：此前后端从未读取 `reply.enabled`，用户关掉开关后机器人照样公开发布评论。
- **修复点赞用户视频**：`like_video` / `get_user_latest_video` / `check_is_follower` 现在会带上会话 Cookie（此前是未登录请求，必然失败）；「仅点赞粉丝视频」的判断方向也被纠正（此前把两个 UID 传反，问的是"我有没有关注对方"），未配置 UID 时按 Python 版语义跳过而不是照点。
- **「清空所有数据」不再清空程序目录**：改为只处理数据目录内的已知文件，并且**先把清单展示在确认框里**；没有可清理文件时不再谎称"应用即将退出"。
- **Cookie 手动登录更安全**：改为**先校验再保存**，校验失败会完整回滚到原有凭证；命令同时返回校验结果，界面不再重复请求一次验证。
- **修复嵌套回复的目标**：楼中楼的 `parent` 现在指向**被回复的那条评论**（此前指向根评论，回复挂错了位置）；评论列表改为「主评论在前、子评论在后」并加入跨页去重，`max_reply_depth` 的层级也与 Python 版对齐。
- **`max_process` 语义对齐**：改为**每轮跨所有视频累计**的上限（此前每个视频各算一份，有 N 个视频时可能发出 N 倍回复）。
- **`only_bvid` 变回「目标」而非「过滤器」**：指定 BVID 时直接只处理该视频，不再需要抓取整个投稿列表，因此也不再要求配置 `uid`。
- **上线即可用**：`[app] auto_start_bot`（默认开启）让配置完整的用户在启动时自动开始工作 —— 此前「开机自启」只会拉起一个待在托盘里不干活的图标。
- **新增单实例保护**：使用数据目录下的文件锁（进程退出/崩溃由系统自动释放），第二次启动会提示并退出，避免两个机器人抢同一份历史库与 Cookie。
- **Ollama 支持完整配置**：新增 `system_prompt` / `max_tokens` / `temperature`（提示词留空则沿用 `[deepseek].system_prompt`）。此前本地模型路径硬编码了提示词与参数，用户在界面上改的人设完全不生效。
- **修正失效开关**：从界面移除未实现的 HTTP 响应缓存（`cache.enabled` / `cache.expire_time`）与始终惰性的 `reply.only_new`，避免"界面有开关但后端不读"。
- **新增 CI**：push / PR 触发 `cargo clippy -- -D warnings` + `cargo test` + 前端构建，并校验运行时数据文件没有被纳入版本控制（此前只有 tag 触发的 release 工作流）。
- **前端首启体验修复**：向导第 4 步的密码此前**从未真正保存**却在摘要里显示「已设置」（现在会保存并校验，失败不前进）；二维码过期后无法重新生成（现在任何状态下都能重新生成，且不会留下重复的轮询计时器）；「清除历史」此前一键即删（现在与「清空所有数据」一样需要输入确认词）；锁屏输错密码此前没有任何反馈、仪表盘操作失败也只是写进 console（现在都有提示）；日志级别筛选的 `WARNING` 与后端实际输出的 `WARN` 对不上导致该筛选项永远为空（已修正并补上 `PREVIEW`）；视频列表刷新按钮此前点了没反应（现在会展示列表）。
- **其他**：移除 4 个未使用的依赖（`toml_edit` / `thiserror` / `url` / `uuid`）；新增 `[profile.release]`（LTO / strip / `opt-level="s"`）以缩小安装包；顺带清理了全部 clippy 警告，单元测试从 19 个增加到 73 个。

- **新增系统托盘与开机自启**：新增 `desktop.rs`（托盘图标 + 菜单「显示主窗口 / 退出程序」，左键单击托盘图标恢复窗口）与 `autostart.rs`（通过 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 实现开机自启，无需管理员权限）。
  - 关闭主窗口时按 `[app] close_action` 处理：`ask`（默认，弹出系统原生对话框选择「最小化到托盘」/「退出程序」）、`tray`（直接隐藏到托盘）、`exit`（直接退出）。隐藏到托盘时机器人继续在后台运行，只有「退出程序」才结束进程。
  - 开机自启写入的命令行带 `--minimized` 参数，开机后静默启动到托盘、不弹出窗口（`tauri.conf.json` 中窗口改为 `visible: false`，由启动逻辑决定是否显示，避免闪窗）。
  - 「系统设置 → 启动与窗口」新增开关与下拉选项，可随时修改并立即写入注册表 / `config.toml`。
- **新增「立即检查」**：仪表盘新增按钮，机器人运行时可手动触发立即开始下一轮评论扫描，无需等待检查间隔（`trigger_manual_check` 命令此前为空壳，现已真正生效）。
- **密码保护生效**：启用访问密码后，应用启动时弹出锁屏验证（此前密码可设置但从不校验，现已实际拦截，`verify_password` 命令接入启动流程）。
- **启动状态同步**：启动时主动拉取机器人运行状态，修复事件未到达时仪表盘显示陈旧"已停止"的问题（`get_bot_status` 接入启动流程）。
- **死代码清理**：移除从未采用的 `HttpClient` 抽象层、未被调用的 `decompress` 解压模块（及 `flate2` 依赖）、冗余的 `CookieManager::new` 构造器与 `BotEvent::History` 事件变体；`is_valid_bvid` 复用进 `bvid_to_aid` 输入校验；删除前端从未调用的 `getHistory` / `getHistoryGrouped` 包装函数。
- **新增评论过滤器**：从 Python 版同步移植关键词黑白名单、评论长度、用户 UID 黑白名单三重过滤。可在「配置 → 回复」标签页分别开关，命中过滤规则的评论将被跳过，不进入 AI 生成与回复流程。
- **修复回复评论失败**：回复/点赞请求未携带会话 Cookie（`SESSDATA`），导致 B站判定未登录而拒绝。现已将完整 Cookie 头及 `Origin`/`Referer` 一并附加到 Web API 请求，确保鉴权通过。
- **修复配置编辑不能立即生效**：`save_config` 现即时写入运行时配置（`bot_state.config`），使 `get_video_list`、`check_ollama_availability` 等命令立即返回新值；主循环排空所有待处理更新并只应用最新一条，且 `check_interval` 等待可被配置更新提前打断，新配置无需等到下一轮才生效。

---

## License

MIT
