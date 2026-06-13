import { writable, derived } from "svelte/store";

// ── 机器人运行状态 ──
export const isRunning = writable(false);
export const botStats = writable({
  total_replied: 0,
  start_time: null,
  last_check: null,
  consecutive_failures: 0,
});

// ── 日志（环形缓冲区，最多500条） ──
export const logs = writable([]); // {time, level, msg}[]
const MAX_LOGS = 500;
export function appendLog(entry) {
  logs.update((arr) => {
    const next = [...arr, entry];
    if (next.length > MAX_LOGS) return next.slice(-MAX_LOGS);
    return next;
  });
}

// ── 配置 ──
export const config = writable(null);

// ── 视频列表 ──
export const videos = writable([]);

// ── 历史记录 ──
export const history = writable({ total: 0, page: 1, items: [] });

// ── 登录状态 ──
export const loginStatus = writable({ loggedIn: false, uname: null, uid: null });

// ── 通知消息 ──
export const toast = writable(null); // {type: 'info'|'error'|'success', text}
let toastTimer;
export function showToast(type, text) {
  toast.set({ type, text });
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toast.set(null), 4000);
}
