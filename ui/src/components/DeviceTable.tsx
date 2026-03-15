import { useState, useEffect, useRef } from "react";
import { Progress, Button, Card, Spin } from "antd";
import { ThunderboltOutlined, LoadingOutlined } from "@ant-design/icons";
import type { DeviceSnapshot } from "../types";
import { formatBytes, formatRate, formatDuration, formatMB } from "../utils";
import { enableTurbo, cancelTurbo } from "../api";

interface Props {
  devices: DeviceSnapshot[];
  onMessage: (text: string, type: "success" | "error" | "info") => void;
}

function modeLabel(mode: string): string {
  return mode === "sustained" ? "throttled" : mode;
}

function modeColor(mode: string): string {
  switch (mode) {
    case "burst":
      return "#60a5fa";
    case "sustained":
      return "#fbbf24";
    case "turbo":
      return "#4ade80";
    default:
      return "#666";
  }
}

function bucketColor(pct: number): string {
  if (pct <= 10) return "#ef4444";
  if (pct <= 30) return "#fbbf24";
  return "#4ade80";
}

function bucketStatus(device: DeviceSnapshot): { text: string; color: string } | null {
  const deviceSpeedBps = device.rate_down_bps + device.rate_up_bps;
  const refillBps = device.bucket_refill_bps;
  const net = refillBps - deviceSpeedBps;

  if (device.bucket_pct >= 100 && deviceSpeedBps === 0) {
    return { text: "Full", color: "#4ade80" };
  }
  if (device.bucket_pct <= 0 && refillBps === 0) {
    return { text: "Empty", color: "#ef4444" };
  }
  if (net > 0) {
    return { text: `Refilling at ${formatRate(net)}`, color: "#4ade80" };
  }
  if (net < 0) {
    return { text: `Draining at ${formatRate(-net)}`, color: "#fbbf24" };
  }
  return null;
}

/** Compute threshold percentage within the bucket, capped to 0-100. */
function thresholdPct(threshold: number, capacity: number): number {
  if (capacity <= 0) return 0;
  return Math.max(0, Math.min(100, (threshold / capacity) * 100));
}

function BucketBar({ device }: { device: DeviceSnapshot }) {
  const shapePct = thresholdPct(device.bucket_shape_at, device.bucket_capacity);
  const unshapePct = thresholdPct(device.bucket_unshape_at, device.bucket_capacity);

  // In burst mode, show the shape_at mark (where it will drop to throttled)
  // In sustained/throttled mode, show the unshape_at mark (where it will return to burst)
  const showMark = device.mode !== "turbo";
  const markPct = device.mode === "burst" ? shapePct : unshapePct;
  const markColor = device.mode === "burst" ? "#fbbf24" : "#60a5fa";

  return (
    <div style={{ position: "relative" }}>
      <Progress
        percent={device.bucket_pct}
        showInfo={false}
        strokeColor={bucketColor(device.bucket_pct)}
        trailColor="#222"
        size={["100%", 6]}
      />
      {showMark && markPct > 0 && markPct < 100 && (
        <div
          style={{
            position: "absolute",
            left: `${markPct}%`,
            top: 0,
            width: 2,
            height: 6,
            background: markColor,
            borderRadius: 1,
          }}
          title={
            device.mode === "burst"
              ? `Throttle at ${Math.round(shapePct)}%`
              : `Burst at ${Math.round(unshapePct)}%`
          }
        />
      )}
    </div>
  );
}

function TurboButton({
  device,
  onMessage,
}: {
  device: DeviceSnapshot;
  onMessage: Props["onMessage"];
}) {
  const [pending, setPending] = useState<boolean | null>(null);
  const pendingRef = useRef<boolean | null>(null);

  useEffect(() => {
    if (pendingRef.current !== null && device.turbo === pendingRef.current) {
      setPending(null);
      pendingRef.current = null;
    }
  }, [device.turbo]);

  const handleToggle = async () => {
    const wantTurbo = !device.turbo;
    setPending(wantTurbo);
    pendingRef.current = wantTurbo;
    try {
      if (device.turbo) {
        await cancelTurbo(device.mac);
        onMessage(`Turbo cancelled for ${device.hostname || device.mac}`, "info");
      } else {
        await enableTurbo(device.mac, 15);
        onMessage(`Turbo enabled for ${device.hostname || device.mac}`, "success");
      }
    } catch (e) {
      setPending(null);
      pendingRef.current = null;
      onMessage(
        `Turbo failed: ${e instanceof Error ? e.message : String(e)}`,
        "error",
      );
    }
  };

  const isLoading = pending !== null;

  if (device.turbo) {
    let remaining = "";
    if (device.turbo_expires) {
      const secs = device.turbo_expires - Math.floor(Date.now() / 1000);
      if (secs > 0) remaining = ` (${formatDuration(secs)})`;
    }
    return (
      <Button
        type="primary"
        size="small"
        danger
        onClick={handleToggle}
        disabled={isLoading}
        icon={isLoading ? <Spin indicator={<LoadingOutlined style={{ fontSize: 14 }} />} /> : <ThunderboltOutlined />}
        style={{ minHeight: 36, minWidth: 44 }}
      >
        Stop{remaining}
      </Button>
    );
  }

  return (
    <Button
      size="small"
      onClick={handleToggle}
      disabled={isLoading}
      icon={isLoading ? <Spin indicator={<LoadingOutlined style={{ fontSize: 14 }} />} /> : <ThunderboltOutlined />}
      style={{ minHeight: 36, minWidth: 44 }}
    >
      Turbo
    </Button>
  );
}

function DeviceCard({
  device,
  onMessage,
}: {
  device: DeviceSnapshot;
  onMessage: Props["onMessage"];
}) {
  const name = device.hostname || device.ip || device.mac;
  const bucketMB = formatMB(device.bucket_bytes);
  const capacityMB = formatMB(device.bucket_capacity);
  const status = bucketStatus(device);

  return (
    <Card
      size="small"
      style={{
        background: "#111",
        borderColor: "#222",
      }}
      styles={{ body: { padding: 14 } }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: 10,
        }}
      >
        <div>
          <div style={{ color: "#fff", fontWeight: 500, fontSize: 15 }}>{name}</div>
          <div style={{ color: "#555", fontSize: 11, marginTop: 2 }}>{device.mac}</div>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <TurboButton device={device} onMessage={onMessage} />
          <span
            style={{
              borderRadius: 4,
              fontWeight: 600,
              textTransform: "uppercase",
              fontSize: 10,
              letterSpacing: "0.05em",
              border: `1px solid ${modeColor(device.mode)}`,
              color: modeColor(device.mode),
              padding: "2px 8px",
              lineHeight: "20px",
              display: "inline-block",
            }}
          >
            {modeLabel(device.mode)}
          </span>
        </div>
      </div>

      <div style={{ marginBottom: 8 }}>
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            marginBottom: 4,
            fontSize: 12,
            color: "#666",
          }}
        >
          <span>Burst Budget</span>
          <span>{device.bucket_pct}% · {bucketMB}/{capacityMB}</span>
        </div>
        <BucketBar device={device} />
        {status && (
          <div style={{ color: status.color, fontSize: 11, marginTop: 4 }}>
            {status.text}
          </div>
        )}
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: "6px 16px",
          fontSize: 12,
        }}
      >
        <div>
          <span style={{ color: "#555" }}>Down </span>
          <span style={{ color: "#60a5fa" }}>
            {formatRate(device.rate_down_bps)}
          </span>
        </div>
        <div>
          <span style={{ color: "#555" }}>Up </span>
          <span style={{ color: "#4ade80" }}>
            {formatRate(device.rate_up_bps)}
          </span>
        </div>
        <div>
          <span style={{ color: "#555" }}>Session </span>
          <span style={{ color: "#999" }}>{formatBytes(device.session_bytes)}</span>
        </div>
        <div>
          <span style={{ color: "#555" }}>Cycle </span>
          <span style={{ color: "#999" }}>{formatBytes(device.cycle_bytes)}</span>
        </div>
      </div>
    </Card>
  );
}

export default function DeviceTable({ devices, onMessage }: Props) {
  if (!devices || devices.length === 0) {
    return (
      <div
        style={{
          background: "#111",
          border: "1px solid #222",
          borderRadius: 8,
          padding: 32,
          marginTop: 12,
          textAlign: "center",
          color: "#555",
        }}
      >
        No devices connected
      </div>
    );
  }

  return (
    <div style={{ marginTop: 12 }}>
      <div
        style={{
          color: "#666",
          fontSize: 12,
          textTransform: "uppercase",
          letterSpacing: "0.05em",
          marginBottom: 8,
        }}
      >
        Devices
      </div>
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))",
          gap: 10,
        }}
      >
        {devices.map((d) => (
          <DeviceCard key={d.mac} device={d} onMessage={onMessage} />
        ))}
      </div>
    </div>
  );
}
