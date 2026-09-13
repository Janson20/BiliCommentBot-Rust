<script>
  import { onMount } from "svelte";
  import { getHistoryByDate, clearHistory } from "../lib/api.js";
  import { showToast } from "../lib/stores.js";
  import CommentNode from "../components/CommentNode.svelte";

  let groups = [];
  let expanded = {};
  let loading = false;

  // ── 清除历史：沿用「设置 → 清空所有数据」的输入确认流程 ──
  let showClearConfirm = false;
  let confirmText = "";
  let clearing = false;
  const CONFIRM_PHRASE = "确认清空";

  onMount(() => { loadHistory(); });

  async function loadHistory() {
    loading = true;
    try {
      groups = await getHistoryByDate();
    } catch (e) {
      showToast("error", "加载失败: " + e);
    }
    loading = false;
  }

  function toggle(date) {
    expanded[date] = !expanded[date];
    expanded = expanded;
  }

  function openClearConfirm() {
    confirmText = "";
    showClearConfirm = true;
  }

  function cancelClear() {
    showClearConfirm = false;
    confirmText = "";
  }

  async function doClear() {
    if (confirmText !== CONFIRM_PHRASE) {
      showToast("error", `请输入 "${CONFIRM_PHRASE}" 确认`);
      return;
    }
    clearing = true;
    try {
      await clearHistory();
      groups = [];
      expanded = {};
      showClearConfirm = false;
      confirmText = "";
      showToast("success", "历史已清除");
    } catch (e) {
      showToast("error", "清除失败: " + e);
    }
    clearing = false;
  }
</script>

<h1>📋 回复历史</h1>

<div class="toolbar">
  <span class="total">{loading ? "加载中..." : `共 ${groups.length} 天`}</span>
  <button class="btn-refresh" on:click={loadHistory}>🔄 刷新</button>
  <button class="btn-danger" on:click={openClearConfirm} disabled={clearing}>🗑 清除历史...</button>
</div>

{#if showClearConfirm}
  <div class="confirm-panel">
    <p class="confirm-desc">
      此操作将清空全部回复历史记录，<strong>不可撤销</strong>。<br />
      请输入 "<strong>{CONFIRM_PHRASE}</strong>" 以确认：
    </p>
    <div class="confirm-row">
      <input
        type="text"
        class="confirm-input"
        bind:value={confirmText}
        placeholder={CONFIRM_PHRASE}
        disabled={clearing}
      />
      <button
        class="btn-danger"
        on:click={doClear}
        disabled={clearing || confirmText !== CONFIRM_PHRASE}
      >
        {clearing ? "清除中..." : "确认清除"}
      </button>
      <button class="btn-refresh" on:click={cancelClear} disabled={clearing}>取消</button>
    </div>
  </div>
{/if}

<div class="cards">
  {#each groups as group (group.date)}
    <div class="card" class:expanded={expanded[group.date]}>
      <button class="card-header" on:click={() => toggle(group.date)}>
        <span class="chevron">{expanded[group.date] ? "▼" : "▶"}</span>
        <div class="card-info">
          <span class="card-title">{group.date}</span>
          <span class="card-meta">{group.comment_count} 条根评论</span>
        </div>
      </button>

      {#if expanded[group.date]}
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
  .btn-danger:disabled { opacity: 0.4; cursor: not-allowed; }
  .confirm-panel {
    background: #2a1620; border: 1px solid #e74c3c55; border-radius: 8px;
    padding: 14px 16px; margin-bottom: 16px;
  }
  .confirm-desc { color: #e07070; font-size: 0.82rem; line-height: 1.6; margin-bottom: 10px; }
  .confirm-desc strong { color: #e74c3c; }
  .confirm-row { display: flex; align-items: center; gap: 10px; }
  .confirm-input {
    padding: 8px 12px; border-radius: 6px; border: 1px solid #c0392b;
    background: #0d1b2a; color: #e0e8f0; font-size: 0.85rem; outline: none;
    width: 200px;
  }
  .confirm-input:focus { border-color: #e74c3c; }
  .confirm-input:disabled { opacity: 0.6; }
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
  }
  .card-meta { font-size: 0.75rem; color: #5a7a9a; margin-top: 2px; display: block; }
  .card-body {
    border-top: 1px solid #1e3a5f; padding: 10px 16px 14px;
    max-height: 60vh; overflow-y: auto;
  }
  .empty { text-align: center; color: #5a7a9a; padding: 48px; }
</style>
