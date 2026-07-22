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
- 🗑️ **一键清空**：将配置、历史、Cookie、日志等所有数据移入回收站（需输入确认文字）
- 🐳 体积轻量：前端 ~25KB gzipped，Rust 后端无运行时

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
│   │   ├── main.rs         # Tauri 入口 + BotState 初始化
│   │   ├── commands.rs     # 19 个 Tauri 命令
│   │   ├── bot.rs          # 机器人主循环编排
│   │   ├── config.rs       # TOML 配置管理
│   │   ├── cookie.rs       # Cookie 扫码/刷新/验证
│   │   ├── app_sign.rs     # BiliDroid 签名算法
│   │   ├── bvid.rs         # BVID ↔ AID 转换
│   │   ├── http_client.rs  # UA 轮换 + APP 参数
│   │   ├── rate_limiter.rs # 指数退避频率控制
│   │   ├── video_fetcher.rs # 视频列表获取
│   │   ├── comment_fetcher.rs # 评论+楼中楼获取
│   │   ├── deepseek.rs     # DeepSeek API
│   │   ├── ollama.rs       # Ollama 本地模型
│   │   ├── reply.rs        # 回复/点赞/粉丝检查
│   │   └── history.rs      # 历史记录管理
│   ├── Cargo.toml
│   └── tauri.conf.json
├── package.json
├── index.html
└── .gitignore
```

---

## 配置说明

配置文件 `config.toml`（**完全兼容** Python 版格式，可直接迁移）：

```toml
[bilibili]
cookie = ""            # B站 Cookie 或留空扫码
uid = ""               # 你的 B站 UID
check_interval = 60    # 检查间隔（秒）

[deepseek]
api_key = "sk-xxx"     # DeepSeek API 密钥
model = "deepseek-chat"

[ollama]
base_url = "http://127.0.0.1:11434"
model = "qwen2.5:7b"

[reply]
enabled = true
prefix = ""            # 回复前缀
max_process = 10       # 每次最多处理评论数
chained_reply_enabled = true
max_reply_depth = 3

# 关键词过滤（[reply.keyword_filter]）
# 启用后，评论内容命中黑名单则跳过；白名单非空时需匹配白名单才回复
# mode: any（任一匹配）/ all（全部匹配）；match_case: 是否区分大小写

# 评论长度过滤（[reply.length_filter]）
# min_length / max_length，0 表示不限制（按 UTF-8 字符数计数）

# 用户过滤（[reply.user_filter]）
# whitelist / blacklist 为逗号分隔的 UID 列表

[ai]
provider = "deepseek"  # "deepseek" 或 "ollama"
```

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
