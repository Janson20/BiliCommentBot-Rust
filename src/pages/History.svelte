<script>
  import { onMount } from "svelte";
  import { getHistory, clearHistory } from "../lib/api.js";
  import { showToast } from "../lib/stores.js";

  let items = [];
  let total = 0;
  let page = 1;
  let pageSize = 30;
  let loading = false;

  onMount(() => { loadHistory(); });

  async function loadHistory() {
    loading = true;
    try {
      const r = await getHistory(page, pageSize);
      items = r.items || [];
      total = r.total || 0;
    } catch (e) {
      console.error(e);
    }
    loading = false;
  }

  function prevPage() {
    if (page <= 1) return;
    page--;
    loadHistory();
  }
  function nextPage() {
    if (page * pageSize >= total) return;
    page++;
    loadHistory();
  }

  async function doClear() {
    try {
      await clearHistory();
      items = [];
      total = 0;
      showToast("success", "历史已清除");
    } catch (e) {
      showToast("error", "清除失败: " + e);
    }
  }

  const maxPage = Math.ceil(total / pageSize) || 1;
</script>

<h1>📋 回复历史</h1>

<div class="toolbar">
  <span class="total">共 {total} 条记录</span>
  <div class="pager">
    <button on:click={prevPage} disabled={page <= 1}>←</button>
    <span class="page-info">{page} / {maxPage}</span>
    <button on:click={nextPage} disabled={page >= maxPage}>→</button>
  </div>
  <button class="btn-danger" on:click={doClear}>🗑 清除历史</button>
</div>

<div class="table-wrap">
  <table>
    <thead>
      <tr>
        <th>时间</th>
        <th>用户</th>
        <th>原评论</th>
        <th>AI回复</th>
      </tr>
    </thead>
    <tbody>
      {#each items as item}
        <tr>
          <td class="cell-time">{item.timestamp}</td>
          <td class="cell-user">{item.user}</td>
          <td class="cell-content">{item.content}</td>
          <td class="cell-reply">{item.reply_content}</td>
        </tr>
      {/each}
    </tbody>
  </table>
  {#if items.length === 0}
    <div class="empty">{loading ? "加载中..." : "暂无记录"}</div>
  {/if}
</div>

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 16px; }
  .toolbar { display: flex; align-items: center; gap: 14px; margin-bottom: 14px; }
  .total { color: #8aa0b8; font-size: 0.85rem; }
  .pager { display: flex; align-items: center; gap: 6px; }
  .pager button {
    padding: 4px 10px; border: 1px solid #1e3a5f; border-radius: 4px;
    background: #0d1b2a; color: #b0c4de; cursor: pointer; font-size: 0.8rem;
  }
  .pager button:disabled { opacity: 0.3; cursor: not-allowed; }
  .page-info { color: #b0c4de; font-size: 0.82rem; }
  .btn-danger {
    padding: 6px 14px; border: 1px solid #e74c3c33; border-radius: 6px;
    background: #e74c3c15; color: #e74c3c; cursor: pointer; font-size: 0.82rem;
  }
  .btn-danger:hover { background: #e74c3c25; }
  .table-wrap {
    max-height: 62vh; overflow-y: auto; border: 1px solid #152238; border-radius: 8px;
  }
  table { width: 100%; border-collapse: collapse; }
  th {
    text-align: left; padding: 10px 12px; font-size: 0.78rem; color: #8aa0b8;
    background: #16213e; border-bottom: 1px solid #1e3a5f; position: sticky; top: 0;
  }
  td { padding: 8px 12px; font-size: 0.82rem; border-bottom: 1px solid #152238; color: #c0d0e0; }
  .cell-time { white-space: nowrap; color: #5a7a9a; width: 120px; }
  .cell-user { color: #00b4d8; white-space: nowrap; width: 100px; }
  .cell-content, .cell-reply { max-width: 300px; word-break: break-all; }
  .empty { text-align: center; color: #5a7a9a; padding: 32px; }
</style>
