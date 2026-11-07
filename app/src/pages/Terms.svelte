<script lang="ts">
  import { onMount } from "svelte";
  import { call } from "../lib/api";

  type Row = { canonical: string; aliases: string; hotword: boolean; protect: boolean };
  type Preset = { canonical: string; aliases: string[]; hotword: boolean; protect: boolean };

  let rows = $state<Row[]>([]);
  let path = $state("");
  let status = $state("");
  let busy = $state(false);

  const split = (text: string) =>
    text.split(/[,，、]/).map((part) => part.trim()).filter(Boolean);

  onMount(async () => {
    try {
      const data = await call<{ path: string; terms: Preset[] }>("get_terms");
      path = data.path;
      rows = data.terms.map((term) => ({
        canonical: term.canonical,
        aliases: term.aliases.join(", "),
        hotword: term.hotword,
        protect: term.protect,
      }));
    } catch (e) {
      status = `读取失败：${e}`;
    }
  });

  function add() {
    rows.push({ canonical: "", aliases: "", hotword: true, protect: false });
  }

  async function importPresets() {
    busy = true;
    try {
      const presets = await call<Preset[]>("term_presets");
      let added = 0;
      for (const preset of presets) {
        const found = rows.find(
          (row) => row.canonical.trim().toLowerCase() === preset.canonical.toLowerCase(),
        );
        if (found) {
          const merged = new Set([...split(found.aliases), ...preset.aliases]);
          found.aliases = [...merged].join(", ");
        } else {
          rows.push({
            canonical: preset.canonical,
            aliases: preset.aliases.join(", "),
            hotword: preset.hotword,
            protect: preset.protect,
          });
          added += 1;
        }
      }
      status = added > 0 ? `已导入 ${added} 个预设词条，点“保存”生效` : "预设词条已存在";
    } catch (e) {
      status = `导入失败：${e}`;
    } finally {
      busy = false;
    }
  }

  async function save() {
    busy = true;
    status = "保存中…";
    try {
      const terms = rows
        .filter((row) => row.canonical.trim())
        .map((row) => ({
          canonical: row.canonical.trim(),
          aliases: split(row.aliases),
          hotword: row.hotword,
          protect: row.protect,
        }));
      await call("save_terms", { terms });
      rows = terms.map((term) => ({
        canonical: term.canonical,
        aliases: term.aliases.join(", "),
        hotword: term.hotword,
        protect: term.protect,
      }));
      status = `已保存 ${terms.length} 个词条`;
    } catch (e) {
      status = `保存失败：${e}`;
    } finally {
      busy = false;
    }
  }
</script>

<h2>识别词汇</h2>
<div class="sub">
  识别完成后，自动把常见误写或拆开的词替换成规范写法。Fcitx5 全局语音与命令行识别共用这份词表。
</div>

<div class="group">
  <div class="row">
    <div class="lbl">
      开发常用预设
      <small>Claude Code、OpenCode、GitHub、API、Docker 等 20 多个常用词</small>
    </div>
    <button class="btn" disabled={busy} onclick={importPresets}>导入预设</button>
  </div>
  <div class="row">
    <div class="lbl">
      词条数量
      <small>{rows.length} 个 · {path || "~/.config/voicekey/terms.yaml"}</small>
    </div>
    <button class="btn pri" disabled={busy} onclick={save}>保存</button>
  </div>
</div>

<div class="list">
  {#each rows as row, i}
    <div class="term">
      <input class="in canon" bind:value={row.canonical} placeholder="规范写法，如 Claude Code" spellcheck="false" />
      <input class="in alias" bind:value={row.aliases} placeholder="别名，用逗号分隔，如 cloud code, cloude code" spellcheck="false" />
      <button class="del" title="删除" onclick={() => rows.splice(i, 1)}>✕</button>
    </div>
  {/each}
  <button class="add" onclick={add}>+ 添加词条</button>
</div>

{#if status}
  <p class="hint">{status}</p>
{/if}
<p class="hint">
  离线引擎不支持实时热词加权，这里的词条是在识别结果上做规范替换；云端渠道同样生效。
  英文别名按整词匹配，不会误伤单词内部。
</p>

<style>
  .list { margin-top: 10px; display: flex; flex-direction: column; gap: 6px; }
  .term {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: var(--rs);
    padding: 7px 9px;
  }
  .in {
    border: 1px solid var(--line);
    background: var(--card2);
    border-radius: var(--rs);
    padding: 5px 10px;
    outline: none;
  }
  .canon { width: 210px; flex: none; }
  .alias { flex: 1; min-width: 0; }
  .del {
    border: 0;
    background: none;
    color: var(--fg3);
    cursor: pointer;
    font-size: 13px;
    padding: 4px 6px;
  }
  .del:hover { color: var(--fg); }
  .add {
    border: 1.5px dashed var(--line);
    background: none;
    color: var(--fg2);
    border-radius: var(--rs);
    padding: 8px;
    cursor: pointer;
  }
  .add:hover { border-color: var(--accent); color: var(--accent); }
  .hint { color: var(--fg3); font-size: 12px; line-height: 1.6; margin-top: 10px; }
</style>
