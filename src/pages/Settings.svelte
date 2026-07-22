<script>
  import { onMount } from "svelte";
  import { setPassword, checkOllama, listOllamaModels, getConfig, clearAllData } from "../lib/api.js";
  import { showToast } from "../lib/stores.js";

  let pwdInput = "";
  let pwdConfirm = "";
  let ollamaStatus = "未检测";
  let ollamaModels = [];
  let checking = false;

  async function handleSetPwd() {
    if (pwdInput !== pwdConfirm) {
      showToast("error", "两次密码不一致");
      return;
    }
    try {
      await setPassword(pwdInput);
      showToast("success", pwdInput ? "密码已设置" : "密码已清除");
      pwdInput = "";
      pwdConfirm = "";
    } catch (e) {
      showToast("error", "设置失败: " + e);
    }
  }

  async function detectOllama() {
    checking = true;
    try {
      const ok = await checkOllama();
      ollamaStatus = ok ? "✅ 可用" : "❌ 不可用";
      if (ok) {
        ollamaModels = await listOllamaModels();
      }
    } catch (e) {
      ollamaStatus = "❌ 检测失败: " + e;
    }
    checking = false;
  }

  async function loadConfig() {
    try {
      const cfg = await getConfig();
      if (cfg.auth?.enabled) {
        pwdInput = "••••••"; // placeholder for existing
      }
    } catch (_) {}
  }

  onMount(loadConfig);

  // ── 清空数据 ──
  let clearing = false;
  let showClearConfirm = false;
  let confirmText = "";
  let clearResult = null;
  const CONFIRM_PHRASE = "确认清空";

  function openClearConfirm() {
    confirmText = "";
    clearResult = null;
    showClearConfirm = true;
  }

  function cancelClear() {
    showClearConfirm = false;
    confirmText = "";
    clearResult = null;
  }

  async function executeClear() {
    if (confirmText !== CONFIRM_PHRASE) {
      showToast("error", `请输入 "${CONFIRM_PHRASE}" 确认`);
      return;
    }
    clearing = true;
    try {
      clearResult = await clearAllData();
      showToast("success", `已清空 ${clearResult.trashed} 个文件到回收站，应用即将退出`);
    } catch (e) {
      clearResult = { error: String(e) };
      showToast("error", "清空失败: " + e);
    }
    clearing = false;
  }
</script>

<h1>🛠 系统设置</h1>

<div class="section">
  <h2>🔒 登录密码</h2>
  <p class="desc">设置后访问需要密码验证；留空则取消密码保护</p>
  <div class="form-row">
    <input type="password" placeholder="新密码" bind:value={pwdInput} />
    <input type="password" placeholder="确认密码" bind:value={pwdConfirm} />
    <button class="btn-save" on:click={handleSetPwd}>保存</button>
  </div>
</div>

{#if showClearConfirm}
  <div class="section danger-section">
    <h2>⚠️ 清空所有数据</h2>
    <p class="desc danger-desc">
      此操作将停止机器人、清空所有配置、回复历史、Cookie、日志文件到系统回收站。<br />
      清空后应用将自动退出。此操作不可撤销！
    </p>
    <div class="form-col">
      <label class="confirm-label">
        请输入 "<strong>{CONFIRM_PHRASE}</strong>" 以确认：
      </label>
      <input
        type="text"
        class="confirm-input"
        bind:value={confirmText}
        placeholder={CONFIRM_PHRASE}
        disabled={clearing}
      />
    </div>
    <div class="form-row">
      <button
        class="btn-danger"
        on:click={executeClear}
        disabled={clearing || confirmText !== CONFIRM_PHRASE}
      >
        {clearing ? "清空中..." : "确认清空"}
      </button>
      <button class="btn-secondary" on:click={cancelClear} disabled={clearing}>
        取消
      </button>
    </div>
    {#if clearResult}
      <div class="result-box">
        {#if clearResult.error}
          <p class="error-text">清空失败：{clearResult.error}</p>
        {:else}
          <p class="success-text">
            已移至回收站 {clearResult.trashed}/{clearResult.total} 个文件
            {#if clearResult.errors?.length}
              <br /><span class="warn-text">{clearResult.errors.length} 个文件失败: {clearResult.errors.join("; ")}</span>
            {/if}
          </p>
        {/if}
      </div>
    {/if}
  </div>
{:else}
  <div class="section">
    <h2>⚠️ 危险操作</h2>
    <p class="desc">清空所有运行时数据到系统回收站（配置、历史、Cookie、日志等）</p>
    <div class="form-row">
      <button class="btn-danger-outline" on:click={openClearConfirm}>
        🗑 清空所有数据...
      </button>
    </div>
  </div>
{/if}

<div class="section">
  <h2>🦙 Ollama 检测</h2>
  <p class="desc">检测本地 Ollama 服务是否可用及已安装的模型</p>
  <div class="form-row">
    <button class="btn-secondary" on:click={detectOllama} disabled={checking}>
      {checking ? "检测中..." : "🔍 检测"}
    </button>
    <span class="status-text">{ollamaStatus}</span>
  </div>
  {#if ollamaModels.length > 0}
    <div class="model-list">
      <span class="label">可用模型:</span>
      {#each ollamaModels as m}
        <span class="model">{m}</span>
      {/each}
    </div>
  {/if}
</div>

<div class="section">
  <h2>ℹ️ 关于</h2>
  <div class="about">
    <p>BiliCommentBot-RS v0.1.3</p>
    <p class="sub">Rust + Tauri + Svelte 构建 · Windows 桌面版</p>
    <p class="sub">基于 <a href="https://github.com/Janson20/BiliCommentBot" target="_blank">BiliCommentBot</a> 移植</p>
  </div>
</div>

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 20px; }
  .section {
    margin-bottom: 28px; padding-bottom: 20px; border-bottom: 1px solid #1e3a5f;
  }
  h2 { font-size: 1.05rem; color: #8aa0b8; margin-bottom: 8px; }
  .desc { font-size: 0.82rem; color: #5a7a9a; margin-bottom: 12px; }
  .form-row { display: flex; gap: 10px; align-items: center; }
  .form-row input {
    padding: 8px 12px; border-radius: 6px; border: 1px solid #1e3a5f;
    background: #0d1b2a; color: #e0e8f0; font-size: 0.85rem; outline: none;
    width: 200px;
  }
  .form-row input:focus { border-color: #00b4d8; }
  .btn-save {
    padding: 8px 20px; border: none; border-radius: 6px;
    background: #00b4d8; color: #fff; font-weight: 600; cursor: pointer;
  }
  .btn-secondary {
    padding: 8px 18px; border: 1px solid #1e3a5f; border-radius: 6px;
    background: #16213e; color: #b0c4de; cursor: pointer; font-size: 0.85rem;
  }
  .btn-secondary:hover { background: #1e3a5f; }
  .btn-secondary:disabled { opacity: 0.5; }
  .status-text { color: #b0c4de; font-size: 0.85rem; }
  .model-list { margin-top: 10px; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .model-list .label { color: #8aa0b8; font-size: 0.8rem; }
  .model {
    background: #1e3a5f; color: #00b4d8; padding: 3px 10px; border-radius: 12px;
    font-size: 0.78rem;
  }
  .about p { color: #b0c4de; font-size: 0.9rem; }
  .about .sub { color: #5a7a9a; font-size: 0.8rem; margin-top: 2px; }
  .about a { color: #00b4d8; }

  /* ── 危险操作区 ── */
  .danger-section { border-color: #8b0000; background: rgba(139, 0, 0, 0.06); border-radius: 8px; padding: 16px; }
  .danger-desc { color: #e07070 !important; }
  .btn-danger-outline {
    padding: 8px 18px; border: 1px solid #c0392b; border-radius: 6px;
    background: transparent; color: #e74c3c; cursor: pointer; font-size: 0.85rem;
  }
  .btn-danger-outline:hover { background: rgba(231, 76, 60, 0.15); }
  .btn-danger {
    padding: 8px 20px; border: none; border-radius: 6px;
    background: #c0392b; color: #fff; font-weight: 600; cursor: pointer;
  }
  .btn-danger:hover { background: #e74c3c; }
  .btn-danger:disabled { opacity: 0.4; cursor: not-allowed; }
  .form-col { display: flex; flex-direction: column; gap: 6px; margin-bottom: 12px; }
  .confirm-label { font-size: 0.82rem; color: #b0c4de; }
  .confirm-label strong { color: #e74c3c; }
  .confirm-input {
    padding: 8px 12px; border-radius: 6px; border: 1px solid #c0392b;
    background: #0d1b2a; color: #e0e8f0; font-size: 0.85rem; outline: none;
    width: 280px;
  }
  .confirm-input:focus { border-color: #e74c3c; }
  .result-box { margin-top: 10px; padding: 10px; border-radius: 6px; background: #16213e; }
  .success-text { color: #27ae60; font-size: 0.82rem; }
  .error-text { color: #e74c3c; font-size: 0.82rem; }
  .warn-text { color: #f39c12; font-size: 0.78rem; }
</style>
