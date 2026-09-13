<script>
  import { onMount } from "svelte";
  import { open as openExternal } from "@tauri-apps/api/shell";
  import {
    setPassword,
    checkOllama,
    listOllamaModels,
    getConfig,
    saveConfig,
    clearAllData,
    getDataFiles,
    getAutostartStatus,
    setAutostart,
  } from "../lib/api.js";
  import { showToast } from "../lib/stores.js";

  let pwdInput = "";
  let pwdConfirm = "";
  let hasPassword = false;
  let pwdSaving = false;
  let ollamaStatus = "未检测";
  let ollamaModels = [];
  let checking = false;

  async function handleSetPwd() {
    if (pwdSaving) return;
    // 留空不再等于「清除密码」：避免把 UI 占位符当成新密码提交
    if (!pwdInput) {
      showToast("error", hasPassword ? "密码未修改：留空保存不会改动现有密码" : "请输入新密码");
      return;
    }
    if (!pwdConfirm) {
      showToast("error", "请再次输入密码以确认");
      return;
    }
    if (pwdInput !== pwdConfirm) {
      showToast("error", "两次密码不一致");
      return;
    }
    pwdSaving = true;
    try {
      await setPassword(pwdInput);
      hasPassword = true;
      pwdInput = "";
      pwdConfirm = "";
      showToast("success", "密码已设置，下次启动需要输入密码");
    } catch (e) {
      showToast("error", "设置失败: " + e);
    }
    pwdSaving = false;
  }

  async function clearPassword() {
    if (pwdSaving) return;
    pwdSaving = true;
    try {
      await setPassword("");
      hasPassword = false;
      pwdInput = "";
      pwdConfirm = "";
      showToast("success", "密码已清除，下次启动不再需要输入密码");
    } catch (e) {
      showToast("error", "清除失败: " + e);
    }
    pwdSaving = false;
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
  let autoStartBot = true;      // app.auto_start_bot，默认 true
  let autoStartBusy = false;

  const CLOSE_ACTION_LABELS = {
    ask: "每次询问",
    tray: "最小化到托盘",
    exit: "直接退出程序",
  };

  async function loadConfig() {
    try {
      const cfg = await getConfig();
      // 字段一律留空：只提示「已设置」，真实密码需要用户重新输入才会被覆盖
      hasPassword = !!(cfg.auth?.enabled && cfg.auth?.password);
      pwdInput = "";
      pwdConfirm = "";
      closeAction = cfg.app?.close_action || "ask";
      autoStartBot = cfg.app?.auto_start_bot !== false;   // 缺省视为开启
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

  async function toggleAutoStartBot(e) {
    const next = e.target.checked;
    const prev = autoStartBot;
    autoStartBot = next;
    autoStartBusy = true;
    try {
      const cfg = await getConfig();
      if (!cfg.app) cfg.app = {};
      cfg.app.auto_start_bot = next;
      await saveConfig(cfg);
      showToast(
        "success",
        next ? "已开启：启动程序后自动运行机器人" : "已关闭：需要手动点击「启动」"
      );
    } catch (err) {
      autoStartBot = prev;
      showToast("error", "保存失败: " + err);
    }
    autoStartBusy = false;
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
  let dataFiles = null;
  let dataFilesLoading = false;
  const CONFIRM_PHRASE = "确认清空";

  function formatSize(bytes) {
    const n = Number(bytes) || 0;
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / 1024 / 1024).toFixed(2)} MB`;
  }

  async function openClearConfirm() {
    confirmText = "";
    clearResult = null;
    showClearConfirm = true;
    // 确认前先列出将被移入回收站的文件
    dataFiles = null;
    dataFilesLoading = true;
    try {
      dataFiles = await getDataFiles();
    } catch (e) {
      dataFiles = { error: String(e) };
    }
    dataFilesLoading = false;
  }

  function cancelClear() {
    showClearConfirm = false;
    confirmText = "";
    clearResult = null;
    dataFiles = null;
  }

  async function executeClear() {
    if (confirmText !== CONFIRM_PHRASE) {
      showToast("error", `请输入 "${CONFIRM_PHRASE}" 确认`);
      return;
    }
    clearing = true;
    try {
      clearResult = await clearAllData();
      // total === 0 表示没有可清理的文件，此时后端不会退出程序
      if ((clearResult?.total ?? 0) === 0) {
        showToast("success", "没有可清理的数据文件，应用不会退出");
      } else {
        showToast("success", `已清空 ${clearResult.trashed} 个文件到回收站，应用即将退出`);
      }
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
  <p class="desc">设置后访问需要密码验证；留空保存不会修改现有密码</p>
  <div class="form-row">
    <input
      type="password"
      placeholder="新密码"
      bind:value={pwdInput}
      disabled={pwdSaving}
      autocomplete="new-password"
    />
    <input
      type="password"
      placeholder="确认密码"
      bind:value={pwdConfirm}
      disabled={pwdSaving}
      autocomplete="new-password"
    />
    <button class="btn-save" on:click={handleSetPwd} disabled={pwdSaving}>
      {pwdSaving ? "保存中..." : "保存"}
    </button>
    {#if hasPassword}
      <button class="btn-secondary" on:click={clearPassword} disabled={pwdSaving}>清除密码</button>
    {/if}
  </div>
  {#if hasPassword}
    <p class="hint pwd-hint">已设置（留空则不修改）</p>
  {/if}
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

  <label class="switch-row mt" class:disabled={autoStartBusy}>
    <input
      type="checkbox"
      checked={autoStartBot}
      disabled={autoStartBusy}
      on:change={toggleAutoStartBot}
    />
    <span class="switch-text">
      启动时自动运行机器人
      <span class="hint">
        配置完整（已登录 + 已配置 AI）时，程序启动即开始工作；关闭后需手动点击「启动」
      </span>
    </span>
  </label>

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
      清空后应用将自动退出；若没有可清理的文件则不会退出。此操作不可撤销！
    </p>

    {#if dataFilesLoading}
      <p class="hint">正在读取待清理的文件...</p>
    {:else if dataFiles}
      {#if dataFiles.error}
        <p class="error-text">读取待清理文件失败：{dataFiles.error}</p>
      {:else}
        <div class="file-box">
          <p class="file-dir">数据目录：<code>{dataFiles.data_dir}</code></p>
          {#if dataFiles.files?.length}
            {#each dataFiles.files as f}
              <div class="file-row">
                <span class="file-path">{f.path}</span>
                <span class="file-size">{f.exists ? formatSize(f.size) : "不存在"}</span>
              </div>
            {/each}
            <p class="hint">共 {dataFiles.files.length} 个文件将被移入回收站</p>
          {:else}
            <p class="hint">当前没有可清理的数据文件，清理后应用不会退出</p>
          {/if}
        </div>
      {/if}
    {/if}

    <div class="form-col">
      <label class="confirm-label" for="clear-confirm-input">
        请输入 "<strong>{CONFIRM_PHRASE}</strong>" 以确认：
      </label>
      <input
        id="clear-confirm-input"
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
    <p class="sub">基于 <a href="https://github.com/Janson20/BiliCommentBot" on:click|preventDefault={() => openExternal("https://github.com/Janson20/BiliCommentBot")}>BiliCommentBot</a> 移植</p>
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
  .btn-save:disabled { opacity: 0.5; cursor: not-allowed; }
  .pwd-hint { display: block; margin-top: 6px; }
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
  .switch-row.mt { margin-top: 12px; }
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
  .file-box {
    background: #16213e; border: 1px solid #1e3a5f; border-radius: 6px;
    padding: 10px 12px; margin-bottom: 12px; max-height: 170px; overflow-y: auto;
  }
  .file-dir { font-size: 0.78rem; color: #8aa0b8; margin-bottom: 6px; word-break: break-all; }
  .file-dir code {
    background: #0d1b2a; border: 1px solid #1e3a5f; border-radius: 4px;
    padding: 1px 5px; color: #b0c4de;
  }
  .file-row {
    display: flex; justify-content: space-between; gap: 12px;
    font-size: 0.75rem; color: #b0c4de; padding: 2px 0;
    font-family: "Consolas", monospace;
  }
  .file-path { word-break: break-all; }
  .file-size { color: #5a7a9a; flex-shrink: 0; }
  .success-text { color: #27ae60; font-size: 0.82rem; }
  .error-text { color: #e74c3c; font-size: 0.82rem; }
  .warn-text { color: #f39c12; font-size: 0.78rem; }
</style>
