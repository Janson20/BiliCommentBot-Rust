<script>
  import { onMount } from "svelte";
  import { getHistoryGrouped, clearHistory } from "../lib/api.js";
  import { showToast } from "../lib/stores.js";
  import CommentNode from "../components/CommentNode.svelte";

  let groups = [];
  let expanded = {};
  let loading = false;

  onMount(() => { loadHistory(); });

  async function loadHistory() {
    loading = true;
    try {
      groups = await getHistoryGrouped();
    } catch (e) {
      showToast("error", "加载失败: " + e);
    }
    loading = false;
  }

  function toggle(bvid) {
    expanded[bvid] = !expanded[bvid];
    expanded = expanded;
  }

  async function doClear() {
    try {
      await clearHistory();
      groups = [];
      expanded = {};
      showToast("success", "历史已清除");
    } catch (e) {
      showToast("error", "清除失败: " + e);
    }
  }
</script>

<h1>📋 回复历史</h1>

<div class="toolbar">
  <span class="total">{loading ? "加载中..." : `共 ${groups.length} 个视频`}</span>
  <button class="btn-refresh" on:click={loadHistory}>🔄 刷新</button>
  <button class="btn-danger" on:click={doClear}>🗑 清除历史</button>
</div>

<div class="cards">
  {#each groups as group (group.bvid)}
    <div class="card" class:expanded={expanded[group.bvid]}>
      <button class="card-header" on:click={() => toggle(group.bvid)}>
        <span class="chevron">{expanded[group.bvid] ? "▼" : "▶"}</span>
        <div class="card-info">
          <span class="card-title">{group.video_title || group.bvid}</span>
          <span class="card-meta">
            {group.reply_count} 条回复
            {#if group.last_reply_time} · {group.last_reply_time}{/if}
          </span>
        </div>
      </button>

      {#if expanded[group.bvid]}
        <div class="card-body">
          {#each group.comments as comment}
            <CommentNode {comment} depth={0} />
          {/each}
        </div>
      {/if}
    </div>
  {/each}

  {#if groups.length === 0}
    <div class="empty">{loading ? "加载中..." : "暂无回复记录"}</div>
  {/if}
</div>

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 14px; }
  .toolbar { display: flex; align-items: center; gap: 12px; margin-bottom: 16px; }
  .total { color: #8aa0b8; font-size: 0.85rem; flex: 1; }
  .btn-refresh {
    padding: 6px 14px; border: 1px solid #1e3a5f; border-radius: 6px;
    background: #16213e; color: #b0c4de; cursor: pointer; font-size: 0.82rem;
  }
  .btn-refresh:hover { background: #1e3a5f; }
  .btn-danger {
    padding: 6px 14px; border: 1px solid #e74c3c33; border-radius: 6px;
    background: #e74c3c15; color: #e74c3c; cursor: pointer; font-size: 0.82rem;
  }
  .btn-danger:hover { background: #e74c3c25; }
  .cards { display: flex; flex-direction: column; gap: 8px; }
  .card {
    background: #16213e; border: 1px solid #1e3a5f; border-radius: 10px;
    overflow: hidden; transition: 0.15s;
  }
  .card.expanded { border-color: #00b4d850; }
  .card-header {
    display: flex; align-items: center; gap: 10px; padding: 14px 16px;
    width: 100%; border: none; background: transparent; color: #e0e8f0;
    cursor: pointer; text-align: left; font-size: 0.9rem;
  }
  .card-header:hover { background: rgba(255,255,255,0.03); }
  .chevron { color: #00b4d8; font-size: 0.7rem; flex-shrink: 0; width: 14px; }
  .card-info { flex: 1; min-width: 0; }
  .card-title {
    font-weight: 600; color: #e0e8f0; display: block;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .card-meta { font-size: 0.75rem; color: #5a7a9a; margin-top: 2px; display: block; }
  .card-body {
    border-top: 1px solid #1e3a5f; padding: 10px 16px 14px;
    max-height: 60vh; overflow-y: auto;
  }
  .empty { text-align: center; color: #5a7a9a; padding: 48px; }
</style>
