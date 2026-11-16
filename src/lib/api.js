import { invoke } from "@tauri-apps/api/tauri";

// ── 机器人 ──
export const startBot = () => invoke("start_bot");
export const stopBot = () => invoke("stop_bot");
export const getBotStatus = () => invoke("get_bot_status");

// ── 配置 ──
export const getConfig = () => invoke("get_config");
export const saveConfig = (cfg) => invoke("save_config", { newConfig: cfg });
export const migrateFromOld = (dir) =>
  invoke("migrate_from_old_project", { oldProjectDir: dir });

// ── Cookie / 登录 ──
export const generateQrcode = () => invoke("generate_qrcode");
export const pollQrLogin = (key) => invoke("poll_qr_login", { qrcodeKey: key });
export const verifyCookie = () => invoke("verify_cookie");
export const refreshCookie = () => invoke("refresh_cookie");
export const setCookieManually = (cookieStr, refreshToken) =>
  invoke("set_cookie_manually", { cookieStr, refreshToken });

// ── 视频 ──
export const getVideoList = () => invoke("get_video_list");
export const triggerManualCheck = () => invoke("trigger_manual_check");

// ── 历史 ──
export const getHistoryByDate = () => invoke("get_history_by_date");
export const clearHistory = () => invoke("clear_history");

// ── Ollama ──
export const checkOllama = () => invoke("check_ollama_availability");
export const listOllamaModels = () => invoke("list_ollama_models");

// ── AI 连接 / 模型 / 用量 ──
export const testAiConnection = (provider) => invoke("test_ai_connection", { provider });
export const listAiModels = (provider) => invoke("list_ai_models", { provider });
export const getAiStats = () => invoke("get_ai_stats");
export const getIgnoredComments = (limit = 100) =>
  invoke("get_ignored_comments", { limit });
export const clearIgnoredComments = () => invoke("clear_ignored_comments");

// ── 密码 ──
export const setPassword = (pwd) => invoke("set_password", { password: pwd });
export const verifyPassword = (input) =>
  invoke("verify_password", { input });

// ── 开机自启（Windows 注册表 Run 项）──
export const getAutostartStatus = () => invoke("get_autostart_status");
export const setAutostart = (enabled) => invoke("set_autostart", { enabled });

// ── 清空数据 ──
// get_data_files → { data_dir, files: [{ path, exists, size }] }
export const getDataFiles = () => invoke("get_data_files");
// clear_all_data → { trashed, total, errors, data_dir, files }
export const clearAllData = () => invoke("clear_all_data");
