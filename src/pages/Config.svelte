<script>
  import { onMount } from "svelte";
  import { getConfig, saveConfig } from "../lib/api.js";
  import { config as cfgStore, showToast } from "../lib/stores.js";

  // 本地编辑副本
  let cfg = null;
  let activeTab = "bilibili";

  onMount(async () => {
    try {
      cfg = await getConfig();
      cfgStore.set(cfg);
    } catch (e) {
      showToast("error", "加载配置失败: " + e);
      cfg = {};
    }
  });

  function field(cfgObj, key) {
    return cfgObj?.[key] ?? "";
  }
  function setField(cfgObj, key, value) {
    if (!cfgObj) return;
    cfgObj[key] = value;
  }
  function boolField(cfgObj, key) {
    return cfgObj?.[key] === true;
  }
  function numField(cfgObj, key) {
    return Number(cfgObj?.[key]) || 0;
  }

  async function handleSave() {
    try {
      await saveConfig(cfg);
      cfgStore.set(cfg);
      showToast("success", "配置已保存");
    } catch (e) {
      showToast("error", "保存失败: " + e);
    }
  }

  const tabs = [
    { id: "bilibili", label: "B站" },
    { id: "deepseek", label: "DeepSeek" },
    { id: "ollama", label: "Ollama" },
    { id: "reply", label: "回复" },
    { id: "rate_limit", label: "频率" },
    { id: "cache", label: "缓存" },
    { id: "logging", label: "日志" },
    { id: "ai", label: "AI" },
  ];
</script>

<h1>⚙️ 配置编辑</h1>

{#if cfg}
  <div class="config-editor">
    <div class="tabs">
      {#each tabs as t}
        <button class="tab" class:active={activeTab === t.id}
          on:click={() => (activeTab = t.id)}>{t.label}</button>
      {/each}
    </div>

    <div class="tab-content">
      {#if activeTab === "bilibili"}
        <h2>B站API配置</h2>
        <div class="field">
          <label>Cookie</label>
          <textarea rows="2" value={field(cfg.bilibili, "cookie")}
            on:input={(e) => setField(cfg.bilibili, "cookie", e.target.value)}></textarea>
        </div>
        <div class="field"><label>Refresh Token</label><input type="text" value={field(cfg.bilibili, "refresh_token")}
          on:input={(e) => setField(cfg.bilibili, "refresh_token", e.target.value)} /></div>
        <div class="field"><label>用户UID</label><input type="text" value={field(cfg.bilibili, "uid")}
          on:input={(e) => setField(cfg.bilibili, "uid", e.target.value)} /></div>
        <div class="field"><label>检查间隔（秒）</label><input type="number" value={numField(cfg.bilibili, "check_interval")}
          on:input={(e) => setField(cfg.bilibili, "check_interval", Number(e.target.value))} /></div>
        <div class="field"><label>评论最大页数</label><input type="number" value={numField(cfg.bilibili, "max_comment_pages")}
          on:input={(e) => setField(cfg.bilibili, "max_comment_pages", Number(e.target.value))} /></div>
        <div class="field"><label>视频最大页数</label><input type="number" value={numField(cfg.bilibili, "max_video_pages")}
          on:input={(e) => setField(cfg.bilibili, "max_video_pages", Number(e.target.value))} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.bilibili, "auto_refresh_cookie")}
          on:change={(e) => setField(cfg.bilibili, "auto_refresh_cookie", e.target.checked)} /> 自动刷新Cookie</label>

      {:else if activeTab === "deepseek"}
        <h2>DeepSeek API</h2>
        <div class="field"><label>API Key <span class="hint">（以 sk- 开头，复制时勿带空格）</span></label><input type="text" value={field(cfg.deepseek, "api_key")}
          on:input={(e) => setField(cfg.deepseek, "api_key", e.target.value)} /></div>
        <div class="field"><label>API 地址</label><input type="text" value={field(cfg.deepseek, "base_url")}
          on:input={(e) => setField(cfg.deepseek, "base_url", e.target.value)} /></div>
        <div class="field"><label>模型</label><input type="text" value={field(cfg.deepseek, "model")}
          on:input={(e) => setField(cfg.deepseek, "model", e.target.value)} /></div>
        <div class="field"><label>最大Token</label><input type="number" value={numField(cfg.deepseek, "max_tokens")}
          on:input={(e) => setField(cfg.deepseek, "max_tokens", Number(e.target.value))} /></div>
        <div class="field"><label>温度</label><input type="number" step="0.1" min="0" max="1" value={field(cfg.deepseek, "temperature")}
          on:input={(e) => setField(cfg.deepseek, "temperature", parseFloat(e.target.value) || 0.7)} /></div>
        <div class="field"><label>系统提示词</label><textarea rows="3" value={field(cfg.deepseek, "system_prompt")}
          on:input={(e) => setField(cfg.deepseek, "system_prompt", e.target.value)}></textarea></div>

      {:else if activeTab === "ollama"}
        <h2>Ollama (本地模型)</h2>
        <div class="field"><label>服务地址</label><input type="text" value={field(cfg.ollama, "base_url")}
          on:input={(e) => setField(cfg.ollama, "base_url", e.target.value)} /></div>
        <div class="field"><label>模型名</label><input type="text" value={field(cfg.ollama, "model")}
          on:input={(e) => setField(cfg.ollama, "model", e.target.value)} /></div>
        <div class="field"><label>超时（秒）</label><input type="number" value={numField(cfg.ollama, "timeout_secs")}
          on:input={(e) => setField(cfg.ollama, "timeout_secs", Number(e.target.value))} /></div>

      {:else if activeTab === "reply"}
        <h2>回复设置</h2>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "enabled")}
          on:change={(e) => setField(cfg.reply, "enabled", e.target.checked)} /> 启用自动回复</label>
        <div class="field"><label>回复前缀</label><input type="text" value={field(cfg.reply, "prefix")}
          on:input={(e) => setField(cfg.reply, "prefix", e.target.value)} /></div>
        <div class="field"><label>每次最多处理</label><input type="number" value={numField(cfg.reply, "max_process")}
          on:input={(e) => setField(cfg.reply, "max_process", Number(e.target.value))} /></div>
        <div class="field"><label>回复延迟（秒）</label><input type="number" value={numField(cfg.reply, "reply_delay")}
          on:input={(e) => setField(cfg.reply, "reply_delay", Number(e.target.value))} /></div>
        <div class="field"><label>仅回复BVID</label><input type="text" value={field(cfg.reply, "only_bvid")}
          on:input={(e) => setField(cfg.reply, "only_bvid", e.target.value)} /></div>
        <div class="field"><label>上下文评论数</label><input type="number" value={numField(cfg.reply, "context_comments_count")}
          on:input={(e) => setField(cfg.reply, "context_comments_count", Number(e.target.value))} /></div>
        <div class="field"><label>最大链式回复深度</label><input type="number" value={numField(cfg.reply, "max_reply_depth")}
          on:input={(e) => setField(cfg.reply, "max_reply_depth", Number(e.target.value))} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "only_new")}
          on:change={(e) => setField(cfg.reply, "only_new", e.target.checked)} /> 仅回复新评论</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "like_enabled")}
          on:change={(e) => setField(cfg.reply, "like_enabled", e.target.checked)} /> 回复后点赞评论</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "chained_reply_enabled")}
          on:change={(e) => setField(cfg.reply, "chained_reply_enabled", e.target.checked)} /> 启用链式回复（楼中楼）</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "like_user_video_enabled")}
          on:change={(e) => setField(cfg.reply, "like_user_video_enabled", e.target.checked)} /> 点赞用户最新视频</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "like_user_video_only_followers")}
          on:change={(e) => setField(cfg.reply, "like_user_video_only_followers", e.target.checked)} /> 仅点赞粉丝视频</label>

      {:else if activeTab === "rate_limit"}
        <h2>频率控制</h2>
        <div class="field"><label>最小请求间隔（秒）</label><input type="number" step="0.5" value={field(cfg.rate_limit, "min_request_interval")}
          on:input={(e) => setField(cfg.rate_limit, "min_request_interval", parseFloat(e.target.value) || 2)} /></div>
        <div class="field"><label>最大重试次数</label><input type="number" value={numField(cfg.rate_limit, "max_retries")}
          on:input={(e) => setField(cfg.rate_limit, "max_retries", Number(e.target.value))} /></div>
        <div class="field"><label>重试延迟（秒）</label><input type="number" value={numField(cfg.rate_limit, "retry_delay")}
          on:input={(e) => setField(cfg.rate_limit, "retry_delay", Number(e.target.value))} /></div>

      {:else if activeTab === "cache"}
        <h2>缓存配置</h2>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.cache, "enabled")}
          on:change={(e) => setField(cfg.cache, "enabled", e.target.checked)} /> 启用缓存</label>
        <div class="field"><label>过期时间（秒）</label><input type="number" value={numField(cfg.cache, "expire_time")}
          on:input={(e) => setField(cfg.cache, "expire_time", Number(e.target.value))} /></div>
        <div class="field"><label>视频缓存过期（秒）</label><input type="number" value={numField(cfg.video_cache, "expire_time")}
          on:input={(e) => setField(cfg.video_cache, "expire_time", Number(e.target.value))} /></div>
        <div class="field"><label>视频缓存文件</label><input type="text" value={field(cfg.video_cache, "cache_file")}
          on:input={(e) => setField(cfg.video_cache, "cache_file", e.target.value)} /></div>

      {:else if activeTab === "logging"}
        <h2>日志设置</h2>
        <div class="field"><label>日志级别</label>
          <select value={field(cfg.logging, "level")}
            on:change={(e) => setField(cfg.logging, "level", e.target.value)}>
            <option>DEBUG</option><option>INFO</option><option>WARNING</option><option>ERROR</option>
          </select></div>
        <div class="field"><label>日志文件</label><input type="text" value={field(cfg.logging, "file")}
          on:input={(e) => setField(cfg.logging, "file", e.target.value)} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.logging, "console")}
          on:change={(e) => setField(cfg.logging, "console", e.target.checked)} /> 输出到控制台</label>

      {:else if activeTab === "ai"}
        <h2>AI提供商</h2>
        <div class="field"><label>选择提供商</label>
          <select value={cfg.ai?.provider || "deepseek"}
            on:change={(e) => { if (cfg.ai) cfg.ai.provider = e.target.value; }} >
            <option value="deepseek">DeepSeek</option>
            <option value="ollama">Ollama（本地）</option>
          </select></div>
      {/if}
    </div>

    <div class="save-area">
      <button class="btn-save" on:click={handleSave}>💾 保存配置</button>
    </div>
  </div>
{:else}
  <p>加载中...</p>
{/if}

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 16px; }
  h2 { font-size: 1.05rem; color: #8aa0b8; margin-bottom: 14px; padding-bottom: 8px; border-bottom: 1px solid #1e3a5f; }
  .tabs { display: flex; gap: 2px; margin-bottom: 18px; flex-wrap: wrap; }
  .tab {
    padding: 6px 14px; border: 1px solid #1e3a5f; background: #0d1b2a;
    border-radius: 6px; color: #8aa0b8; cursor: pointer; font-size: 0.82rem;
  }
  .tab.active { background: #00b4d8; color: #fff; border-color: #00b4d8; }
  .tab-content { max-height: 55vh; overflow-y: auto; padding-right: 8px; }
  .field { margin-bottom: 12px; }
  .field label { display: block; font-size: 0.82rem; color: #8aa0b8; margin-bottom: 4px; }
  .field label .hint { color: #5a7a9a; font-size: 0.72rem; }
  .field input, .field textarea, .field select {
    width: 100%; padding: 8px 10px; border-radius: 6px;
    border: 1px solid #1e3a5f; background: #0d1b2a; color: #e0e8f0;
    font-size: 0.85rem; outline: none;
  }
  .field textarea { resize: vertical; }
  .field input:focus, .field textarea:focus, .field select:focus { border-color: #00b4d8; }
  .checkbox {
    display: flex; align-items: center; gap: 8px; margin-bottom: 10px;
    font-size: 0.85rem; color: #c0d0e0; cursor: pointer;
  }
  .checkbox input { width: auto; accent-color: #00b4d8; }
  .save-area { margin-top: 20px; }
  .btn-save {
    padding: 10px 28px; border: none; border-radius: 8px;
    background: #00b4d8; color: #fff; font-weight: 600; font-size: 0.95rem;
    cursor: pointer; transition: 0.15s;
  }
  .btn-save:hover { opacity: 0.85; }
</style>
