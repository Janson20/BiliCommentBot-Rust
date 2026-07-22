<script>
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  import Sidebar from "./components/Sidebar.svelte";
  import Toast from "./components/Toast.svelte";
  import Dashboard from "./pages/Dashboard.svelte";
  import Login from "./pages/Login.svelte";
  import Config from "./pages/Config.svelte";
  import Logs from "./pages/Logs.svelte";
  import History from "./pages/History.svelte";
  import Settings from "./pages/Settings.svelte";
  import Wizard from "./pages/Wizard.svelte";

  import {
    isRunning,
    botStats,
    appendLog,
    loginStatus,
    videos,
    showToast,
  } from "./lib/stores.js";
  import { currentRoute, navigate } from "./lib/router.js";
  import { verifyCookie, getConfig, getBotStatus, verifyPassword } from "./lib/api.js";

  let wizardDone = false;
  let route = "/";           // 本地变量，绑定 store
  let unsubRoute = null;    // store 订阅取消函数
  let unlistenFn = null;    // event 取消函数

  // 密码锁屏
  let locked = false;
  let pwdInput = "";
  let unlocking = false;

  onMount(() => {
    // 显式订阅路由 store → 本地变量（比 $currentRoute 更可靠）
    unsubRoute = currentRoute.subscribe((v) => {
      route = v;
    });

    listen("bot-event", (event) => {
      const data = event.payload;
      if (!data) return;
      switch (data.type) {
        case "log":
          appendLog({ time: data.time, level: data.level, msg: data.msg });
          break;
        case "stats":
          isRunning.set(data.running);
          botStats.set({
            total_replied: data.total_replied,
            start_time: data.start_time,
            last_check: data.last_check,
            consecutive_failures: data.consecutive_failures,
          });
          break;
        case "status":
          isRunning.set(data.running);
          break;
        case "video_list":
          videos.set(data.videos || []);
          break;
      }
    }).then((fn) => { unlistenFn = fn; });

    // 后台加载配置 → 决定是否显示新手向导
    // 条件：无 B站登录凭证 且 无 AI 配置 → 视为首次使用
    (async () => {
      let cfg = null;
      try {
        cfg = await getConfig();
      } catch (_) {}

      // 密码保护：若启用了访问密码，先锁屏
      if (cfg?.auth?.enabled && cfg?.auth?.password) {
        locked = true;
      }

      const hasLogin = !!(cfg?.bilibili?.cookie && cfg?.bilibili?.uid);
      const hasAi = !!(
        cfg?.ai?.provider === "ollama" &&
        cfg?.ollama?.base_url &&
        cfg?.ollama?.model
      );
      // 两者缺一则显示向导
      if (!hasLogin || !hasAi) {
        wizardDone = false;
      } else {
        wizardDone = true;
      }

      // 启动时同步机器人运行状态（事件未到达前避免显示陈旧的"已停止"）
      try {
        const st = await getBotStatus();
        if (st) {
          isRunning.set(!!st.running);
          botStats.set({
            total_replied: st.total_replied ?? 0,
            start_time: st.start_time ?? null,
            last_check: st.last_check ?? null,
            consecutive_failures: st.consecutive_failures ?? 0,
          });
        }
      } catch (_) {}

      try {
        const result = await verifyCookie();
        if (result?.valid) {
          loginStatus.set({ loggedIn: true, uname: result.uname, uid: result.uid });
        }
      } catch (_) {}
    })();
  });

  onDestroy(() => {
    if (unsubRoute) unsubRoute();
    if (unlistenFn) unlistenFn();
  });

  function handleWizardDone() {
    wizardDone = true;
    // wizardDone 变化后 need tick，然后导航
    setTimeout(() => {
      navigate("/");
    }, 0);
  }

  async function handleUnlock() {
    unlocking = true;
    try {
      const ok = await verifyPassword(pwdInput);
      if (ok) {
        locked = false;
        pwdInput = "";
      } else {
        showToast("error", "密码错误");
      }
    } catch (e) {
      showToast("error", "验证失败: " + e);
    }
    unlocking = false;
  }
</script>

{#if locked}
  <div class="lock-screen">
    <form class="lock-card" on:submit|preventDefault={handleUnlock}>
      <div class="lock-icon">🔒</div>
      <h1>访问密码</h1>
      <p class="lock-hint">请输入密码以继续</p>
      <input
        type="password"
        bind:value={pwdInput}
        placeholder="密码"
        autocomplete="current-password"
        disabled={unlocking}
      />
      <button type="submit" disabled={unlocking || !pwdInput}>
        {unlocking ? "验证中..." : "解锁"}
      </button>
    </form>
  </div>
{:else if !wizardDone}
  <div class="app-layout">
    <div class="full-content">
      <Wizard on:done={handleWizardDone} />
    </div>
  </div>
{:else}
  <div class="app-layout">
    <Sidebar />
    <div class="main-content">
      <svelte:component this={
        route === "/login"    ? Login :
        route === "/config"   ? Config :
        route === "/logs"     ? Logs :
        route === "/history"  ? History :
        route === "/settings" ? Settings :
        Dashboard
      } />
    </div>
    <Toast />
  </div>
{/if}

<style>
  :global(*) { margin: 0; padding: 0; box-sizing: border-box; }
  :global(body) {
    font-family: "Microsoft YaHei", "PingFang SC", sans-serif;
    background: #1a1a2e;
    color: #e0e8f0;
    overflow: hidden;
  }
  .app-layout { display: flex; height: 100vh; }
  .full-content {
    flex: 1; display: flex; align-items: center; justify-content: center;
    background: #1a1a2e;
  }
  .main-content {
    flex: 1; overflow-y: auto; padding: 24px 28px; background: #1a1a2e;
  }
  :global(::-webkit-scrollbar) { width: 6px; }
  :global(::-webkit-scrollbar-track) { background: #0f1a2e; }
  :global(::-webkit-scrollbar-thumb) { background: #334; border-radius: 3px; }

  .lock-screen {
    display: flex; align-items: center; justify-content: center;
    height: 100vh; background: #1a1a2e;
  }
  .lock-card {
    display: flex; flex-direction: column; align-items: center; gap: 12px;
    background: #16213e; border: 1px solid #1e3a5f; border-radius: 14px;
    padding: 36px 40px; width: 320px;
  }
  .lock-icon { font-size: 2.4rem; }
  .lock-card h1 { font-size: 1.2rem; color: #00b4d8; }
  .lock-hint { font-size: 0.82rem; color: #8aa0b8; margin-top: -6px; }
  .lock-card input {
    width: 100%; padding: 10px 12px; border-radius: 8px;
    border: 1px solid #1e3a5f; background: #0d1b2a; color: #e0e8f0;
    font-size: 0.9rem; outline: none;
  }
  .lock-card input:focus { border-color: #00b4d8; }
  .lock-card button {
    width: 100%; padding: 10px; border: none; border-radius: 8px;
    background: #00b4d8; color: #fff; font-size: 0.9rem; font-weight: 600;
    cursor: pointer;
  }
  .lock-card button:hover { opacity: 0.9; }
  .lock-card button:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
