# Changelog

本项目使用[约定式提交](https://www.conventionalcommits.org/zh-hans/)，发布说明由 CI 按提交信息自动生成。
本文件用于记录**尚未发版的改动**；已发布的版本请见 [Releases](https://github.com/Janson20/BiliCommentBot-Rust/releases)。

## Unreleased

### 安全（重要）

- 运行时数据（`config.toml` / `bilibili_cookie.json` / `history.db` / `video_cache.json` / `logs/`）
  从进程工作目录迁移到用户数据目录 `%APPDATA%\BiliCommentBot-RS`，并支持旧数据自动迁移。
  此前数据写在 CWD，而快捷方式与注册表自启的 CWD 并不可控，既可能读到另一份配置，也可能因安装目录
  无写权限而静默保存失败。
- 旧版本把含 Cookie 与 API Key 的 `config.toml` 写进了 `src-tauri/`，并被提交到公开仓库。
  现已从版本控制与**全部历史**中移除，`.gitignore` 改为不带根锚定的匹配，CI 会校验运行时数据文件
  未被跟踪。
  > ⚠️ 如果你曾 clone 过该仓库：泄露过的 B站 会话与 API Key 必须吊销（光删文件不够）。
- `tauri.conf.json` 收窄 allowlist：移除未使用的 `fs`（此前为 `**` 全盘读写）、`path`、`process`、
  `clipboard`、`protocol`，`dialog` 收窄为 `open`；同步精简 `Cargo.toml` 的 tauri features。
- 手动填写的 Cookie 改为**先校验再落盘**，校验失败时回滚原有凭证。
- 新增单实例保护（数据目录下的文件锁，进程退出即释放），避免两个实例抢同一份历史库。

### 修复

- **`reply.enabled` 现在真正生效**：此前后端从不读取该字段，用户关掉「启用自动回复」后机器人
  依然在公开发布回复。
- **`[logging]` 现在真正生效**：新增文件日志（级别 / 文件 / 控制台，支持热更新，5MB 自动轮转）。
  此前只用 `env_logger` 写 stderr，而正式版 `windows_subsystem = "windows"` 没有控制台，
  等于日志被全部丢弃。
- **`rate_limit.max_retries` / `retry_delay` 现在真正生效**：新增 `RetryPolicy` 重试层，覆盖全部
  B站 请求与 DeepSeek/Ollama 调用，支持 `Retry-After`，补齐 `+412` 限流码并覆盖所有接口。
- **`cookie_refresh_interval` 现在真正生效**：避免每一轮都去请求 passport 接口。
- **修复点赞用户视频**：补齐会话 Cookie（此前是未登录请求）；纠正「仅点赞粉丝视频」的 UID 传参方向
  （此前问的是"我有没有关注对方"）；未配置 UID 时按 Python 语义跳过。
- **楼中楼 `parent` 指向被回复的那条评论**（此前指向根评论，回复挂错位置）。
- **评论顺序改为主评论在前、子评论在后**，并加入跨页去重。
- **`max_reply_depth` 层级与 Python 版一致**（此前深一层，请求量更大）。
- **`max_process` 改为每轮跨所有视频累计**（此前每个视频各算一份，有 N 个视频时可能发出 N 倍回复）。
- **`only_bvid` 恢复为「目标」而非「过滤器」**：指定后只处理该视频，不再抓取投稿列表，也不再要求 `uid`。
- **`context_comments_count` 取当前评论之前的 N 条 + 父评论**（此前取列表前 N 条，等于每条评论拿到的
  上下文都一样）。
- **`history.db` 打不开时不再 panic**：标记为不可用并拒绝启动机器人（避免重复回复），给出明确提示。
- **「清空所有数据」不再清空程序目录**：改为只处理数据目录内的已知文件，并把清单展示在确认框里；
  没有可清理文件时不再谎称"应用即将退出"。
- **Ollama 支持 `system_prompt` / `max_tokens` / `temperature`**（此前硬编码，自定义人设对本地模型无效）。
- **界面移除失效开关**：未实现的 HTTP 响应缓存（`cache.*`）与始终惰性的 `reply.only_new`。
- **首次配置向导**：第 4 步的密码此前从未保存却在摘要里显示「已设置」；二维码过期后无法重新生成；
  「清除历史」一键即删；锁屏输错密码无反馈；日志级别 `WARNING` 与后端 `WARN` 不匹配导致该筛选项永远为空；
  视频列表刷新按钮点了没反应 —— 均已修复。

### 新增

- `[app] auto_start_bot`（默认开启）：配置完整（已登录 + 已配好 AI）时启动即开始运行机器人。
  此前「开机自启」只会拉起一个待在托盘里不干活的图标，与 README 的承诺不符。
- **异常提醒横幅**：登录失效 / 缺少 `bili_jct` / 连续失败达到阈值时，通过新的 `alert` 事件在界面上
  常驻提示。
- **托盘菜单**新增「启动机器人 / 停止机器人 / 立即检查」。
- `config.example.toml`：带注释的完整配置字段参考（不含任何凭证）。
- CI（`.github/workflows/ci.yml`）：push/PR 触发 `cargo clippy -D warnings` + 单元测试 + 前端构建，
  并校验运行时数据文件没有被纳入版本控制。此前只有 tag 触发的 release 工作流。
- 回复长度超出 B站 上限时主动截断并告警，而不是提交后被服务端拒绝。
- 手改 `config.toml` 的未知字段在界面保存后会被保留（`RawConfig` 增加未知字段兜底）。

### 工程

- 移除 4 个未使用依赖（`toml_edit` / `thiserror` / `url` / `uuid`）。
- 新增 `[profile.release]`（LTO / strip / `opt-level=s` / `codegen-units=1`），
  二进制由 13.2MB 降到 6.2MB。
- 单元测试由 19 个增加到 84 个，clippy 0 警告，前端构建 0 警告（此前 54 条）。
- 新增 `src-tauri/.cargo/config.toml`：开发与测试的数据目录隔离到 `target/dev-data`，
  避免运行时数据混进源码目录。
