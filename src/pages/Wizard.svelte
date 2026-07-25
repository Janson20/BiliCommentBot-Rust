<script>
  import { onDestroy, createEventDispatcher } from "svelte";
  import { open } from "@tauri-apps/api/dialog";
  import {
    migrateFromOld, generateQrcode, pollQrLogin, verifyCookie,
    setCookieManually, refreshCookie, getConfig, saveConfig,
    checkOllama, listOllamaModels, setPassword
  } from "../lib/api.js";
  import { showToast, loginStatus, config as cfgStore } from "../lib/stores.js";

  const dispatch = createEventDispatcher();

  // ── 向导模式: "fresh" | "migrate" ──
  let mode = null; // null = 选择界面
  let step = 0;     // 当前步骤
  let maxStep = 4;  // migrate 模式下只有 2 步
  let loading = false;

  // ── Step 0: Welcome ──
  function selectFresh() {
    mode = "fresh";
    step = 1;
    maxStep = 4;
  }
  function selectMigrate() {
    mode = "migrate";
    step = 1;
    maxStep = 2;
  }

  // ════════════════════════════════════════════════
  //  Step 1a: 迁移模式
  // ════════════════════════════════════════════════
  let oldDir = "";
  let migrating = false;
  let migrateResult = null;

  async function browseFolder() {
    try {
      const selected = await open({
        directory: true, multiple: false,
        title: "选择旧版 BiliCommentBot 项目文件夹"
      });
      if (selected && typeof selected === "string") {
        oldDir = selected;
      }
    } catch (e) {
      showToast("error", "选择文件夹失败: " + e);
    }
  }

  async function doMigrate() {
    if (!oldDir) { showToast("error", "请先选择项目文件夹"); return; }
    migrating = true;
    try {
      migrateResult = await migrateFromOld(oldDir);
      step = 2; // done
    } catch (e) {
      showToast("error", "迁移失败: " + e);
    }
    migrating = false;
  }

  // ════════════════════════════════════════════════
  //  Step 1b: 登录 (扫码 + 手动)
  // ════════════════════════════════════════════════
  let loginMethod = "qr";       // "qr" | "manual"
  let qrBase64 = "";
  let qrcodeKey = "";
  let polling = false;
  let pollMsg = "";
  let pollTimer = null;
  let manualCookie = "";
  let manualRefreshToken = "";
  let loginDone = false;
  let userUname = "";
  let userUid = "";

  // 进入时自动检测已有 Cookie
  async function checkExistingLogin() {
    try {
      const r = await verifyCookie();
      if (r.valid) {
        userUname = r.uname || "";
        userUid = r.uid || "";
        loginDone = true;
      }
    } catch (_) {}
  }

  async function startQrLogin() {
    try {
      const r = await generateQrcode();
      qrBase64 = r.qrcode_base64;
      qrcodeKey = r.qrcode_key;
      startPolling();
    } catch (e) {
      showToast("error", "获取二维码失败: " + e);
    }
  }

  function startPolling() {
    polling = true;
    pollMsg = "请使用 B 站 APP 扫描二维码...";
    poll();
  }

  function stopPolling() {
    polling = false;
    if (pollTimer) clearTimeout(pollTimer);
  }

  async function poll() {
    if (!polling || !qrcodeKey) return;
    try {
      const r = await pollQrLogin(qrcodeKey);
      if (r.code === 0) {
        stopPolling();
        // 获取用户名
        const verify = await verifyCookie();
        if (verify.valid) {
          userUname = verify.uname || "";
          userUid = verify.uid || "";
          loginStatus.set({ loggedIn: true, uname: userUname, uid: userUid });
        }
        loginDone = true;
        showToast("success", "扫码登录成功！");
        return;
      }
      if (r.code === 86038) {
        pollMsg = "二维码已过期，请点击重新生成";
        stopPolling(); return;
      }
      if (r.code === 86090 || r.code === 86101) {
        pollMsg = "已扫描，请在手机上确认登录...";
      }
    } catch (_) {
      pollMsg = "轮询出错，请重试";
      stopPolling(); return;
    }
    pollTimer = setTimeout(poll, 2000);
  }

  async function handleManualSubmit() {
    if (!manualCookie) { showToast("error", "请输入 Cookie"); return; }
    loading = true;
    try {
      await setCookieManually(manualCookie, manualRefreshToken || null);
      const verify = await verifyCookie();
      if (verify.valid) {
        userUname = verify.uname || "";
        userUid = verify.uid || "";
        loginStatus.set({ loggedIn: true, uname: userUname, uid: userUid });
        loginDone = true;
        showToast("success", "Cookie 已保存并验证通过");
      } else {
        showToast("error", "Cookie 无效或已过期");
      }
    } catch (e) {
      showToast("error", "保存失败: " + e);
    }
    loading = false;
  }

  // ════════════════════════════════════════════════
  //  Step 2: AI 提供商
  // ════════════════════════════════════════════════
  let aiProvider = "deepseek";
  let deepseekApiKey = "";
  let deepseekModel = "deepseek-v4-flash";
  let ollamaBaseUrl = "http://127.0.0.1:11434";
  let ollamaModel = "qwen2.5:7b";
  let ollamaChecking = false;
  let ollamaAvailable = false;
  let ollamaModels = [];

  async function detectOllama() {
    ollamaChecking = true;
    ollamaAvailable = false;
    ollamaModels = [];
    try {
      const ok = await checkOllama();
      if (ok) {
        ollamaAvailable = true;
        ollamaModels = await listOllamaModels();
      }
    } catch (_) {}
    ollamaChecking = false;
  }

  async function saveAiConfig() {
    try {
      const cfg = await getConfig();
      cfg.ai = cfg.ai || {};
      cfg.ai.provider = aiProvider;
      if (aiProvider === "deepseek") {
        cfg.deepseek = cfg.deepseek || {};
        cfg.deepseek.api_key = deepseekApiKey;
        cfg.deepseek.model = deepseekModel || "deepseek-v4-flash";
      } else {
        cfg.ollama = cfg.ollama || {};
        cfg.ollama.base_url = ollamaBaseUrl;
        cfg.ollama.model = ollamaModel || "qwen2.5:7b";
      }
      await saveConfig(cfg);
      cfgStore.set(cfg);
      return true;
    } catch (e) {
      showToast("error", "保存 AI 配置失败: " + e);
      return false;
    }
  }

  // ════════════════════════════════════════════════
  //  Step 3: 回复设置
  // ════════════════════════════════════════════════
  let replyEnabled = true;
  let replyPrefix = "";
  let replyMaxProcess = 10;
  let replyLikeEnabled = false;
  let replyChainedEnabled = true;

  async function saveReplyConfig() {
    try {
      const cfg = await getConfig();
      cfg.reply = cfg.reply || {};
      cfg.reply.enabled = replyEnabled;
      cfg.reply.prefix = replyPrefix;
      cfg.reply.max_process = replyMaxProcess;
      cfg.reply.like_enabled = replyLikeEnabled;
      cfg.reply.chained_reply_enabled = replyChainedEnabled;
      await saveConfig(cfg);
      cfgStore.set(cfg);
      return true;
    } catch (e) {
      showToast("error", "保存回复配置失败: " + e);
      return false;
    }
  }

  // ════════════════════════════════════════════════
  //  Step 4: 安全设置
  // ════════════════════════════════════════════════
  let pwdInput = "";
  let pwdConfirm = "";

  async function handleSetPwd() {
    if (pwdInput !== pwdConfirm) {
      showToast("error", "两次密码不一致");
      return false;
    }
    try {
      await setPassword(pwdInput);
      showToast("success", pwdInput ? "密码已设置" : "密码已清除");
      return true;
    } catch (e) {
      showToast("error", "设置失败: " + e);
      return false;
    }
  }

  // ════════════════════════════════════════════════
  //  导航
  // ════════════════════════════════════════════════
  async function goNext() {
    // 验证当前步骤
    if (mode === "fresh") {
      if (step === 1 && !loginDone) {
        showToast("error", "请先完成登录"); return;
      }
      if (step === 2) {
        if (aiProvider === "deepseek" && !deepseekApiKey) {
          showToast("error", "请输入 DeepSeek API Key"); return;
        }
        if (aiProvider === "ollama" && !ollamaAvailable) {
          showToast("error", "请先检测 Ollama 服务是否可用"); return;
        }
        await saveAiConfig();
      }
      if (step === 3) {
        await saveReplyConfig();
      }
    }
    if (mode === "migrate" && step === 1) {
      // migrate step handled by doMigrate
      return;
    }
    step += 1;
  }

  function goPrev() {
    if (step > 0) step -= 1;
  }

  // 跳过可选步骤
  function skipStep() {
    step += 1;
  }

  function doneWizard() {
    dispatch("done");
  }

  // ── 进度标签 ──
  const freshStepLabels = ["登录B站", "AI引擎", "回复设置", "安全设置", "完成"];
  const migrateStepLabels = ["选择文件夹", "迁移", "完成"];

  $: stepLabels = mode === "migrate" ? migrateStepLabels : freshStepLabels;

  onDestroy(() => stopPolling());
</script>

<!-- ═══════════════════════════════════════════════════════════
  模板
  ═══════════════════════════════════════════════════════════ -->

<div class="wizard-root">
  <!-- ── 进度条 ── -->
  {#if mode}
    <div class="progress-bar">
      {#each stepLabels as label, i}
        <div class="progress-step" class:done={i < step} class:active={i === step}>
          <div class="step-dot">{i < step ? "✓" : i + 1}</div>
          <div class="step-label">{label}</div>
          {#if i < stepLabels.length - 1}
            <div class="step-line" class:filled={i < step}></div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- ═══════════════════════════════════════════════════
    Step 0: 欢迎 / 选择模式
    ═══════════════════════════════════════════════════ -->
  {#if step === 0}
    <div class="step-card">
      <div class="welcome-icon">🚀</div>
      <h1>欢迎使用 BiliCommentBot-RS</h1>
      <p class="sub">B 站评论自动回复机器人 · Rust 桌面版</p>
      <p class="desc">检测到这是你第一次使用，请选择配置方式。整个过程只需 <strong>2~3 分钟</strong>。</p>
      <div class="mode-options">
        <button class="mode-card" on:click={selectMigrate}>
          <span class="mode-icon">📂</span>
          <span class="mode-title">从旧版迁移</span>
          <span class="mode-desc">从 Python 版 BiliCommentBot 项目文件夹<br>一键导入所有配置、Cookie 和历史记录</span>
        </button>
        <button class="mode-card" on:click={selectFresh}>
          <span class="mode-icon">🆕</span>
          <span class="mode-title">全新配置</span>
          <span class="mode-desc">分步引导完成<br>B 站登录 → AI 引擎 → 回复设置</span>
        </button>
      </div>
    </div>

  <!-- ═══════════════════════════════════════════════════
    Step 1 (migrate): 选择文件夹
    ═══════════════════════════════════════════════════ -->
  {:else if mode === "migrate" && step === 1}
    <div class="step-card">
      <h1>📂 选择旧版项目文件夹</h1>
      <p class="desc">选择包含 <code>config.toml</code>、<code>history.json</code>、<code>bilibili_cookie.json</code>、<code>video_cache.json</code> 的文件夹</p>
      <div class="dir-row">
        <input type="text" placeholder="点击右侧按钮选择文件夹..." bind:value={oldDir} readonly />
        <button class="btn-outline" on:click={browseFolder}>📁 浏览...</button>
      </div>
      {#if migrateResult}
        <div class="migrate-result">
          <p>✅ 成功迁移 {migrateResult.migrated_count} 个项目</p>
          {#if migrateResult.errors?.length}
            <div class="migrate-errors">
              <p>以下问题需要注意：</p>
              {#each migrateResult.errors as err}
                <div class="err-item">⚠ {err}</div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
      <div class="step-actions">
        <button class="btn-outline" on:click={() => { mode = null; step = 0; }}>← 返回</button>
        {#if !migrateResult}
          <button class="btn-primary" on:click={doMigrate} disabled={migrating || !oldDir}>
            {migrating ? "⏳ 迁移中..." : "开始迁移"}
          </button>
        {:else}
          <button class="btn-primary" on:click={doneWizard}>完成 →</button>
        {/if}
      </div>
    </div>

  <!-- ═══════════════════════════════════════════════════
    Step 1 (fresh): 登录 B 站
    ═══════════════════════════════════════════════════ -->
  {:else if mode === "fresh" && step === 1}
    <div class="step-card">
      <h1>🔑 登录 B 站</h1>
      <p class="desc">使用 B 站 APP 扫码登录，或手动输入 Cookie 字符串</p>

      {#if loginDone}
        <div class="login-success">
          <span class="badge-green">✅ 已登录</span>
          <div class="user-row"><span>用户名</span><strong>{userUname || "—"}</strong></div>
          <div class="user-row"><span>UID</span><strong>{userUid || "—"}</strong></div>
        </div>
      {:else}
        <div class="login-tabs">
          <button class="tab" class:active={loginMethod === "qr"} on:click={() => (loginMethod = "qr")}>📱 扫码登录</button>
          <button class="tab" class:active={loginMethod === "manual"} on:click={() => (loginMethod = "manual")}>✏️ 手动输入</button>
        </div>

        {#if loginMethod === "qr"}
          <div class="qr-area">
            {#if qrBase64}
              <div class="qr-wrap">
                <img src={qrBase64} alt="QR Code" />
                {#if pollMsg}
                  <div class="poll-text">{pollMsg}</div>
                {/if}
              </div>
            {:else}
              <button class="btn-primary" on:click={startQrLogin}>📱 生成二维码</button>
            {/if}
            <p class="hint">打开 B 站 APP → 我的 → 扫一扫</p>
          </div>
        {:else}
          <div class="manual-form">
            <label>Cookie 字符串
              <textarea bind:value={manualCookie} placeholder="SESSDATA=xxx; bili_jct=xxx; DedeUserID=xxx; ..." rows="3"></textarea>
            </label>
            <label>Refresh Token（可选）
              <input type="text" bind:value={manualRefreshToken} placeholder="刷新令牌" />
            </label>
            <button class="btn-primary" on:click={handleManualSubmit} disabled={loading}>
              {loading ? "验证中..." : "💾 保存并验证"}
            </button>
          </div>
        {/if}
      {/if}

      <div class="step-actions">
        <button class="btn-outline" on:click={() => { mode = null; step = 0; }}>← 返回</button>
        <button class="btn-primary" on:click={goNext} disabled={!loginDone}>下一步 →</button>
      </div>
    </div>

  <!-- ═══════════════════════════════════════════════════
    Step 2 (fresh): AI 引擎
    ═══════════════════════════════════════════════════ -->
  {:else if mode === "fresh" && step === 2}
    <div class="step-card">
      <h1>🧠 选择 AI 引擎</h1>
      <p class="desc">选择回复评论使用的 AI 模型。后续可在「配置」页面随时切换。</p>

      <div class="ai-tabs">
        <button class="ai-tab" class:active={aiProvider === "deepseek"} on:click={() => (aiProvider = "deepseek")}>
          <span class="ai-icon">☁️</span> DeepSeek（云端）
        </button>
        <button class="ai-tab" class:active={aiProvider === "ollama"} on:click={() => (aiProvider = "ollama")}>
          <span class="ai-icon">🦙</span> Ollama（本地）
        </button>
      </div>

      {#if aiProvider === "deepseek"}
        <div class="ai-config">
          <div class="field">
            <label>API Key <span class="required">*</span></label>
            <input type="password" bind:value={deepseekApiKey} placeholder="sk-xxxxxxxxxxxxxxxx" />
            <span class="field-hint">在 <a href="https://platform.deepseek.com/api_keys" target="_blank">platform.deepseek.com</a> 获取</span>
          </div>
          <div class="field">
            <label>模型</label>
            <input type="text" bind:value={deepseekModel} placeholder="deepseek-v4-flash" />
          </div>
        </div>
      {:else}
        <div class="ai-config">
          <p class="hint">需要先安装 <a href="https://ollama.com" target="_blank">Ollama</a> 并拉取模型</p>
          <div class="field">
            <label>服务地址</label>
            <input type="text" bind:value={ollamaBaseUrl} placeholder="http://127.0.0.1:11434" />
          </div>
          <div class="field">
            <label>模型名</label>
            <input type="text" bind:value={ollamaModel} placeholder="qwen2.5:7b" />
          </div>
          <div class="ollama-detect">
            <button class="btn-outline" on:click={detectOllama} disabled={ollamaChecking}>
              {ollamaChecking ? "⏳ 检测中..." : "🔍 检测 Ollama 服务"}
            </button>
            {#if ollamaAvailable}
              <span class="status-ok">✅ 服务可用</span>
              {#if ollamaModels.length > 0}
                <div class="model-tags">
                  {#each ollamaModels as m}
                    <span class="model-tag" on:click={() => (ollamaModel = m)} class:selected={ollamaModel === m}>{m}</span>
                  {/each}
                </div>
              {/if}
            {:else if !ollamaChecking && ollamaAvailable === false}
              <span class="status-err">❌ 不可用，请确认 Ollama 已启动</span>
            {/if}
          </div>
        </div>
      {/if}

      <div class="step-actions">
        <button class="btn-outline" on:click={goPrev}>← 上一步</button>
        <button class="btn-primary" on:click={goNext}>下一步 →</button>
      </div>
    </div>

  <!-- ═══════════════════════════════════════════════════
    Step 3 (fresh): 回复设置
    ═══════════════════════════════════════════════════ -->
  {:else if mode === "fresh" && step === 3}
    <div class="step-card">
      <h1>💬 回复设置</h1>
      <p class="desc">配置自动回复行为。高级选项请在后续「配置」页面中调整。</p>

      <div class="reply-form">
        <label class="checkbox-row">
          <input type="checkbox" bind:checked={replyEnabled} /> 启用自动回复
        </label>
        <div class="field">
          <label>回复前缀</label>
          <input type="text" bind:value={replyPrefix} placeholder="可选，AI 生成的回复将添加此前缀" />
        </div>
        <div class="field">
          <label>每次最多处理评论数</label>
          <input type="number" bind:value={replyMaxProcess} min="1" max="50" />
        </div>
        <label class="checkbox-row">
          <input type="checkbox" bind:checked={replyChainedEnabled} /> 启用楼中楼链式回复
        </label>
        <label class="checkbox-row">
          <input type="checkbox" bind:checked={replyLikeEnabled} /> 回复后自动点赞评论
        </label>
      </div>

      <div class="step-actions">
        <button class="btn-outline" on:click={goPrev}>← 上一步</button>
        <button class="btn-subtle" on:click={skipStep}>跳过 →</button>
        <button class="btn-primary" on:click={goNext}>下一步 →</button>
      </div>
    </div>

  <!-- ═══════════════════════════════════════════════════
    Step 4 (fresh): 安全设置
    ═══════════════════════════════════════════════════ -->
  {:else if mode === "fresh" && step === 4}
    <div class="step-card">
      <h1>🔒 安全设置（可选）</h1>
      <p class="desc">设置登录密码保护机器人。留空则无需密码直接访问。</p>

      <div class="pwd-form">
        <div class="field">
          <label>登录密码</label>
          <input type="password" bind:value={pwdInput} placeholder="留空跳过" />
        </div>
        <div class="field">
          <label>确认密码</label>
          <input type="password" bind:value={pwdConfirm} placeholder="再次输入" />
        </div>
      </div>

      <div class="step-actions">
        <button class="btn-outline" on:click={goPrev}>← 上一步</button>
        <button class="btn-subtle" on:click={skipStep}>跳过 →</button>
        <button class="btn-primary" on:click={goNext}>下一步 →</button>
      </div>
    </div>

  <!-- ═══════════════════════════════════════════════════
    Step 5 (fresh) / Step 2 (migrate): 完成
    ═══════════════════════════════════════════════════ -->
  {:else if (mode === "fresh" && step === 5) || (mode === "migrate" && step === 2)}
    <div class="step-card done-card">
      <div class="done-icon">🎉</div>
      <h1>设置完成！</h1>
      <p class="desc">所有必要配置已完成，可以开始使用了。</p>

      <div class="summary">
        <h3>配置摘要</h3>
        <div class="summary-row"><span>B 站登录</span><span class="val-ok">✅ 已登录</span></div>
        {#if mode === "fresh"}
          <div class="summary-row"><span>AI 引擎</span><span>{aiProvider === "deepseek" ? "DeepSeek（云端）" : "Ollama（本地）"}</span></div>
          <div class="summary-row"><span>自动回复</span><span>{replyEnabled ? "✅ 启用" : "⛔ 关闭"}</span></div>
          <div class="summary-row"><span>密码保护</span><span>{pwdInput ? "🔒 已设置" : "— 未设置"}</span></div>
        {/if}
        {#if mode === "migrate" && migrateResult}
          <div class="summary-row"><span>迁移项目数</span><span>{migrateResult.migrated_count} 个文件</span></div>
        {/if}
      </div>

      <p class="next-hint">提示：启动前可在「配置」页面查看和调整全部设置，在「设置」页面可设置密码和检测 Ollama。</p>
      <button class="btn-primary btn-large" on:click={doneWizard}>🚀 开始使用</button>
    </div>
  {/if}
</div>

<style>
  /* ── 根容器 ── */
  .wizard-root {
    width: 100%; max-width: 640px; margin: 0 auto;
    display: flex; flex-direction: column; align-items: center;
    padding: 20px 0;
  }

  /* ── 步骤卡片 ── */
  .step-card {
    width: 100%; background: #16213e; border: 1px solid #1e3a5f;
    border-radius: 14px; padding: 32px 30px;
  }
  .step-card h1 { font-size: 1.35rem; color: #00b4d8; margin-bottom: 8px; }
  .step-card .desc { color: #8aa0b8; font-size: 0.88rem; margin-bottom: 20px; line-height: 1.5; }
  .step-card .sub { color: #5a7a9a; text-align: center; font-size: 0.85rem; margin-bottom: 6px; }
  .step-card .hint { color: #5a7a9a; font-size: 0.78rem; margin-top: 8px; }
  .step-card .hint a { color: #00b4d8; }
  .step-card code { background: #0d1b2a; padding: 2px 6px; border-radius: 4px; font-size: 0.82rem; color: #f0c040; }

  /* ── 进度条 ── */
  .progress-bar {
    display: flex; align-items: flex-start; justify-content: center;
    gap: 0; margin-bottom: 24px; width: 100%; max-width: 560px;
  }
  .progress-step {
    display: flex; flex-direction: column; align-items: center;
    position: relative; flex: 1; min-width: 0;
  }
  .step-dot {
    width: 28px; height: 28px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    font-size: 0.75rem; font-weight: 700; flex-shrink: 0;
    background: #1a2a4a; border: 2px solid #1e3a5f; color: #5a7a9a;
  }
  .progress-step.active .step-dot {
    background: #00b4d8; border-color: #00b4d8; color: #fff;
  }
  .progress-step.done .step-dot {
    background: #2ecc71; border-color: #2ecc71; color: #fff;
  }
  .step-label {
    font-size: 0.7rem; color: #5a7a9a; margin-top: 5px;
    text-align: center; white-space: nowrap;
  }
  .progress-step.active .step-label { color: #00b4d8; font-weight: 600; }
  .progress-step.done .step-label { color: #2ecc71; }
  .step-line {
    position: absolute; top: 14px; left: 50%; width: 100%;
    height: 2px; background: #1e3a5f; z-index: -1;
  }
  .step-line.filled { background: #2ecc71; }

  /* ── Welcome 模式选择 ── */
  .welcome-icon { font-size: 3rem; text-align: center; margin-bottom: 8px; }
  .mode-options { display: flex; gap: 14px; margin-top: 16px; }
  .mode-card {
    flex: 1; background: #1a2a4a; border: 1px solid #1e3a5f; border-radius: 10px;
    padding: 22px 16px; cursor: pointer; transition: 0.2s;
    display: flex; flex-direction: column; align-items: center; gap: 8px;
    color: #c0d0e0; font-size: 0.85rem; text-align: center;
  }
  .mode-card:hover { border-color: #00b4d8; background: #1e3860; transform: translateY(-2px); }
  .mode-icon { font-size: 2.2rem; }
  .mode-title { font-weight: 600; font-size: 1rem; color: #e0e8f0; }
  .mode-desc { color: #8aa0b8; line-height: 1.45; }

  /* ── 导航按钮组 ── */
  .step-actions {
    display: flex; justify-content: space-between; align-items: center;
    margin-top: 24px; gap: 10px;
  }
  .btn-primary {
    padding: 10px 22px; border: none; border-radius: 8px;
    background: #00b4d8; color: #fff; font-weight: 600;
    font-size: 0.9rem; cursor: pointer; transition: 0.15s;
  }
  .btn-primary:hover { opacity: 0.85; }
  .btn-primary:disabled { opacity: 0.4; cursor: not-allowed; }
  .btn-outline {
    padding: 10px 18px; border: 1px solid #1e3a5f; border-radius: 8px;
    background: #0d1b2a; color: #b0c4de; font-size: 0.85rem;
    cursor: pointer; transition: 0.15s;
  }
  .btn-outline:hover { background: #1e3a5f; color: #e0e8f0; }
  .btn-outline:disabled { opacity: 0.5; cursor: not-allowed; }
  .btn-subtle {
    padding: 10px 18px; border: none; background: transparent;
    color: #5a7a9a; font-size: 0.85rem; cursor: pointer; transition: 0.15s;
  }
  .btn-subtle:hover { color: #8aa0b8; }
  .btn-large { padding: 14px 32px; font-size: 1rem; }

  /* ── 迁移 ── */
  .dir-row { display: flex; gap: 8px; margin-bottom: 16px; }
  .dir-row input {
    flex: 1; padding: 10px 14px; border-radius: 8px;
    border: 1px solid #1e3a5f; background: #0d1b2a; color: #e0e8f0;
    font-size: 0.85rem; outline: none;
  }
  .migrate-result { margin-top: 12px; }
  .migrate-result p { color: #2ecc71; font-size: 0.88rem; }
  .migrate-errors { margin-top: 8px; }
  .migrate-errors p { color: #e74c3c; font-size: 0.82rem; margin-bottom: 4px; }
  .err-item { padding: 3px 0; color: #f0c040; font-size: 0.8rem; }

  /* ── 登录 ── */
  .login-success {
    background: #1a2a4a; border: 1px solid #1e3a5f; border-radius: 10px;
    padding: 16px; margin-bottom: 16px; display: flex; flex-direction: column; gap: 8px;
  }
  .badge-green { color: #2ecc71; font-weight: 600; font-size: 0.9rem; }
  .user-row { display: flex; justify-content: space-between; font-size: 0.85rem; }
  .user-row span { color: #8aa0b8; }
  .user-row strong { color: #e0e8f0; }
  .login-tabs { display: flex; gap: 0; margin-bottom: 16px; }
  .tab {
    flex: 1; padding: 8px 16px; border: 1px solid #1e3a5f;
    background: #0d1b2a; color: #8aa0b8; cursor: pointer;
    font-size: 0.85rem; text-align: center;
  }
  .tab:first-child { border-radius: 8px 0 0 8px; }
  .tab:last-child { border-radius: 0 8px 8px 0; }
  .tab.active { background: #00b4d8; color: #fff; border-color: #00b4d8; }
  .qr-area { text-align: center; padding: 16px 0; }
  .qr-wrap { display: inline-block; }
  .qr-wrap img { width: 180px; height: 180px; border-radius: 10px; background: #fff; padding: 6px; }
  .poll-text { margin-top: 8px; color: #00b4d8; font-size: 0.82rem; }
  .manual-form { display: flex; flex-direction: column; gap: 12px; }
  .manual-form label { font-size: 0.82rem; color: #8aa0b8; display: flex; flex-direction: column; gap: 4px; }

  /* ── AI ── */
  .ai-tabs { display: flex; gap: 0; margin-bottom: 20px; }
  .ai-tab {
    flex: 1; padding: 12px 16px; border: 1px solid #1e3a5f;
    background: #0d1b2a; color: #8aa0b8; cursor: pointer;
    font-size: 0.9rem; display: flex; align-items: center; justify-content: center; gap: 6px;
    transition: 0.15s;
  }
  .ai-tab:first-child { border-radius: 10px 0 0 10px; }
  .ai-tab:last-child { border-radius: 0 10px 10px 0; }
  .ai-tab:hover { background: #1a2a4a; }
  .ai-tab.active { background: #00b4d8; color: #fff; border-color: #00b4d8; }
  .ai-icon { font-size: 1.1rem; }
  .ai-config { padding: 8px 0; }
  .ollama-detect { margin-top: 10px; display: flex; flex-direction: column; gap: 8px; }
  .status-ok { color: #2ecc71; font-size: 0.85rem; }
  .status-err { color: #e74c3c; font-size: 0.85rem; }
  .model-tags { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  .model-tag {
    padding: 4px 12px; border-radius: 14px; background: #1a2a4a;
    border: 1px solid #1e3a5f; color: #8aa0b8; font-size: 0.78rem;
    cursor: pointer; transition: 0.15s;
  }
  .model-tag:hover { border-color: #00b4d8; color: #e0e8f0; }
  .model-tag.selected { background: #00b4d8; color: #fff; border-color: #00b4d8; }

  /* ── 通用控件 ── */
  .field { margin-bottom: 14px; }
  .field label { display: block; font-size: 0.84rem; color: #8aa0b8; margin-bottom: 5px; }
  .field .required { color: #e74c3c; }
  .field-hint { display: block; font-size: 0.74rem; color: #5a7a9a; margin-top: 3px; }
  .field-hint a { color: #00b4d8; }
  input[type="text"], input[type="password"], input[type="number"], textarea, select {
    width: 100%; padding: 9px 12px; border-radius: 8px;
    border: 1px solid #1e3a5f; background: #0d1b2a; color: #e0e8f0;
    font-size: 0.88rem; outline: none; font-family: "Microsoft YaHei", "PingFang SC", "Consolas", monospace;
  }
  input:focus, textarea:focus, select:focus { border-color: #00b4d8; }
  textarea { resize: vertical; }
  .checkbox-row {
    display: flex; align-items: center; gap: 8px; margin-bottom: 12px;
    font-size: 0.88rem; color: #c0d0e0; cursor: pointer;
  }
  .checkbox-row input { width: auto; accent-color: #00b4d8; transform: scale(1.1); }

  /* ── 回复 ── */
  .reply-form { padding: 4px 0; }

  /* ── 密码 ── */
  .pwd-form { padding: 4px 0; }

  /* ── 完成 ── */
  .done-card { text-align: center; }
  .done-icon { font-size: 3.5rem; margin-bottom: 4px; }
  .summary {
    text-align: left; background: #1a2a4a; border-radius: 10px;
    padding: 16px 20px; margin: 16px 0;
  }
  .summary h3 { font-size: 0.9rem; color: #8aa0b8; margin-bottom: 10px; }
  .summary-row {
    display: flex; justify-content: space-between; padding: 5px 0;
    font-size: 0.84rem; color: #b0c4de; border-bottom: 1px solid #1e3a5f;
  }
  .summary-row:last-child { border-bottom: none; }
  .val-ok { color: #2ecc71; font-weight: 600; }
  .next-hint {
    color: #5a7a9a; font-size: 0.78rem; line-height: 1.5;
    margin: 12px 0 20px;
  }
</style>
