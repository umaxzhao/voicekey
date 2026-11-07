<script lang="ts">
  import { onMount } from "svelte";
  let { level = 0, idle = false, height = 32 }: { level?: number; idle?: boolean; height?: number } = $props();
  let cv: HTMLCanvasElement;
  let shown = 0;

  onMount(() => {
    let raf = 0;
    let last = performance.now();

    const rounded = (x: CanvasRenderingContext2D, bx: number, by: number, bw: number, bh: number, r: number) => {
      x.beginPath();
      x.moveTo(bx + r, by);
      x.arcTo(bx + bw, by, bx + bw, by + bh, r);
      x.arcTo(bx + bw, by + bh, bx, by + bh, r);
      x.arcTo(bx, by + bh, bx, by, r);
      x.arcTo(bx, by, bx + bw, by, r);
      x.closePath();
    };

    const draw = (now: number) => {
      const dt = Math.min(48, Math.max(1, now - last));
      last = now;
      const target = Math.max(0, Math.min(1, level));
      shown += (target - shown) * Math.min(1, (dt / 16) * (target > shown ? 0.55 : 0.16));

      const r = devicePixelRatio || 1;
      const w = cv.clientWidth;
      const h = cv.clientHeight;
      if (w > 0 && h > 0) {
        const pw = Math.round(w * r);
        const ph = Math.round(h * r);
        if (cv.width !== pw || cv.height !== ph) {
          cv.width = pw;
          cv.height = ph;
        }
        const x = cv.getContext("2d")!;
        x.setTransform(r, 0, 0, r, 0, 0);
        x.clearRect(0, 0, w, h);

        const n = Math.max(18, Math.min(40, Math.round(w / 5)));
        const gap = Math.max(1.6, (w / n) * 0.34);
        const bw = Math.max(1.5, (w - gap * (n - 1)) / n);
        const t = now / 1000;

        const grad = x.createLinearGradient(0, 0, w, 0);
        grad.addColorStop(0, "#3ee7ff");
        grad.addColorStop(0.42, "#7c6bff");
        grad.addColorStop(0.72, "#c56cff");
        grad.addColorStop(1, "#ff5fa2");

        x.fillStyle = grad;
        x.shadowColor = "rgba(124, 107, 255, 0.42)";
        x.shadowBlur = 10;

        for (let i = 0; i < n; i++) {
          const p = i / (n - 1);
          const env = Math.pow(Math.sin(Math.PI * p), 0.8);
          const wave =
            Math.sin(t * 2.4 + p * 7.4) * 0.28 +
            Math.sin(t * 1.35 - p * 11.0) * 0.16 +
            Math.sin(t * 4.2 + p * 3.1) * 0.07;
          const base = idle ? 0.2 + 0.14 * (Math.sin(t * 1.7 + p * 4.2) * 0.5 + 0.5) : 0.06 + shown * 0.94;
          const v = Math.max(0.1, Math.min(1, base * env * (0.8 + wave * 0.55)));
          const bh = Math.max(2.5, v * (h - 2));
          const bx = i * (bw + gap);
          const by = (h - bh) / 2;
          rounded(x, bx, by, bw, bh, Math.min(bw / 2, 5));
          x.fill();
        }

        x.shadowBlur = 0;
      }
      raf = requestAnimationFrame(draw);
    };

    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  });
</script>

<canvas bind:this={cv} style="height:{height}px" aria-hidden="true"></canvas>

<style>
  canvas { display: block; width: 100%; min-width: 0; }
</style>
