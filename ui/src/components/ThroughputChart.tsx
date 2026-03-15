import { useRef, useEffect, useCallback } from "react";
import type { ThroughputState } from "../types";
import { formatRate } from "../utils";

interface Props {
  throughput: ThroughputState;
}

function Sparkline({ samples }: { samples: ThroughputState["samples_1m"] }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const w = rect.width;
    const h = rect.height;
    canvas.width = w * dpr;
    canvas.height = h * dpr;

    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);

    if (!samples || samples.length < 2) return;

    let maxBps = 1000;
    for (const s of samples) {
      if (s.down_bps > maxBps) maxBps = s.down_bps;
      if (s.up_bps > maxBps) maxBps = s.up_bps;
    }
    maxBps *= 1.15;

    const n = samples.length;

    // Download area + line
    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j].down_bps / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.lineTo(w, h);
    ctx.lineTo(0, h);
    ctx.closePath();
    const grad1 = ctx.createLinearGradient(0, 0, 0, h);
    grad1.addColorStop(0, "#60a5fa18");
    grad1.addColorStop(1, "#60a5fa04");
    ctx.fillStyle = grad1;
    ctx.fill();

    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j].down_bps / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = "#60a5fa50";
    ctx.lineWidth = 1.5;
    ctx.stroke();

    // Upload area + line
    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j].up_bps / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.lineTo(w, h);
    ctx.lineTo(0, h);
    ctx.closePath();
    const grad2 = ctx.createLinearGradient(0, 0, 0, h);
    grad2.addColorStop(0, "#4ade8012");
    grad2.addColorStop(1, "#4ade8004");
    ctx.fillStyle = grad2;
    ctx.fill();

    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j].up_bps / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = "#4ade8040";
    ctx.lineWidth = 1;
    ctx.stroke();
  }, [samples]);

  useEffect(() => {
    draw();
    const canvas = canvasRef.current;
    if (!canvas) return;
    const observer = new ResizeObserver(() => draw());
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [draw]);

  return (
    <canvas
      ref={canvasRef}
      style={{
        position: "absolute",
        inset: 0,
        width: "100%",
        height: "100%",
      }}
    />
  );
}

export default function ThroughputChart({ throughput }: Props) {
  return (
    <div
      style={{
        background: "#111",
        border: "1px solid #222",
        borderRadius: 8,
        padding: 14,
        position: "relative",
        overflow: "hidden",
        minHeight: 72,
      }}
    >
      <Sparkline samples={throughput.samples_1m} />
      <div style={{ position: "relative", zIndex: 1, display: "flex", gap: 20, alignItems: "baseline" }}>
        <div>
          <div style={{ color: "#666", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 2 }}>
            Down
          </div>
          <div style={{ color: "#60a5fa", fontSize: 22, fontWeight: 600, lineHeight: 1.2 }}>
            {formatRate(throughput.current_down_bps)}
          </div>
        </div>
        <div>
          <div style={{ color: "#666", fontSize: 11, textTransform: "uppercase", letterSpacing: "0.05em", marginBottom: 2 }}>
            Up
          </div>
          <div style={{ color: "#4ade80", fontSize: 22, fontWeight: 600, lineHeight: 1.2 }}>
            {formatRate(throughput.current_up_bps)}
          </div>
        </div>
      </div>
    </div>
  );
}
