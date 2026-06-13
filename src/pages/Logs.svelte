<script>
  import { logs } from "../lib/stores.js";

  let filterLevel = "ALL";
  let search = "";

  $: filtered = $logs.filter((l) => {
    if (filterLevel !== "ALL" && l.level !== filterLevel) return false;
    if (search && !l.msg.toLowerCase().includes(search.toLowerCase())) return false;
    return true;
  }).reverse();

  function exportLogs() {
    const text = filtered.map((l) => `[${l.time}] [${l.level}] ${l.msg}`).join("\n");
    const blob = new Blob([text], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `bilibot-logs-${new Date().toISOString().slice(0,10)}.txt`;
    a.click();
    URL.revokeObjectURL(url);
  }
</script>

<h1>📜 日志查看</h1>

<div class="toolbar">
  <div class="filter-group">
    <label>级别</label>
    <select bind:value={filterLevel}>
      <option value="ALL">全部</option>
      <option value="DEBUG">DEBUG</option>
      <option value="INFO">INFO</option>
      <option value="WARNING">WARNING</option>
      <option value="ERROR">ERROR</option>
    </select>
  </div>
  <div class="filter-group search">
    <label>搜索</label>
    <input type="text" placeholder="搜索日志..." bind:value={search} />
  </div>
  <button class="btn-export" on:click={exportLogs}>📥 导出</button>
</div>

<div class="log-list">
  {#each filtered as log}
    <div class="log-entry log-{log.level.toLowerCase()}">
      <span class="time">{log.time}</span>
      <span class="level">[{log.level}]</span>
      <span class="msg">{log.msg}</span>
    </div>
  {/each}
  {#if filtered.length === 0}
    <div class="empty">
      {#if $logs.length === 0}暂无日志{:else}没有匹配的日志{/if}
    </div>
  {/if}
</div>

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 16px; }
  .toolbar { display: flex; gap: 14px; align-items: flex-end; margin-bottom: 16px; flex-wrap: wrap; }
  .filter-group { display: flex; flex-direction: column; gap: 3px; }
  .filter-group label { font-size: 0.78rem; color: #8aa0b8; }
  .filter-group select,
  .filter-group input {
    padding: 6px 10px; border-radius: 6px; border: 1px solid #1e3a5f;
    background: #0d1b2a; color: #e0e8f0; font-size: 0.82rem; outline: none;
  }
  .search input { width: 200px; }
  .filter-group select:focus,
  .filter-group input:focus { border-color: #00b4d8; }
  .btn-export {
    padding: 7px 16px; border: 1px solid #1e3a5f; border-radius: 6px;
    background: #16213e; color: #b0c4de; cursor: pointer; font-size: 0.85rem;
  }
  .btn-export:hover { background: #1e3a5f; }
  .log-list {
    background: #0d1b2a; border: 1px solid #152238; border-radius: 8px;
    padding: 8px 14px; max-height: 62vh; overflow-y: auto;
  }
  .log-entry {
    padding: 4px 0; font-size: 0.78rem; font-family: "Consolas", monospace;
    display: flex; gap: 10px; border-bottom: 1px solid #152238;
  }
  .log-entry:last-child { border: none; }
  .time { color: #5a7a9a; flex-shrink: 0; }
  .level { flex-shrink: 0; font-weight: 600; min-width: 52px; }
  .log-info .level { color: #00b4d8; }
  .log-warning .level { color: #f0c040; }
  .log-error .level { color: #e74c3c; }
  .log-debug .level { color: #5a7a9a; }
  .msg { color: #c0d0e0; word-break: break-all; }
  .empty { text-align: center; color: #5a7a9a; padding: 24px; }
</style>
