<script>
  import { onMount } from "svelte";
  import {
    getConfig,
    saveConfig,
    testAiConnection,
    listAiModels,
  } from "../lib/api.js";
  import { config as cfgStore, showToast } from "../lib/stores.js";

  // 本地编辑副本
  let cfg = null;
  let activeTab = "bilibili";

  // AI 连接测试 / 模型列表
  let testing = false;
  let testResult = null;
  let listingModels = false;
  let modelList = [];
  let modelProvider = "";

  onMount(async () => {
    try {
      cfg = await getConfig();
      // 旧配置可能缺少新增 section；这里补齐，避免输入框静默失效
      cfg.ai = cfg.ai || {};
      cfg.reply = cfg.reply || {};
      cfg.reply.ai_guard = cfg.reply.ai_guard || {};
      cfg.openai = cfg.openai || {};
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
  // 确保嵌套对象存在（如 cfg.reply.keyword_filter），不存在则创建并返回
  function ensureObj(parentObj, key) {
    if (!parentObj) return null;
    if (!parentObj[key] || typeof parentObj[key] !== "object") {
      parentObj[key] = {};
    }
    return parentObj[key];
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

  async function persist() {
    await saveConfig(cfg);
    cfgStore.set(cfg);
  }

  async function handleTest() {
    testing = true;
    testResult = null;
    try {
      // 先保存，确保后端测试的是界面上的当前值
      await persist();
      const r = await testAiConnection(cfg.ai?.provider || "deepseek");
      testResult = { ok: true, ...r };
    } catch (e) {
      testResult = { ok: false, error: String(e) };
    }
    testing = false;
  }

  async function fetchModels(provider) {
    listingModels = true;
    modelProvider = provider;
    modelList = [];
    try {
      await persist();
      const r = await listAiModels(provider);
      modelList = Array.isArray(r?.models) ? r.models : [];
      if (modelList.length === 0) showToast("info", "接口未返回模型列表");
    } catch (e) {
      showToast("error", "获取模型列表失败: " + e);
    }
    listingModels = false;
  }

  function useModel(section, name) {
    if (!cfg[section]) cfg[section] = {};
    cfg[section].model = name;
    cfg = cfg; // 触发 Svelte 重新计算模型 chip 的选中状态
    showToast("success", "已选择模型 " + name);
  }

  const tabs = [
    { id: "bilibili", label: "B站" },
    { id: "deepseek", label: "DeepSeek" },
    { id: "openai", label: "OpenAI兼容" },
    { id: "ollama", label: "Ollama" },
    { id: "reply", label: "回复" },
    { id: "rate_limit", label: "频率" },
    { id: "cache", label: "缓存" },
    { id: "logging", label: "日志" },
    { id: "ai", label: "AI" },
  ];
</script>

<h1>配置编辑</h1>

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
          <label for="cfg-bili-cookie">Cookie</label>
          <textarea id="cfg-bili-cookie" rows="2" value={field(cfg.bilibili, "cookie")}
            on:input={(e) => setField(cfg.bilibili, "cookie", e.target.value)}></textarea>
        </div>
        <div class="field"><label for="cfg-bili-refresh">Refresh Token</label><input id="cfg-bili-refresh" type="text" value={field(cfg.bilibili, "refresh_token")}
          on:input={(e) => setField(cfg.bilibili, "refresh_token", e.target.value)} /></div>
        <div class="field"><label for="cfg-bili-uid">用户UID</label><input id="cfg-bili-uid" type="text" value={field(cfg.bilibili, "uid")}
          on:input={(e) => setField(cfg.bilibili, "uid", e.target.value)} /></div>
        <div class="field"><label for="cfg-bili-interval">检查间隔（秒）</label><input id="cfg-bili-interval" type="number" value={numField(cfg.bilibili, "check_interval")}
          on:input={(e) => setField(cfg.bilibili, "check_interval", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-bili-max-comment">评论最大页数</label><input id="cfg-bili-max-comment" type="number" value={numField(cfg.bilibili, "max_comment_pages")}
          on:input={(e) => setField(cfg.bilibili, "max_comment_pages", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-bili-max-video">视频最大页数</label><input id="cfg-bili-max-video" type="number" value={numField(cfg.bilibili, "max_video_pages")}
          on:input={(e) => setField(cfg.bilibili, "max_video_pages", Number(e.target.value))} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.bilibili, "auto_refresh_cookie")}
          on:change={(e) => setField(cfg.bilibili, "auto_refresh_cookie", e.target.checked)} /> 自动刷新Cookie</label>

      {:else if activeTab === "deepseek"}
        <h2>DeepSeek API</h2>
        <div class="field"><label for="cfg-ds-key">API Key <span class="hint">（以 sk- 开头，复制时勿带空格）</span></label><input id="cfg-ds-key" type="text" value={field(cfg.deepseek, "api_key")}
          on:input={(e) => setField(cfg.deepseek, "api_key", e.target.value)} /></div>
        <div class="field"><label for="cfg-ds-url">API 地址</label><input id="cfg-ds-url" type="text" value={field(cfg.deepseek, "base_url")}
          on:input={(e) => setField(cfg.deepseek, "base_url", e.target.value)} /></div>
        <div class="field"><label for="cfg-ds-model">模型</label><input id="cfg-ds-model" type="text" value={field(cfg.deepseek, "model")}
          on:input={(e) => setField(cfg.deepseek, "model", e.target.value)} /></div>
        <div class="field"><label for="cfg-ds-max-tokens">最大Token</label><input id="cfg-ds-max-tokens" type="number" value={numField(cfg.deepseek, "max_tokens")}
          on:input={(e) => setField(cfg.deepseek, "max_tokens", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-ds-temp">温度</label><input id="cfg-ds-temp" type="number" step="0.1" min="0" max="1" value={field(cfg.deepseek, "temperature")}
          on:input={(e) => setField(cfg.deepseek, "temperature", parseFloat(e.target.value) || 0.7)} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.deepseek, "thinking")}
          on:change={(e) => setField(cfg.deepseek, "thinking", e.target.checked)} /> 使用思考模式</label>
        <p class="hint-note">新模型默认开启思考模式：先输出思维链再回复，更慢、更耗 token，
          且官方说明思考模式下「温度」会被忽略。评论自动回复建议保持关闭。</p>
        <div class="field"><label for="cfg-ds-prompt">系统提示词</label><textarea id="cfg-ds-prompt" rows="3" value={field(cfg.deepseek, "system_prompt")}
          on:input={(e) => setField(cfg.deepseek, "system_prompt", e.target.value)}></textarea></div>
        <div class="field">
          <button type="button" class="btn-mini" on:click={() => fetchModels("deepseek")} disabled={listingModels}>
            {listingModels && modelProvider === "deepseek" ? "获取中..." : "获取模型列表"}
          </button>
        </div>
        {#if modelProvider === "deepseek" && modelList.length > 0}
          <div class="model-chips">
            {#each modelList as m}
              <button type="button" class="chip" class:active={cfg.deepseek.model === m}
                on:click={() => useModel("deepseek", m)}>{m}</button>
            {/each}
          </div>
        {/if}

      {:else if activeTab === "openai"}
        <h2>OpenAI 兼容端点</h2>
        <p class="hint-note">支持 OpenAI、OpenRouter、SiliconFlow、LM Studio、vLLM 等标准
          /chat/completions 接口。base_url 需带版本前缀，例如 https://api.openai.com/v1。</p>
        <div class="field"><label for="cfg-openai-key">API Key <span class="hint">（本地服务可留空）</span></label>
          <input id="cfg-openai-key" type="password" value={field(cfg.openai, "api_key")}
            on:input={(e) => setField(cfg.openai, "api_key", e.target.value)} /></div>
        <div class="field"><label for="cfg-openai-url">API 地址</label>
          <input id="cfg-openai-url" type="text" value={field(cfg.openai, "base_url")}
            on:input={(e) => setField(cfg.openai, "base_url", e.target.value)} /></div>
        <div class="field"><label for="cfg-openai-model">模型</label>
          <input id="cfg-openai-model" type="text" value={field(cfg.openai, "model")}
            placeholder="例如 gpt-4o-mini / qwen3 / glm-4.6"
            on:input={(e) => setField(cfg.openai, "model", e.target.value)} /></div>
        <div class="field"><label for="cfg-openai-max">最大 Token</label>
          <input id="cfg-openai-max" type="number" value={numField(cfg.openai, "max_tokens")}
            on:input={(e) => setField(cfg.openai, "max_tokens", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-openai-temp">温度</label>
          <input id="cfg-openai-temp" type="number" step="0.1" min="0" max="2" value={field(cfg.openai, "temperature")}
            on:input={(e) => setField(cfg.openai, "temperature", parseFloat(e.target.value) || 0.7)} /></div>
        <div class="field"><label for="cfg-openai-token-param">Token 上限字段</label>
          <select id="cfg-openai-token-param" value={field(cfg.openai, "token_param") || "max_tokens"}
            on:change={(e) => setField(cfg.openai, "token_param", e.target.value)}>
            <option value="max_tokens">max_tokens（多数兼容端点）</option>
            <option value="max_completion_tokens">max_completion_tokens（o 系列推理模型）</option>
          </select></div>
        <div class="field"><label for="cfg-openai-effort">推理强度 <span class="hint">（留空不发送；设置后不发送 temperature）</span></label>
          <select id="cfg-openai-effort" value={field(cfg.openai, "reasoning_effort")}
            on:change={(e) => setField(cfg.openai, "reasoning_effort", e.target.value)}>
            <option value="">默认</option>
            <option value="low">low</option>
            <option value="medium">medium</option>
            <option value="high">high</option>
          </select></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.openai, "json_mode")}
          on:change={(e) => setField(cfg.openai, "json_mode", e.target.checked)} />
          请求 JSON 模式（端点不支持 response_format 时关闭）</label>
        <div class="field"><label for="cfg-openai-prompt">系统提示词 <span class="hint">（留空沿用 DeepSeek）</span></label>
          <textarea id="cfg-openai-prompt" rows="3" value={field(cfg.openai, "system_prompt")}
            on:input={(e) => setField(cfg.openai, "system_prompt", e.target.value)}></textarea></div>
        <div class="field">
          <button type="button" class="btn-mini" on:click={() => fetchModels("openai")} disabled={listingModels}>
            {listingModels && modelProvider === "openai" ? "获取中..." : "获取模型列表"}
          </button>
        </div>
        {#if modelProvider === "openai" && modelList.length > 0}
          <div class="model-chips">
            {#each modelList as m}
              <button type="button" class="chip" class:active={cfg.openai.model === m}
                on:click={() => useModel("openai", m)}>{m}</button>
            {/each}
          </div>
        {/if}

      {:else if activeTab === "ollama"}
        <h2>Ollama (本地模型)</h2>
        <div class="field"><label for="cfg-ollama-url">服务地址</label><input id="cfg-ollama-url" type="text" value={field(cfg.ollama, "base_url")}
          on:input={(e) => setField(cfg.ollama, "base_url", e.target.value)} /></div>
        <div class="field"><label for="cfg-ollama-model">模型名</label><input id="cfg-ollama-model" type="text" value={field(cfg.ollama, "model")}
          on:input={(e) => setField(cfg.ollama, "model", e.target.value)} /></div>
        <div class="field"><label for="cfg-ollama-timeout">超时（秒）</label><input id="cfg-ollama-timeout" type="number" value={numField(cfg.ollama, "timeout_secs")}
          on:input={(e) => setField(cfg.ollama, "timeout_secs", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-ollama-ctx">上下文窗口（num_ctx） <span class="hint">（提示词较长时调大）</span></label><input id="cfg-ollama-ctx" type="number" value={numField(cfg.ollama, "num_ctx")}
          on:input={(e) => setField(cfg.ollama, "num_ctx", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-ollama-max">最大 Token</label><input id="cfg-ollama-max" type="number" value={numField(cfg.ollama, "max_tokens")}
          on:input={(e) => setField(cfg.ollama, "max_tokens", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-ollama-temp">温度</label><input id="cfg-ollama-temp" type="number" step="0.1" min="0" max="2" value={field(cfg.ollama, "temperature")}
          on:input={(e) => setField(cfg.ollama, "temperature", parseFloat(e.target.value) || 0.7)} /></div>
        <div class="field"><label for="cfg-ollama-prompt">系统提示词 <span class="hint">（留空沿用 DeepSeek）</span></label>
          <textarea id="cfg-ollama-prompt" rows="3" value={field(cfg.ollama, "system_prompt")}
            on:input={(e) => setField(cfg.ollama, "system_prompt", e.target.value)}></textarea></div>
        <div class="field">
          <button type="button" class="btn-mini" on:click={() => fetchModels("ollama")} disabled={listingModels}>
            {listingModels && modelProvider === "ollama" ? "获取中..." : "获取模型列表"}
          </button>
        </div>
        {#if modelProvider === "ollama" && modelList.length > 0}
          <div class="model-chips">
            {#each modelList as m}
              <button type="button" class="chip" class:active={cfg.ollama.model === m}
                on:click={() => useModel("ollama", m)}>{m}</button>
            {/each}
          </div>
        {/if}

      {:else if activeTab === "reply"}
        <h2>回复设置</h2>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "enabled")}
          on:change={(e) => setField(cfg.reply, "enabled", e.target.checked)} /> 启用自动回复</label>
        <label class="checkbox dry"><input type="checkbox" checked={boolField(cfg.reply, "dry_run")}
          on:change={(e) => setField(cfg.reply, "dry_run", e.target.checked)} /> 预览模式：仅生成AI回复日志，不实际发表</label>
        <div class="field"><label for="cfg-reply-prefix">回复前缀</label><input id="cfg-reply-prefix" type="text" value={field(cfg.reply, "prefix")}
          on:input={(e) => setField(cfg.reply, "prefix", e.target.value)} /></div>
        <div class="field"><label for="cfg-reply-max">每次最多处理</label><input id="cfg-reply-max" type="number" value={numField(cfg.reply, "max_process")}
          on:input={(e) => setField(cfg.reply, "max_process", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-reply-delay">回复延迟（秒）</label><input id="cfg-reply-delay" type="number" value={numField(cfg.reply, "reply_delay")}
          on:input={(e) => setField(cfg.reply, "reply_delay", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-reply-only-bvid">仅回复BVID</label><input id="cfg-reply-only-bvid" type="text" value={field(cfg.reply, "only_bvid")}
          on:input={(e) => setField(cfg.reply, "only_bvid", e.target.value)} /></div>
        <div class="field"><label for="cfg-reply-context">上下文评论数 <span class="hint">（同视频其他评论，0=不附带）</span></label><input id="cfg-reply-context" type="number" value={numField(cfg.reply, "context_comments_count")}
          on:input={(e) => setField(cfg.reply, "context_comments_count", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-reply-thread">楼中楼对话链层数 <span class="hint">（0=不附带父评论）</span></label><input id="cfg-reply-thread" type="number" value={numField(cfg.reply, "thread_context_max")}
          on:input={(e) => setField(cfg.reply, "thread_context_max", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-reply-style">历史风格示例条数 <span class="hint">（0=关闭）</span></label><input id="cfg-reply-style" type="number" value={numField(cfg.reply, "style_examples_count")}
          on:input={(e) => setField(cfg.reply, "style_examples_count", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-reply-dedupe">回复相似度阈值 <span class="hint">（0.82 推荐，0=关闭去重）</span></label><input id="cfg-reply-dedupe" type="number" step="0.05" min="0" max="1" value={field(cfg.reply, "dedupe_threshold")}
          on:input={(e) => setField(cfg.reply, "dedupe_threshold", parseFloat(e.target.value) || 0)} /></div>
        <div class="field"><label for="cfg-reply-depth">最大链式回复深度</label><input id="cfg-reply-depth" type="number" value={numField(cfg.reply, "max_reply_depth")}
          on:input={(e) => setField(cfg.reply, "max_reply_depth", Number(e.target.value))} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "like_enabled")}
          on:change={(e) => setField(cfg.reply, "like_enabled", e.target.checked)} /> 回复后点赞评论</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "chained_reply_enabled")}
          on:change={(e) => setField(cfg.reply, "chained_reply_enabled", e.target.checked)} /> 启用链式回复（楼中楼）</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "like_user_video_enabled")}
          on:change={(e) => setField(cfg.reply, "like_user_video_enabled", e.target.checked)} /> 点赞用户最新视频</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply, "like_user_video_only_followers")}
          on:change={(e) => setField(cfg.reply, "like_user_video_only_followers", e.target.checked)} /> 仅点赞粉丝视频</label>

        <h3 class="sub-title">AI 安全门</h3>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.ai_guard, "enabled")}
          on:change={(e) => setField(cfg.reply.ai_guard, "enabled", e.target.checked)} />
          启用结构化决策（广告/引战/敏感/无意义评论直接跳过）</label>
        <p class="hint-note">开启后每次生成会要求模型输出 JSON 决策，并在置信度不足时复核一次。
          被跳过的评论会记录原因，下一轮不再重复调用 AI。</p>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.ai_guard, "skip_spam")}
          on:change={(e) => setField(cfg.reply.ai_guard, "skip_spam", e.target.checked)} /> 跳过广告/引流</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.ai_guard, "skip_toxic")}
          on:change={(e) => setField(cfg.reply.ai_guard, "skip_toxic", e.target.checked)} /> 跳过辱骂/引战</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.ai_guard, "skip_sensitive")}
          on:change={(e) => setField(cfg.reply.ai_guard, "skip_sensitive", e.target.checked)} /> 跳过敏感话题</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.ai_guard, "skip_meaningless")}
          on:change={(e) => setField(cfg.reply.ai_guard, "skip_meaningless", e.target.checked)} /> 跳过无意义灌水</label>
        <div class="field"><label for="cfg-guard-confidence">跳过低置信度阈值 <span class="hint">（0.6 推荐，越高越保守）</span></label><input id="cfg-guard-confidence" type="number" step="0.05" min="0" max="1" value={field(cfg.reply?.ai_guard, "min_confidence")}
          on:input={(e) => setField(cfg.reply.ai_guard, "min_confidence", parseFloat(e.target.value) || 0.6)} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.ai_guard, "review_low_confidence")}
          on:change={(e) => setField(cfg.reply.ai_guard, "review_low_confidence", e.target.checked)} /> 低置信度时追加一次复核</label>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.ai_guard, "store_skips")}
          on:change={(e) => setField(cfg.reply.ai_guard, "store_skips", e.target.checked)} /> 记录跳过原因（避免重复调用 AI）</label>

        <h3 class="sub-title">关键词过滤</h3>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.keyword_filter, "enabled")}
          on:change={(e) => setField(ensureObj(cfg.reply, "keyword_filter"), "enabled", e.target.checked)} /> 启用关键词过滤</label>
        <div class="field"><label for="cfg-kf-whitelist">白名单关键词 <span class="hint">（逗号分隔，匹配才回复；留空不限制）</span></label><input id="cfg-kf-whitelist" type="text" value={field(cfg.reply?.keyword_filter, "whitelist")}
          on:input={(e) => setField(ensureObj(cfg.reply, "keyword_filter"), "whitelist", e.target.value)} /></div>
        <div class="field"><label for="cfg-kf-blacklist">黑名单关键词 <span class="hint">（逗号分隔，匹配任一则跳过）</span></label><input id="cfg-kf-blacklist" type="text" value={field(cfg.reply?.keyword_filter, "blacklist")}
          on:input={(e) => setField(ensureObj(cfg.reply, "keyword_filter"), "blacklist", e.target.value)} /></div>
        <div class="field"><label for="cfg-kf-mode">白名单匹配模式</label>
          <select id="cfg-kf-mode" value={field(cfg.reply?.keyword_filter, "mode") || "any"}
            on:change={(e) => setField(ensureObj(cfg.reply, "keyword_filter"), "mode", e.target.value)}>
            <option value="any">any（任一匹配）</option>
            <option value="all">all（全部匹配）</option>
          </select></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.keyword_filter, "match_case")}
          on:change={(e) => setField(ensureObj(cfg.reply, "keyword_filter"), "match_case", e.target.checked)} /> 区分大小写</label>

        <h3 class="sub-title">评论长度过滤</h3>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.length_filter, "enabled")}
          on:change={(e) => setField(ensureObj(cfg.reply, "length_filter"), "enabled", e.target.checked)} /> 启用长度过滤</label>
        <div class="field"><label for="cfg-lf-min">最短长度 <span class="hint">（0=不限制）</span></label><input id="cfg-lf-min" type="number" value={numField(cfg.reply?.length_filter, "min_length")}
          on:input={(e) => setField(ensureObj(cfg.reply, "length_filter"), "min_length", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-lf-max">最长长度 <span class="hint">（0=不限制）</span></label><input id="cfg-lf-max" type="number" value={numField(cfg.reply?.length_filter, "max_length")}
          on:input={(e) => setField(ensureObj(cfg.reply, "length_filter"), "max_length", Number(e.target.value))} /></div>

        <h3 class="sub-title">用户过滤</h3>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.reply?.user_filter, "enabled")}
          on:change={(e) => setField(ensureObj(cfg.reply, "user_filter"), "enabled", e.target.checked)} /> 启用用户过滤</label>
        <div class="field"><label for="cfg-uf-whitelist">白名单 UID <span class="hint">（逗号分隔，仅回复这些用户）</span></label><input id="cfg-uf-whitelist" type="text" value={field(cfg.reply?.user_filter, "whitelist")}
          on:input={(e) => setField(ensureObj(cfg.reply, "user_filter"), "whitelist", e.target.value)} /></div>
        <div class="field"><label for="cfg-uf-blacklist">黑名单 UID <span class="hint">（逗号分隔，屏蔽这些用户）</span></label><input id="cfg-uf-blacklist" type="text" value={field(cfg.reply?.user_filter, "blacklist")}
          on:input={(e) => setField(ensureObj(cfg.reply, "user_filter"), "blacklist", e.target.value)} /></div>

      {:else if activeTab === "rate_limit"}
        <h2>频率控制</h2>
        <div class="field"><label for="cfg-rl-interval">最小请求间隔（秒）</label><input id="cfg-rl-interval" type="number" step="0.5" value={field(cfg.rate_limit, "min_request_interval")}
          on:input={(e) => setField(cfg.rate_limit, "min_request_interval", parseFloat(e.target.value) || 2)} /></div>
        <div class="field"><label for="cfg-rl-retries">最大重试次数</label><input id="cfg-rl-retries" type="number" value={numField(cfg.rate_limit, "max_retries")}
          on:input={(e) => setField(cfg.rate_limit, "max_retries", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-rl-delay">重试延迟（秒）</label><input id="cfg-rl-delay" type="number" value={numField(cfg.rate_limit, "retry_delay")}
          on:input={(e) => setField(cfg.rate_limit, "retry_delay", Number(e.target.value))} /></div>

      {:else if activeTab === "cache"}
        <h2>视频缓存</h2>
        <div class="field"><label for="cfg-vc-expire">视频缓存过期（秒）</label><input id="cfg-vc-expire" type="number" value={numField(cfg.video_cache, "expire_time")}
          on:input={(e) => setField(cfg.video_cache, "expire_time", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-vc-file">视频缓存文件</label><input id="cfg-vc-file" type="text" value={field(cfg.video_cache, "cache_file")}
          on:input={(e) => setField(cfg.video_cache, "cache_file", e.target.value)} /></div>

      {:else if activeTab === "logging"}
        <h2>日志设置</h2>
        <div class="field"><label for="cfg-log-level">日志级别</label>
          <select id="cfg-log-level" value={field(cfg.logging, "level")}
            on:change={(e) => setField(cfg.logging, "level", e.target.value)}>
            <option>DEBUG</option><option>INFO</option><option>WARN</option><option>ERROR</option>
          </select></div>
        <div class="field"><label for="cfg-log-file">日志文件</label><input id="cfg-log-file" type="text" value={field(cfg.logging, "file")}
          on:input={(e) => setField(cfg.logging, "file", e.target.value)} /></div>
        <label class="checkbox"><input type="checkbox" checked={boolField(cfg.logging, "console")}
          on:change={(e) => setField(cfg.logging, "console", e.target.checked)} /> 输出到控制台</label>

      {:else if activeTab === "ai"}
        <h2>AI 引擎与用量</h2>
        <div class="field"><label for="cfg-ai-provider">主提供商</label>
          <select id="cfg-ai-provider" value={cfg.ai?.provider || "deepseek"}
            on:change={(e) => { cfg.ai.provider = e.target.value; }} >
            <option value="deepseek">DeepSeek（云端）</option>
            <option value="ollama">Ollama（本地）</option>
            <option value="openai">OpenAI 兼容端点</option>
          </select></div>
        <div class="field"><label for="cfg-ai-fallback">失败回退 <span class="hint">（主提供商重试后仍失败时）</span></label>
          <select id="cfg-ai-fallback" value={cfg.ai?.fallback || "none"}
            on:change={(e) => { cfg.ai.fallback = e.target.value; }} >
            <option value="none">不启用</option>
            <option value="deepseek">回退到 DeepSeek</option>
            <option value="ollama">回退到 Ollama</option>
            <option value="openai">回退到 OpenAI 兼容端点</option>
          </select></div>
        <div class="field"><label for="cfg-ai-timeout">请求超时（秒） <span class="hint">（DeepSeek / OpenAI 兼容；Ollama 用各自的超时）</span></label>
          <input id="cfg-ai-timeout" type="number" value={numField(cfg.ai, "request_timeout_secs")}
            on:input={(e) => setField(cfg.ai, "request_timeout_secs", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-ai-budget">每日 token 预算 <span class="hint">（0=不限制；超限后当天停止调用 AI）</span></label>
          <input id="cfg-ai-budget" type="number" value={numField(cfg.ai, "daily_token_budget")}
            on:input={(e) => setField(cfg.ai, "daily_token_budget", Number(e.target.value))} /></div>
        <div class="field"><label for="cfg-ai-input-price">输入价格（元 / 百万 token，0=不估算）</label>
          <input id="cfg-ai-input-price" type="number" step="0.01" min="0" value={field(cfg.ai, "input_price_per_million")}
            on:input={(e) => setField(cfg.ai, "input_price_per_million", parseFloat(e.target.value) || 0)} /></div>
        <div class="field"><label for="cfg-ai-output-price">输出价格（元 / 百万 token，0=不估算）</label>
          <input id="cfg-ai-output-price" type="number" step="0.01" min="0" value={field(cfg.ai, "output_price_per_million")}
            on:input={(e) => setField(cfg.ai, "output_price_per_million", parseFloat(e.target.value) || 0)} /></div>
        <div class="field">
          <button type="button" class="btn-mini" on:click={handleTest} disabled={testing}>
            {testing ? "测试中..." : "保存并测试连接"}
          </button>
        </div>
        {#if testResult}
          <p class:test-ok={testResult.ok} class:test-err={!testResult.ok} class="test-result">
            {#if testResult.ok}
              连接成功：{testResult.provider} / {testResult.model}，{testResult.latency_ms} ms，返回 “{testResult.reply}”
            {:else}
              连接失败：{testResult.error}
            {/if}
          </p>
        {/if}
      {/if}
    </div>

    <div class="save-area">
      <button class="btn-save" on:click={handleSave}>保存配置</button>
    </div>
  </div>
{:else}
  <p>加载中...</p>
{/if}

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 16px; }
  h2 { font-size: 1.05rem; color: #8aa0b8; margin-bottom: 14px; padding-bottom: 8px; border-bottom: 1px solid #1e3a5f; }
  .sub-title { font-size: 0.88rem; color: #00b4d8; margin: 16px 0 10px; padding-top: 8px; border-top: 1px dashed #1e3a5f; }
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
  .hint-note { color: #5a7a9a; font-size: 0.72rem; line-height: 1.55; margin: -4px 0 14px; }
  .field input, .field textarea, .field select {
    width: 100%; padding: 8px 10px; border-radius: 6px;
    border: 1px solid #1e3a5f; background: #0d1b2a; color: #e0e8f0;
    font-size: 0.85rem; outline: none;
  }
  .field textarea { resize: vertical; }
  .field input:focus, .field textarea:focus, .field select:focus { border-color: #00b4d8; }
  .btn-mini {
    padding: 6px 14px; border: 1px solid #1e3a5f; border-radius: 6px;
    background: #16213e; color: #b0c4de; cursor: pointer; font-size: 0.8rem;
  }
  .btn-mini:hover { background: #1e3a5f; }
  .btn-mini:disabled { opacity: 0.5; cursor: not-allowed; }
  .model-chips { display: flex; flex-wrap: wrap; gap: 6px; margin: -6px 0 14px; }
  .chip {
    padding: 4px 10px; border: 1px solid #1e3a5f; border-radius: 14px;
    background: #0d1b2a; color: #8aa0b8; cursor: pointer; font-size: 0.75rem;
  }
  .chip.active { border-color: #00b4d8; color: #00b4d8; }
  .test-result { font-size: 0.8rem; line-height: 1.6; margin: -2px 0 14px; word-break: break-all; }
  .test-ok { color: #2ecc71; }
  .test-err { color: #e74c3c; }
  .checkbox { display: flex; align-items: center; gap: 8px; margin-bottom: 10px; font-size: 0.85rem; color: #c0d0e0; cursor: pointer; }
  .checkbox.dry { color: #f0c060; border-left: 3px solid #f0c060; padding-left: 10px; }
  .checkbox input { width: auto; accent-color: #00b4d8; }
  .save-area { margin-top: 20px; }
  .btn-save {
    padding: 10px 28px; border: none; border-radius: 8px;
    background: #00b4d8; color: #fff; font-weight: 600; font-size: 0.95rem;
    cursor: pointer; transition: 0.15s;
  }
  .btn-save:hover { opacity: 0.85; }
</style>
