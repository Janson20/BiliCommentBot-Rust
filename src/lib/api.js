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
export const getHistory = (page, pageSize) =>
  invoke("get_history", { page, pageSize });
export const getHistoryGrouped = () => invoke("get_history_grouped");
export const clearHistory = () => invoke("clear_history");

// ── Ollama ──
export const checkOllama = () => invoke("check_ollama_availability");
export const listOllamaModels = () => invoke("list_ollama_models");

// ── 密码 ──
export const setPassword = (pwd) => invoke("set_password", { password: pwd });
export const verifyPassword = (input) =>
  invoke("verify_password", { input });

// ── 清空数据 ──
export const clearAllData = () => invoke("clear_all_data");
