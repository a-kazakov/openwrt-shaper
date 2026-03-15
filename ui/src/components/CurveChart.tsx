import { useRef, useEffect, useCallback } from "react";
import type { CurveState, QuotaState, ConfigValues } from "../types";
import { formatRateKbit } from "../utils";
import { arcLengthCurvePoints } from "../curvePoints";

interface Props {
  curve: CurveState;
  quota: QuotaState;
  config: ConfigValues | null;
}

export default function CurveChart({ curve, quota, config }: Props) {
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

    const shape = curve.shape || 0.4;
    const maxRate = config?.max_rate_kbit ?? (curve.rate_kbit ? curve.rate_kbit * 2 : 50000);
    const minRate = config?.min_rate_kbit ?? 1000;

    const pad = { top: 20, right: 20, bottom: 30, left: 60 };
    const pw = w - pad.left - pad.right;
    const ph = h - pad.top - pad.bottom;

    // Clear
    ctx.clearRect(0, 0, w, h);

    // Grid lines
    ctx.strokeStyle = "#222";
    ctx.lineWidth = 1;
    ctx.font = "11px -apple-system, sans-serif";
    ctx.fillStyle = "#666";
    ctx.textAlign = "right";

    for (let i = 0; i <= 4; i++) {
      const gy = pad.top + (i / 4) * ph;
      const gridRate = maxRate * (1 - i / 4);
      ctx.beginPath();
      ctx.moveTo(pad.left, gy);
      ctx.lineTo(w - pad.right, gy);
      ctx.stroke();
      ctx.fillText(formatRateKbit(Math.round(gridRate)), pad.left - 8, gy + 4);
    }

    // X-axis labels
    ctx.textAlign = "center";
    ctx.fillStyle = "#666";
    ctx.fillText("0%", pad.left, h - 6);
    ctx.fillText("50% used", pad.left + pw / 2, h - 6);
    ctx.fillText("100%", pad.left + pw, h - 6);

    // Build curve with arc-length parameterization
    const pts = arcLengthCurvePoints(shape, minRate, maxRate, pad.left, pad.top, pw, ph);

    // Area fill
    ctx.beginPath();
    pts.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
    ctx.lineTo(pad.left, pad.top + ph);
    ctx.lineTo(pad.left + pw, pad.top + ph);
    ctx.closePath();
    ctx.fillStyle = "rgba(255, 255, 255, 0.04)";
    ctx.fill();

    // Curve line
    ctx.beginPath();
    pts.forEach((p, i) => (i === 0 ? ctx.moveTo(p.x, p.y) : ctx.lineTo(p.x, p.y)));
    ctx.strokeStyle = "rgba(255, 255, 255, 0.6)";
    ctx.lineWidth = 2;
    ctx.stroke();

    // "You are here" marker
    const usedPct = quota.total > 0 ? quota.used / quota.total : 0;
    const clampedPct = Math.min(1, Math.max(0, usedPct));
    const remainRatio = 1 - clampedPct;
    const youCurved = Math.pow(remainRatio, shape);
    const youRate = minRate + (maxRate - minRate) * youCurved;
    const youX = pad.left + clampedPct * pw;
    const youY = pad.top + ph - (youRate / maxRate) * ph;

    // Vertical dashed line
    ctx.beginPath();
    ctx.setLineDash([4, 4]);
    ctx.strokeStyle = "rgba(255, 255, 255, 0.3)";
    ctx.lineWidth = 1;
    ctx.moveTo(youX, pad.top);
    ctx.lineTo(youX, pad.top + ph);
    ctx.stroke();
    ctx.setLineDash([]);

    // Dot
    ctx.beginPath();
    ctx.arc(youX, youY, 6, 0, Math.PI * 2);
    ctx.fillStyle = "#fff";
    ctx.fill();
    ctx.strokeStyle = "#000";
    ctx.lineWidth = 2;
    ctx.stroke();

    // Label
    const labelText = formatRateKbit(Math.round(youRate));
    ctx.font = "bold 12px -apple-system, sans-serif";
    ctx.fillStyle = "#fff";
    // Flip label side when marker is near right edge to prevent clipping
    if (youX > w - 100) {
      ctx.textAlign = "right";
      ctx.fillText(labelText, youX - 12, youY - 12);
    } else {
      ctx.textAlign = "left";
      ctx.fillText(labelText, youX + 12, youY - 12);
    }
  }, [curve, quota, config]);

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
          color: "#666",
          fontSize: 12,
          textTransform: "uppercase",
          letterSpacing: "0.05em",
          marginBottom: 12,
        }}
      >
        Rate Curve
      </div>
      <canvas
        ref={canvasRef}
        style={{ width: "100%", height: 180, display: "block" }}
      />
    </div>
  );
}
