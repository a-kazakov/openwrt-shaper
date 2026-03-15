import { useRef, useEffect, useCallback } from "react";
import type { ThroughputState } from "../types";
import { formatRate } from "../utils";

interface Props {
  throughput: ThroughputState;
}

export default function ThroughputChart({ throughput }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const samples = throughput.samples_1m;
    if (!samples || samples.length < 2) return;

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

    // Find max for scale
    let maxBps = 1000;
    for (const s of samples) {
      if (s.down_bps > maxBps) maxBps = s.down_bps;
      if (s.up_bps > maxBps) maxBps = s.up_bps;
    }
    maxBps *= 1.1;

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
    ctx.fillStyle = "rgba(96, 165, 250, 0.15)";
    ctx.fill();

    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j].down_bps / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = "#60a5fa";
    ctx.lineWidth = 2;
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
    ctx.fillStyle = "rgba(74, 222, 128, 0.10)";
    ctx.fill();

    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j].up_bps / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = "#4ade80";
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }, [throughput]);

  useEffect(() => {
    draw();

    const canvas = canvasRef.current;
    if (!canvas) return;

    const observer = new ResizeObserver(() => draw());
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [draw]);

  return (
    <div
      style={{
        background: "#111",
        border: "1px solid #222",
        borderRadius: 8,
        padding: 16,
        marginTop: 12,
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: 12,
        }}
      >
        <span
          style={{
            color: "#666",
            fontSize: 12,
            textTransform: "uppercase",
            letterSpacing: "0.05em",
          }}
        >
          Throughput (60s)
        </span>
        <div style={{ display: "flex", gap: 16, fontSize: 13 }}>
          <span style={{ color: "#60a5fa" }}>
            {formatRate(throughput.current_down_bps)} down
          </span>
          <span style={{ color: "#4ade80" }}>
            {formatRate(throughput.current_up_bps)} up
          </span>
        </div>
      </div>
      <div style={{ display: "flex", gap: 12, marginBottom: 8 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 11, color: "#666" }}>
          <span style={{ width: 8, height: 8, borderRadius: "50%", background: "#60a5fa", display: "inline-block" }} />
          Down
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 11, color: "#666" }}>
          <span style={{ width: 8, height: 8, borderRadius: "50%", background: "#4ade80", display: "inline-block" }} />
          Up
        </div>
      </div>
      <canvas
        ref={canvasRef}
        style={{ width: "100%", height: 100, display: "block" }}
      />
    </div>
  );
}
