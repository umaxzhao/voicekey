<script lang="ts">
  import { onMount } from "svelte";
  import { call } from "../lib/api";
  import { app, save as saveSettings } from "../lib/store.svelte";

  type Fcitx5Settings = {
    holdKey: string;
    tailMs: number;
    command: string;
    configPath: string;
  };

  const HOLDS: [string, string][] = [
    ["F5", "F5"],
    ["F6", "F6"],
    ["F7", "F7"],
    ["F8", "F8"],
    ["F9", "F9"],
    ["F10", "F10"],
    ["F11", "F11"],
    ["F12", "F12"],
    ["Alt_R", "Alt（左右均可）"],
    ["Control_R", "Ctrl（左右均可）"],
    ["Shift_R", "Shift（左右均可）"],
    ["Super_L", "Super（左右均可）"],
  ];

  let s = $state<Fcitx5Settings | null>(null);
  let status = $state("");

  onMount(async () => {
    try {
      s = await call<Fcitx5Settings>("get_fcitx5_settings");
    } catch (e) {
      status = `读取失败：${e}`;
    }
  });

  async function save() {
    if (!s) return;
    status = "保存中…";
    try {
      await call("set_fcitx5_settings", {
        holdKey: s.holdKey,
        tailMs: Math.max(0, Math.min(2000, Number(s.tailMs) || 0)),
      });
      await call("restart_fcitx5");
      status = "已保存，并已重启 Fcitx5";
    } catch (e) {
      status = `保存失败：${e}`;
    }
  }
</script>

<h2>Fcitx5 语音模块</h2>
<div class="sub">保留现有拼音/五笔等输入法，按住快捷键说话，松开后直接上屏。</div>

{#if s}
  <div class="group">
    <div class="row">
      <div class="lbl">长按说话键<small>与“快捷键”页自动同步；也可在这里设为 F5–F12 等特殊键</small></div>
      <select class="in" bind:value={s.holdKey}>
        {#each HOLDS as [id, name]}
          <option value={id}>{name}</option>
        {/each}
      </select>
    </div>
    <div class="row">
      <div class="lbl">尾音时长<small>松开后继续录制的毫秒数，避免最后一个字被截断</small></div>
      <input class="in" type="number" min="0" max="2000" step="50" bind:value={s.tailMs} />
    </div>
    <div class="row">
      <div class="lbl">
        仅使用 Fcitx5 语音
        <small>关闭独立应用的按键监听，避免同时出现两个浮层或重复上屏</small>
      </div>
      <button
        class="sw"
        class:on={app.s?.fcitxOnly}
        onclick={() => app.s && saveSettings({ fcitxOnly: !app.s.fcitxOnly })}
        aria-label="仅使用 Fcitx5 语音"
      ></button>
    </div>
    <div class="row">
      <div class="lbl">配置文件<small>{s.configPath}</small></div>
      <button class="btn pri" onclick={save}>保存并重启 Fcitx5</button>
    </div>
    {#if status}
      <div class="row">
        <div class="lbl">{status}</div>
      </div>
    {/if}
  </div>
  <p class="hint">
    这里设置的是 <b>Fcitx5 全局语音</b> 的按键：在任何输入框里按住说话，松开即上屏。
    在“快捷键”页选择修饰键（如左 Super、左 Ctrl）时会自动同步到这里，不需要设两次。
    修饰键不区分左右，按左 Ctrl 或右 Ctrl 都可以。
    模块通过 <code>voicekey-asr</code> 识别，并读取“识别”页当前选择的渠道。
  </p>
{:else}
  <div class="sub">读取中…</div>
{/if}

<style>
  .in {
    width: 200px;
    flex: none;
    border: 1px solid var(--line);
    background: var(--card2);
    border-radius: var(--rs);
    padding: 5px 10px;
    outline: none;
  }
  .hint {
    color: var(--fg3);
    font-size: 12px;
    line-height: 1.6;
    margin-top: 14px;
  }
  code {
    background: var(--card2);
    padding: 1px 5px;
    border-radius: 4px;
  }
</style>
