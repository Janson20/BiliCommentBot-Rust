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
  } from "./lib/stores.js";
  import { currentRoute, navigate } from "./lib/router.js";
  import { verifyCookie, getConfig } from "./lib/api.js";

  let wizardDone = false;
  let route = "/";           // 本地变量，绑定 store
  let unsubRoute = null;    // store 订阅取消函数
  let unlistenFn = null;    // event 取消函数

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
      try {
        const cfg = await getConfig();
        const hasLogin = !!(cfg?.bilibili?.cookie && cfg?.bilibili?.uid);
        const hasAi = !!(cfg?.deepseek?.api_key) || !!(
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
</script>

{#if !wizardDone}
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
</style>
