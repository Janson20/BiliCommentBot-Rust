<script>
  import { onMount } from "svelte";
  import {
    setPassword,
    checkOllama,
    listOllamaModels,
    getConfig,
    saveConfig,
    clearAllData,
    getAutostartStatus,
    setAutostart,
  } from "../lib/api.js";
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

  // ── 启动与窗口 ──
  let closeAction = "ask";
  let autostartEnabled = false;
  let autostartSupported = true;
  let autostartCommand = "";
  let autostartBusy = false;
  let closeActionBusy = false;

  const CLOSE_ACTION_LABELS = {
    ask: "每次询问",
    tray: "最小化到托盘",
    exit: "直接退出程序",
  };

  async function loadConfig() {
    try {
      const cfg = await getConfig();
      if (cfg.auth?.enabled) {
        pwdInput = "••••••"; // placeholder for existing
      }
      closeAction = cfg.app?.close_action || "ask";
    } catch (_) {}

    // 开机自启状态以后端（注册表）为准
    try {
      const st = await getAutostartStatus();
      autostartEnabled = !!st?.enabled;
      autostartCommand = st?.launch_command || "";
      autostartSupported = st?.supported !== false;
    } catch (e) {
      autostartSupported = false;
      autostartCommand = "读取失败: " + e;
    }
  }

  async function toggleAutostart(e) {
    const next = e.target.checked;
    const prev = autostartEnabled;
    autostartEnabled = next;
    autostartBusy = true;
    try {
      const st = await setAutostart(next);
      autostartEnabled = !!st?.enabled;
      autostartCommand = st?.launch_command || autostartCommand;
      if (autostartEnabled === next) {
        showToast(
          "success",
          next ? "已开启开机自启（开机后静默运行在系统托盘）" : "已关闭开机自启"
        );
      } else {
        showToast("error", "系统未接受该设置，请检查注册表权限或安全软件拦截");
      }
    } catch (err) {
      autostartEnabled = prev;
      showToast("error", "设置失败: " + err);
    }
    autostartBusy = false;
  }

  async function changeCloseAction(e) {
    const next = e.target.value;
    const prev = closeAction;
    closeAction = next;
    closeActionBusy = true;
    try {
      const cfg = await getConfig();
      if (!cfg.app) cfg.app = {};
      cfg.app.close_action = next;
      await saveConfig(cfg);
      showToast("success", `关闭主窗口时：${CLOSE_ACTION_LABELS[next] || next}`);
    } catch (err) {
      closeAction = prev;
      showToast("error", "保存失败: " + err);
    }
    closeActionBusy = false;
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

<div class="section">
  <h2>🚀 启动与窗口</h2>
  <p class="desc">开机自启、关闭主窗口时的行为；关闭窗口后程序仍可在系统托盘后台运行</p>

  <label
    class="switch-row"
    class:disabled={!autostartSupported || autostartBusy}
  >
    <input
      type="checkbox"
      checked={autostartEnabled}
      disabled={!autostartSupported || autostartBusy}
      on:change={toggleAutostart}
    />
    <span class="switch-text">
      开机自启
      <span class="hint">
        开机后静默启动并进入系统托盘（不弹出窗口），机器人可立即在后台工作
      </span>
    </span>
  </label>

  {#if autostartCommand}
    <p class="cmd-hint">自启命令：<code>{autostartCommand}</code></p>
  {/if}

  <div class="form-row close-row">
    <span class="row-label">关闭主窗口时</span>
    <select
      value={closeAction}
      on:change={changeCloseAction}
      disabled={closeActionBusy}
    >
      <option value="ask">每次询问（推荐）</option>
      <option value="tray">最小化到系统托盘</option>
      <option value="exit">直接退出程序</option>
    </select>
  </div>

  <p class="hint block-hint">
    「每次询问」会在点击窗口关闭按钮时弹出系统对话框，可选择最小化到托盘或退出程序。<br />
    最小化到托盘后机器人继续在后台运行：左键单击托盘图标可重新打开窗口，右键菜单可退出程序。
  </p>
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
    <p>BiliCommentBot-RS v0.1.5</p>
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

  /* ── 启动与窗口 ── */
  .hint { color: #5a7a9a; font-size: 0.78rem; }
  .switch-row {
    display: flex; align-items: flex-start; gap: 10px; cursor: pointer;
    padding: 10px 12px; border: 1px solid #1e3a5f; border-radius: 8px;
    background: #16213e; max-width: 620px;
  }
  .switch-row.disabled { opacity: 0.6; cursor: not-allowed; }
  .switch-row input[type="checkbox"] {
    width: 16px; height: 16px; margin-top: 2px; flex: none;
    accent-color: #00b4d8; cursor: pointer;
  }
  .switch-row.disabled input[type="checkbox"] { cursor: not-allowed; }
  .switch-text {
    display: flex; flex-direction: column; gap: 3px;
    color: #e0e8f0; font-size: 0.9rem;
  }
  .cmd-hint {
    margin-top: 8px; font-size: 0.75rem; color: #5a7a9a;
    word-break: break-all; line-height: 1.6;
  }
  .cmd-hint code {
    background: #0d1b2a; border: 1px solid #1e3a5f; border-radius: 4px;
    padding: 2px 6px; color: #8aa0b8;
  }
  .close-row { margin-top: 16px; }
  .row-label { color: #8aa0b8; font-size: 0.85rem; }
  .form-row select {
    padding: 8px 12px; border-radius: 6px; border: 1px solid #1e3a5f;
    background: #0d1b2a; color: #e0e8f0; font-size: 0.85rem;
    outline: none; cursor: pointer;
  }
  .form-row select:focus { border-color: #00b4d8; }
  .form-row select:disabled { opacity: 0.6; cursor: not-allowed; }
  .block-hint { margin-top: 10px; line-height: 1.7; }

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
