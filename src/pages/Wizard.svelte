<script>
  import { createEventDispatcher } from "svelte";
  import { migrateFromOld, getConfig, verifyCookie } from "../lib/api.js";
  import { loginStatus, showToast } from "../lib/stores.js";
  import { push } from "svelte-spa-router";

  const dispatch = createEventDispatcher();

  let step = 0; // 0=选择, 1=迁移文件夹, 2=完成
  let oldDir = "";
  let migrating = false;
  let migrateResult = null;

  function chooseNew() {
    step = 2;
    setTimeout(() => {
      dispatch("done");
      push("/");
    }, 500);
  }

  function chooseMigrate() {
    step = 1;
  }

  async function doMigrate() {
    if (!oldDir) {
      showToast("error", "请先选择项目文件夹");
      return;
    }
    migrating = true;
    try {
      migrateResult = await migrateFromOld(oldDir);
      step = 2;
    } catch (e) {
      showToast("error", "迁移失败: " + e);
    }
    migrating = false;
  }

  function doneWizard() {
    dispatch("done");
    push("/");
  }
</script>

<div class="wizard">
  {#if step === 0}
    <div class="wizard-card">
      <h1>🚀 欢迎使用 BiliCommentBot-RS</h1>
      <p class="sub">检测到你是第一次使用，请选择配置方式：</p>
      <div class="options">
        <button class="option-card" on:click={chooseMigrate}>
          <span class="icon">📂</span>
          <span class="title">迁移旧版数据</span>
          <span class="desc">从 Python 版 BiliCommentBot 项目文件夹导入所有配置和数据</span>
        </button>
        <button class="option-card" on:click={chooseNew}>
          <span class="icon">🆕</span>
          <span class="title">全新配置</span>
          <span class="desc">从头开始设置，适合第一次使用或想重新配置</span>
        </button>
      </div>
    </div>

  {:else if step === 1}
    <div class="wizard-card">
      <h1>📂 选择旧版项目文件夹</h1>
      <p class="sub">包含 config.toml、history.json、bilibili_cookie.json、video_cache.json 的文件夹</p>
      <div class="dir-input">
        <input
          type="text"
          placeholder="D:\your\BiliCommentBot 路径"
          bind:value={oldDir}
        />
      </div>
      <div class="wizard-actions">
        <button class="btn-back" on:click={() => (step = 0)}>← 返回</button>
        <button class="btn-primary" on:click={doMigrate} disabled={migrating || !oldDir}>
          {migrating ? "迁移中..." : "开始迁移"}
        </button>
      </div>
    </div>

  {:else if step === 2}
    <div class="wizard-card success">
      <h1>✅ 设置完成</h1>
      {#if migrateResult}
        <p>成功迁移 {migrateResult.migrated_count} 个文件</p>
        {#if migrateResult.errors?.length}
          <div class="migrate-errors">
            <p>迁移过程中出现以下问题：</p>
            {#each migrateResult.errors as err}
              <div class="err-item">⚠ {err}</div>
            {/each}
          </div>
        {/if}
      {/if}
      <button class="btn-primary" on:click={doneWizard}>
        开始使用 →
      </button>
    </div>
  {/if}
</div>

<style>
  .wizard {
    display: flex; align-items: center; justify-content: center; height: 100%;
  }
  .wizard-card {
    background: #16213e; border: 1px solid #1e3a5f; border-radius: 14px;
    padding: 40px 36px; max-width: 560px; width: 100%;
  }
  h1 { font-size: 1.4rem; color: #00b4d8; margin-bottom: 10px; text-align: center; }
  .sub { color: #8aa0b8; text-align: center; margin-bottom: 24px; font-size: 0.9rem; }
  .options { display: flex; gap: 14px; }
  .option-card {
    flex: 1; background: #1a2a4a; border: 1px solid #1e3a5f; border-radius: 10px;
    padding: 20px 16px; cursor: pointer; transition: 0.15s;
    display: flex; flex-direction: column; align-items: center; gap: 8px;
    color: #c0d0e0; font-size: 0.85rem;
  }
  .option-card:hover { border-color: #00b4d8; background: #1e3860; }
  .option-card .icon { font-size: 2rem; }
  .option-card .title { font-weight: 600; font-size: 0.95rem; }
  .option-card .desc { text-align: center; color: #8aa0b8; line-height: 1.4; }
  .dir-input { margin-bottom: 20px; }
  .dir-input input {
    width: 100%; padding: 10px 14px; border-radius: 8px;
    border: 1px solid #1e3a5f; background: #0d1b2a; color: #e0e8f0;
    font-size: 0.85rem; outline: none;
  }
  .dir-input input:focus { border-color: #00b4d8; }
  .wizard-actions {
    display: flex; justify-content: space-between; gap: 10px;
  }
  .btn-back {
    padding: 10px 20px; border: 1px solid #1e3a5f; border-radius: 8px;
    background: #0d1b2a; color: #b0c4de; cursor: pointer; font-size: 0.9rem;
  }
  .btn-primary {
    padding: 10px 24px; border: none; border-radius: 8px;
    background: #00b4d8; color: #fff; font-weight: 600; font-size: 0.9rem;
    cursor: pointer; transition: 0.15s;
  }
  .btn-primary:hover { opacity: 0.85; }
  .btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
  .success { text-align: center; }
  .success p { color: #b0c4de; margin: 8px 0 20px; }
  .migrate-errors { text-align: left; margin: 12px 0; }
  .migrate-errors p { color: #e74c3c; margin-bottom: 4px; }
  .err-item { padding: 4px 0; color: #f0c040; font-size: 0.85rem; }
</style>
