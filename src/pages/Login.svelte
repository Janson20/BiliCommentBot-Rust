<script>
  import { onMount, onDestroy } from "svelte";
  import { generateQrcode, pollQrLogin, verifyCookie, setCookieManually, refreshCookie } from "../lib/api.js";
  import { loginStatus, showToast } from "../lib/stores.js";

  let qrBase64 = "";
  let qrcodeKey = "";
  let polling = false;
  let pollMsg = "";
  let pollTimer = null;

  // 手动输入
  let manualMode = false;
  let manualCookie = "";
  let manualRefreshToken = "";

  onMount(() => {
    checkLogin();
  });

  async function checkLogin() {
    try {
      const r = await verifyCookie();
      if (r.valid) {
        loginStatus.set({ loggedIn: true, uname: r.uname, uid: r.uid });
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
    pollMsg = "请使用B站APP扫码...";
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
        // 登录成功
        stopPolling();
        showToast("success", "扫码登录成功！");
        await checkLogin();
        return;
      }
      if (r.code === 86038) {
        pollMsg = "二维码已过期，请重新生成";
        stopPolling();
        return;
      }
      if (r.code === 86090 || r.code === 86101) {
        pollMsg = "已扫描，请在手机上确认...";
      }
    } catch (_) {
      pollMsg = "轮询出错了，请重试";
      stopPolling();
      return;
    }
    pollTimer = setTimeout(poll, 2000);
  }

  async function handleManualSubmit() {
    if (!manualCookie) {
      showToast("error", "请输入Cookie");
      return;
    }
    try {
      await setCookieManually(manualCookie, manualRefreshToken || null);
      showToast("success", "Cookie已保存");
      await checkLogin();
    } catch (e) {
      showToast("error", "设置Cookie失败: " + e);
    }
  }

  async function doRefresh() {
    try {
      const r = await refreshCookie();
      if (r.success) {
        showToast("success", "Cookie刷新成功");
        await checkLogin();
      } else {
        showToast("error", r.message);
      }
    } catch (e) {
      showToast("error", "刷新失败: " + e);
    }
  }

  onDestroy(() => stopPolling());
</script>

<h1>🔑 扫码登录</h1>

{#if $loginStatus.loggedIn}
  <div class="logged-in">
    <div class="user-card">
      <span class="badge">✅ 已登录</span>
      <div class="user-info">
        <span class="label">用户名</span>
        <span class="val">{$loginStatus.uname || "—"}</span>
      </div>
      <div class="user-info">
        <span class="label">UID</span>
        <span class="val">{$loginStatus.uid || "—"}</span>
      </div>
      <button class="btn-sm" on:click={doRefresh}>🔄 刷新Cookie</button>
    </div>
  </div>
{/if}

<div class="tabs">
  <button class="tab" class:active={!manualMode} on:click={() => (manualMode = false)}>扫码登录</button>
  <button class="tab" class:active={manualMode} on:click={() => (manualMode = true)}>手动输入Cookie</button>
</div>

{#if !manualMode}
  <div class="qr-section">
    {#if qrBase64}
      <div class="qr-wrapper">
        <img src={qrBase64} alt="QR Code" />
        {#if pollMsg}
          <div class="poll-msg">{pollMsg}</div>
        {/if}
      </div>
    {:else}
      <button class="btn-primary" on:click={startQrLogin}>📱 生成二维码</button>
    {/if}
  </div>
{:else}
  <div class="manual-section">
    <div class="form-group">
      <label>Cookie 字符串</label>
      <textarea
        bind:value={manualCookie}
        placeholder="SESSDATA=xxx; bili_jct=xxx; DedeUserID=xxx; ..."
        rows="4"
      ></textarea>
    </div>
    <div class="form-group">
      <label>Refresh Token（可选）</label>
      <input type="text" bind:value={manualRefreshToken} placeholder="刷新令牌" />
    </div>
    <button class="btn-primary" on:click={handleManualSubmit}>💾 保存</button>
  </div>
{/if}

<style>
  h1 { font-size: 1.5rem; color: #00b4d8; margin-bottom: 20px; }
  .logged-in { margin-bottom: 20px; }
  .user-card {
    background: #16213e; border: 1px solid #1e3a5f; border-radius: 10px;
    padding: 18px; display: flex; flex-direction: column; gap: 8px;
    max-width: 320px;
  }
  .badge { color: #2ecc71; font-weight: 600; font-size: 0.9rem; }
  .user-info { display: flex; justify-content: space-between; }
  .user-info .label { color: #8aa0b8; font-size: 0.82rem; }
  .user-info .val { color: #e0e8f0; font-size: 0.85rem; }
  .btn-sm {
    margin-top: 4px; padding: 6px 14px; border: 1px solid #1e3a5f;
    border-radius: 6px; background: #1a2a4a; color: #b0c4de;
    cursor: pointer; font-size: 0.82rem;
  }
  .btn-sm:hover { background: #1e3a5f; }
  .tabs { display: flex; gap: 0; margin-bottom: 20px; }
  .tab {
    padding: 8px 20px; border: 1px solid #1e3a5f; background: #0d1b2a;
    color: #8aa0b8; cursor: pointer; font-size: 0.85rem;
  }
  .tab:first-child { border-radius: 8px 0 0 8px; }
  .tab:last-child { border-radius: 0 8px 8px 0; }
  .tab.active { background: #00b4d8; color: #fff; border-color: #00b4d8; }
  .qr-section { display: flex; justify-content: center; padding: 24px 0; }
  .qr-wrapper { text-align: center; }
  .qr-wrapper img { width: 200px; height: 200px; border-radius: 10px; background: #fff; padding: 8px; }
  .poll-msg { margin-top: 10px; color: #00b4d8; font-size: 0.85rem; }
  .btn-primary {
    padding: 10px 24px; border: none; border-radius: 8px;
    background: #00b4d8; color: #fff; font-weight: 600; cursor: pointer;
  }
  .btn-primary:hover { opacity: 0.85; }
  .manual-section { max-width: 500px; }
  .form-group { margin-bottom: 14px; }
  .form-group label {
    display: block; margin-bottom: 5px; font-size: 0.85rem; color: #8aa0b8;
  }
  .form-group textarea,
  .form-group input {
    width: 100%; padding: 10px 12px; border-radius: 8px;
    border: 1px solid #1e3a5f; background: #0d1b2a; color: #e0e8f0;
    font-size: 0.82rem; outline: none; resize: vertical; font-family: "Consolas", monospace;
  }
  .form-group textarea:focus,
  .form-group input:focus { border-color: #00b4d8; }
</style>
