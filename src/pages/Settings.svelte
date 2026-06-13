<script>
  import { onMount } from "svelte";
  import { setPassword, checkOllama, listOllamaModels, getConfig } from "../lib/api.js";
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
    <p>BiliCommentBot-RS v0.1.0</p>
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
</style>
