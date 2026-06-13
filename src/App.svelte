<script>
  import Router, { location } from "svelte-spa-router";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

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
    showToast,
    loginStatus,
    videos,
    history,
  } from "./lib/stores.js";
  import { getBotStatus, verifyCookie, getConfig } from "./lib/api.js";

  const routes = {
    "/": Dashboard,
    "/login": Login,
    "/config": Config,
    "/logs": Logs,
    "/history": History,
    "/settings": Settings,
    "/wizard": Wizard,
  };

  let wizardDone = false;

  onMount(async () => {
    // 监听后端 bot-event 推送
    const unlisten = await listen("bot-event", (event) => {
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
        case "history":
          // 单条新历史记录，前端按需刷新
          break;
      }
    });

    // 加载初始配置
    try {
      const cfg = await getConfig();
      // 检查是否需要新手向导
      if (
        !cfg.bilibili.uid ||
        (!cfg.bilibili.cookie && !cfg.deepseek.api_key)
      ) {
        wizardDone = false;
      } else {
        wizardDone = true;
      }
    } catch (e) {
      console.error("加载配置失败:", e);
    }

    // 检查登录状态
    try {
      const result = await verifyCookie();
      if (result.valid) {
        loginStatus.set({ loggedIn: true, uname: result.uname, uid: result.uid });
      }
    } catch (_) {}

    return () => unlisten();
  });
</script>

<div class="app-layout">
  <Sidebar />
  <div class="main-content">
    {#if !wizardDone}
      <Wizard on:done={() => (wizardDone = true)} />
    {:else}
      <Router {routes} />
    {/if}
  </div>
  <Toast />
</div>

<style>
  :global(*) {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
  }
  :global(body) {
    font-family: "Microsoft YaHei", "PingFang SC", sans-serif;
    background: #1a1a2e;
    color: #e0e8f0;
    overflow: hidden;
  }
  .app-layout {
    display: flex;
    height: 100vh;
  }
  .main-content {
    flex: 1;
    overflow-y: auto;
    padding: 24px 28px;
    background: #1a1a2e;
  }
  :global(.main-content h1) {
    font-size: 1.5rem;
    color: #00b4d8;
    margin-bottom: 16px;
  }
  :global(::-webkit-scrollbar) { width: 6px; }
  :global(::-webkit-scrollbar-track) { background: #0f1a2e; }
  :global(::-webkit-scrollbar-thumb) { background: #334; border-radius: 3px; }
</style>
