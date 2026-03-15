import { useRef, useEffect, useCallback } from "react";
import { Row, Col } from "antd";
import type { ThroughputState } from "../types";
import { formatRate } from "../utils";

interface Props {
  throughput: ThroughputState;
}

function Sparkline({
  samples,
  field,
  color,
}: {
  samples: ThroughputState["samples_1m"];
  field: "down_bps" | "up_bps";
  color: string;
}) {
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
      if (s[field] > maxBps) maxBps = s[field];
    }
    maxBps *= 1.15;

    const n = samples.length;

    // Area fill
    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j][field] / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.lineTo(w, h);
    ctx.lineTo(0, h);
    ctx.closePath();

    const grad = ctx.createLinearGradient(0, 0, 0, h);
    grad.addColorStop(0, color + "20");
    grad.addColorStop(1, color + "05");
    ctx.fillStyle = grad;
    ctx.fill();

    // Line
    ctx.beginPath();
    for (let j = 0; j < n; j++) {
      const x = (j / (n - 1)) * w;
      const y = h - (samples[j][field] / maxBps) * h;
      if (j === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = color + "60";
    ctx.lineWidth = 1.5;
    ctx.stroke();
  }, [samples, field, color]);

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
    <Row gutter={[10, 10]} style={{ marginTop: 10 }}>
      <Col xs={12}>
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
          <Sparkline
            samples={throughput.samples_1m}
            field="down_bps"
            color="#60a5fa"
          />
          <div style={{ position: "relative", zIndex: 1 }}>
            <div
              style={{
                color: "#666",
                fontSize: 11,
                textTransform: "uppercase",
                letterSpacing: "0.05em",
                marginBottom: 2,
              }}
            >
              Download
            </div>
            <div
              style={{
                color: "#60a5fa",
                fontSize: 22,
                fontWeight: 600,
                lineHeight: 1.2,
              }}
            >
              {formatRate(throughput.current_down_bps)}
            </div>
          </div>
        </div>
      </Col>
      <Col xs={12}>
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
          <Sparkline
            samples={throughput.samples_1m}
            field="up_bps"
            color="#4ade80"
          />
          <div style={{ position: "relative", zIndex: 1 }}>
            <div
              style={{
                color: "#666",
                fontSize: 11,
                textTransform: "uppercase",
                letterSpacing: "0.05em",
                marginBottom: 2,
              }}
            >
              Upload
            </div>
            <div
              style={{
                color: "#4ade80",
                fontSize: 22,
                fontWeight: 600,
                lineHeight: 1.2,
              }}
            >
              {formatRate(throughput.current_up_bps)}
            </div>
          </div>
        </div>
      </Col>
    </Row>
  );
}
