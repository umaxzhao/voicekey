<script lang="ts">
  import { onMount } from "svelte";
  import { call, on, applyLook, type AppInfo, type Channel } from "./lib/api";
  import ChIcon from "./lib/ChIcon.svelte";
  import Wave from "./lib/Wave.svelte";

  let st = $state<{ state: string; text: string; channel: Channel }>({ state: "hidden", text: "", channel: "doubao" });
  let level = $state(0);

  onMount(() => {
    call<AppInfo>("get_state").then((i) => applyLook(i.platform, "dark"));
    const un = [
      on<{ state: string; text?: string; channel?: Channel }>("hud", (p) => (st = { ...st, text: "", ...p })),
      on<number>("level", (v) => (level = v)),
    ];
    return () => un.forEach((p) => p.then((f) => f()));
  });
</script>

<div class="wrap">
  <div class="capsule" class:show={st.state !== "hidden"} class:error={st.state === "error"} class:listening={st.state === "listen"}>
    {#if st.state === "error"}
      <span class="ic sm bad">!</span>
    {:else}
      <ChIcon ch={st.channel} size="sm" />
    {/if}
    {#if st.state === "listen"}
      <span class="pulse" aria-hidden="true"></span>
      <div class="wave"><Wave {level} height={32} /></div>
    {/if}
    {#if st.text}
      <div class="live"><span>{st.text}</span></div>
    {:else if st.state === "wait"}
      <div class="hint">识别中…</div>
    {/if}
    {#if st.state === "wait"}<span class="spin"></span>{/if}
  </div>
</div>

<style>
  .wrap { height: 100vh; display: flex; align-items: flex-end; justify-content: center; padding-bottom: 18px; }
  .capsule {
    position: relative;
    display: flex; align-items: center; gap: 12px;
    height: 54px; max-width: 720px; padding: 0 22px 0 14px;
    border-radius: 27px;
    background: linear-gradient(135deg, rgba(24, 26, 42, 0.94), rgba(34, 36, 58, 0.88));
    color: #fff;
    border: 1px solid rgba(255, 255, 255, 0.07);
    box-shadow:
      0 18px 55px rgba(0, 0, 0, 0.5),
      0 0 0 1px rgba(124, 107, 255, 0.12),
      0 0 40px rgba(62, 231, 255, 0.08);
    backdrop-filter: blur(20px) saturate(1.2);
    opacity: 0; transform: translateY(10px) scale(0.97);
    transition: opacity 0.18s ease, transform 0.18s cubic-bezier(0.2, 0.9, 0.2, 1), box-shadow 0.25s;
  }
  .capsule.show { opacity: 1; transform: none; }
  .capsule.listening {
    box-shadow:
      0 18px 55px rgba(0, 0, 0, 0.5),
      0 0 0 1px rgba(62, 231, 255, 0.2),
      0 0 55px rgba(62, 231, 255, 0.18);
  }
  .capsule::before {
    content: "";
    position: absolute; inset: -8px; border-radius: 34px; z-index: -1;
    background: radial-gradient(60% 120% at 50% 50%, rgba(62, 231, 255, 0.16), rgba(124, 107, 255, 0.1) 45%, transparent 72%);
    filter: blur(14px);
    opacity: 0; transition: opacity 0.25s;
  }
  .capsule.listening::before { opacity: 1; }
  .wave { width: 138px; flex: none; }
  .pulse {
    width: 7px; height: 7px; border-radius: 50%; flex: none;
    background: #3ee7ff;
    box-shadow: 0 0 12px #3ee7ff, 0 0 24px rgba(62, 231, 255, 0.5);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .live { min-width: 0; max-width: 470px; white-space: nowrap; overflow: hidden; direction: rtl; text-align: left; font-size: 14.5px; letter-spacing: 0.01em; }
  .live span { direction: ltr; unicode-bidi: plaintext; }
  .hint { font-size: 13px; color: rgba(255, 255, 255, 0.65); }
  .bad { background: var(--err); font-size: 13px; }
  .spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid rgba(255, 255, 255, 0.22); border-top-color: #3ee7ff; animation: spin 0.8s linear infinite; flex: none; }
  @keyframes pulse {
    0%, 100% { opacity: 0.35; transform: scale(0.75); }
    50% { opacity: 1; transform: scale(1.15); }
  }
</style>
