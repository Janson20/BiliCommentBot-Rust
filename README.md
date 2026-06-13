# BiliCommentBot-RS

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
- 🌐 **新手迁移向导**：首次启动自动引导，一键导入 Python 版所有配置和历史
- 📊 实时仪表盘：运行状态、已回复统计、最近日志
- ⚙️ **配置热更新**：修改后立即生效无需重启
- 📜 实时日志查看 + 分级别过滤 + 搜索 + 导出
- 📋 回复历史记录查看 + 分页 + 清除
- 🔒 登录密码保护（SHA-256）
- 🐳 体积轻量：前端 ~25KB gzipped，Rust 后端无运行时

---

## 快速开始

### Windows 安装

1. 下载最新 `.msi` 安装包：[Releases](https://github.com/Janson20/BiliCommentBot-RS/releases)
2. 双击安装，桌面自动创建快捷方式
3. 首次启动自动弹出**新手向导**，引导完成配置

### 从 Python 版迁移

1. 启动 BiliCommentBot-RS
2. 在欢迎向导中选择「迁移旧版数据」
3. 选择 Python 版 `BiliCommentBot` 项目文件夹（包含 `config.toml`、`history.json`、`bilibili_cookie.json`、`video_cache.json`）
4. 自动导入所有配置和数据，无需重新扫码！

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
│   │   ├── http_client.rs  # UA/Referer 轮换
│   │   ├── decompress.rs   # gzip/zlib 解压
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
| 新增功能 | — | 新手向导、Ollama 支持、双 AI 引擎 |

---

## License

MIT
