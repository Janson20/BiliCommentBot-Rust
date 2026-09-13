<script>
  import { isRunning, botStats, logs, videos, showToast } from "../lib/stores.js";
  import { startBot, stopBot, getVideoList, triggerManualCheck } from "../lib/api.js";

  let loading = false;
  let videosLoading = false;

  async function handleToggle() {
    loading = true;
    try {
      if ($isRunning) {
        await stopBot();
      } else {
        await startBot();
      }
    } catch (e) {
      showToast("error", String(e));
    }
    loading = false;
  }

  async function refreshVideos() {
    videosLoading = true;
    try {
      const list = await getVideoList();
      videos.set(Array.isArray(list) ? list : list?.videos || []);
    } catch (e) {
      showToast("error", String(e));
    }
    videosLoading = false;
  }

  async function manualCheck() {
    try {
      await triggerManualCheck();
    } catch (e) {
      showToast("error", String(e));
    }
  }

  // 简介可能很长，列表里只显示摘要
  const truncate = (s, n) => {
    const t = String(s ?? "");
    return t.length > n ? t.slice(0, n) + "…" : t;
  };

  // 最近日志
  $: recentLogs = $logs.slice(-6).reverse();
</script>

<h1>📊 仪表盘</h1>

<div class="grid-cards">
  <div class="card">
    <div class="card-label">运行状态</div>
    <div class="card-value">
      <span class="dot" class:on={$isRunning}></span>
      {$isRunning ? "运行中" : "已停止"}
    </div>
  </div>
  <div class="card">
    <div class="card-label">已回复评论</div>
    <div class="card-value num">{$botStats.total_replied}</div>
  </div>
  <div class="card">
    <div class="card-label">连续失败</div>
    <div class="card-value {$botStats.consecutive_failures > 0 ? 'warn' : 'num'}">{$botStats.consecutive_failures}</div>
  </div>
  <div class="card">
    <div class="card-label">启动时间</div>
    <div class="card-value time">{$botStats.start_time || "—"}</div>
  </div>
  <div class="card">
    <div class="card-label">最后检查</div>
    <div class="card-value time">{$botStats.last_check || "—"}</div>
  </div>
</div>

<div class="actions">
  <button
    class="btn-toggle"
    class:running={$isRunning}
    on:click={handleToggle}
    disabled={loading}
  >
    {loading ? "..." : $isRunning ? "⏹ 停止" : "▶ 启动"}
  </button>
  <button class="btn-secondary" on:click={refreshVideos} disabled={videosLoading}>
    {videosLoading ? "⏳ 刷新中..." : "🔄 刷新视频列表"}
  </button>
  <button class="btn-secondary" on:click={manualCheck} disabled={!$isRunning}>⚡ 立即检查</button>
</div>

<div class="section">
  <h2>🎬 视频列表{#if $videos.length > 0}<span class="section-note">共 {$videos.length} 个</span>{/if}</h2>
  {#if videosLoading}
    <div class="empty">正在获取视频列表...</div>
  {:else if $videos.length === 0}
    <div class="empty">暂无数据，点击上方「🔄 刷新视频列表」获取</div>
  {:else}
    <div class="video-list">
      {#each $videos as v}
        <div class="video-item">
          <div class="video-title" title={v.title}>{truncate(v.title, 40)}</div>
          {#if v.desc}
            <div class="video-desc" title={v.desc}>{truncate(v.desc, 60)}</div>
          {/if}
          <div class="video-meta">
            <span class="bvid">{v.bvid}</span>
            <span>播放 {v.play ?? 0}</span>
            <span>评论 {v.comment ?? 0}</span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<div class="section">
  <h2>📜 最近日志</h2>
  <div class="log-list">
    {#each recentLogs as log}
      <div class="log-entry log-{log.level.toLowerCase()}">
        <span class="log-time">{log.time}</span>
        <span class="log-level">[{log.level}]</span>
        <span class="log-msg">{log.msg}</span>
      </div>
    {/each}
    {#if recentLogs.length === 0}
      <div class="empty">暂无日志</div>
    {/if}
  </div>
</div>

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 20px; }
  .grid-cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 14px;
    margin-bottom: 20px;
  }
  .card {
    background: linear-gradient(135deg, #16213e 0%, #1a2a4a 100%);
    border: 1px solid #1e3a5f;
    border-radius: 10px;
    padding: 16px;
  }
  .card-label { font-size: 0.78rem; color: #8aa0b8; margin-bottom: 6px; }
  .card-value { font-size: 1.1rem; font-weight: 600; display: flex; align-items: center; gap: 6px; }
  .card-value.num { color: #00b4d8; font-size: 1.6rem; }
  .card-value.time { font-size: 0.85rem; color: #b0c4de; }
  .card-value.warn { color: #e74c3c; font-size: 1.6rem; }
  .dot {
    width: 10px; height: 10px; border-radius: 50%;
    background: #e74c3c; box-shadow: 0 0 6px rgba(231,76,60,0.5);
  }
  .dot.on { background: #2ecc71; box-shadow: 0 0 6px rgba(46,204,113,0.5); }
  .actions {
    display: flex; gap: 10px; margin-bottom: 24px;
  }
  .btn-toggle {
    padding: 10px 24px; border: none; border-radius: 8px; font-size: 0.95rem;
    font-weight: 600; cursor: pointer; color: #fff;
    background: #2ecc71; transition: 0.15s;
  }
  .btn-toggle:hover { opacity: 0.85; }
  .btn-toggle.running { background: #e74c3c; }
  .btn-toggle:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-secondary {
    padding: 10px 18px; border: 1px solid #1e3a5f; border-radius: 8px;
    background: #16213e; color: #b0c4de; cursor: pointer; font-size: 0.9rem;
  }
  .btn-secondary:hover { background: #1e3a5f; }
  .btn-secondary:disabled { opacity: 0.5; cursor: not-allowed; }
  .section { margin-top: 8px; }
  h2 { font-size: 1.05rem; color: #8aa0b8; margin-bottom: 10px; }
  .section-note { font-size: 0.78rem; color: #5a7a9a; margin-left: 8px; font-weight: 400; }
  .video-list {
    background: #0d1b2a; border-radius: 8px; padding: 8px 14px;
    max-height: 220px; overflow-y: auto;
  }
  .video-item { padding: 7px 0; border-bottom: 1px solid #152238; }
  .video-item:last-child { border: none; }
  .video-title {
    font-size: 0.85rem; color: #e0e8f0; font-weight: 600;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .video-desc {
    font-size: 0.76rem; color: #5a7a9a; margin-top: 2px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .video-meta {
    display: flex; gap: 12px; margin-top: 4px; flex-wrap: wrap;
    font-size: 0.75rem; color: #8aa0b8; font-family: "Consolas", monospace;
  }
  .video-meta .bvid { color: #00b4d8; }
  .log-list {
    background: #0d1b2a; border-radius: 8px; padding: 10px 14px;
    max-height: 220px; overflow-y: auto;
  }
  .log-entry {
    padding: 4px 0; font-size: 0.8rem; font-family: "Consolas", monospace;
    display: flex; gap: 10px; border-bottom: 1px solid #152238;
  }
  .log-entry:last-child { border: none; }
  .log-time { color: #5a7a9a; flex-shrink: 0; }
  .log-level { flex-shrink: 0; font-weight: 600; min-width: 52px; }
  .log-info .log-level { color: #00b4d8; }
  .log-warn .log-level { color: #f0c040; }
  .log-error .log-level { color: #e74c3c; }
  .log-debug .log-level { color: #5a7a9a; }
  .log-preview .log-level { color: #8aa0b8; }
  .log-msg { color: #c0d0e0; word-break: break-all; }
  .empty { text-align: center; color: #5a7a9a; padding: 16px; }
</style>
