import { useState, useEffect, useRef } from "react";
import { Button, Card, Dropdown, Spin } from "antd";
import type { MenuProps } from "antd";
import { ThunderboltOutlined, LoadingOutlined } from "@ant-design/icons";
import type { DeviceSnapshot } from "../types";
import { formatRate, formatDuration, formatMB, formatBytesRound, formatRateRound, formatLimitPair, modeLabel, modeColor } from "../utils";
import { colors } from "../theme";
import { enableTurbo, cancelTurbo } from "../api";

interface Props {
  devices: DeviceSnapshot[];
  onMessage: (text: string, type: "success" | "error" | "info") => void;
}

function bucketColor(pct: number): string {
  if (pct <= 10) return colors.danger;
  if (pct <= 30) return colors.warning;
  return colors.success;
}

function bucketStatus(device: DeviceSnapshot): { text: string; color: string } | null {
  const deviceSpeedBps = device.rate_down_bps + device.rate_up_bps;
  const refillBps = device.bucket_refill_bps;
  const net = refillBps - deviceSpeedBps;

  if (device.bucket_pct >= 100 && deviceSpeedBps === 0) {
    return { text: "Full", color: colors.success };
  }
  if (device.bucket_pct <= 0 && refillBps === 0) {
    return { text: "Empty", color: colors.danger };
  }
  if (net > 0) {
    return { text: `Refilling at ${formatRate(net)}`, color: colors.success };
  }
  if (net < 0) {
    return { text: `Draining at ${formatRate(-net)}`, color: colors.warning };
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

  // Hysteresis marks: in burst mode show throttle threshold (black),
  // in sustained mode show burst threshold (white). Dead zone between
  // the two thresholds prevents mode flapping.
  const showMark = device.mode !== "turbo";
  const markPct = device.mode === "burst" ? shapePct : unshapePct;
  const markColor = device.mode === "burst" ? "#000000" : "#ffffff";

  return (
    <div
      style={{
        position: "relative",
        height: 6,
        borderRadius: 3,
        background: "#222",
        overflow: "visible",
      }}
    >
      <div
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          height: "100%",
          width: `${Math.max(0, Math.min(100, device.bucket_pct))}%`,
          borderRadius: 3,
          background: bucketColor(device.bucket_pct),
          transition: "width 0.3s",
        }}
      />
      {showMark && markPct > 0 && markPct < 100 && (
        <div
          style={{
            position: "absolute",
            left: `${markPct}%`,
            top: 0,
            height: "100%",
            width: 2,
            background: markColor,
            zIndex: 1,
            marginLeft: -1,
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

const TURBO_DURATIONS: { key: string; label: string; minutes: number }[] = [
  { key: "15", label: "15 min", minutes: 15 },
  { key: "30", label: "30 min", minutes: 30 },
  { key: "45", label: "45 min", minutes: 45 },
  { key: "60", label: "1 hour", minutes: 60 },
  { key: "90", label: "1h 30m", minutes: 90 },
  { key: "120", label: "2 hours", minutes: 120 },
  { key: "180", label: "3 hours", minutes: 180 },
  { key: "240", label: "4 hours", minutes: 240 },
  { key: "300", label: "5 hours", minutes: 300 },
  { key: "360", label: "6 hours", minutes: 360 },
];

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

  const handleEnable = async (minutes: number) => {
    setPending(true);
    pendingRef.current = true;
    try {
      await enableTurbo(device.mac, minutes);
      onMessage(`Turbo enabled for ${device.hostname || device.mac}`, "success");
    } catch (e) {
      setPending(null);
      pendingRef.current = null;
      onMessage(
        `Turbo failed: ${e instanceof Error ? e.message : String(e)}`,
        "error",
      );
    }
  };

  const handleCancel = async () => {
    setPending(false);
    pendingRef.current = false;
    try {
      await cancelTurbo(device.mac);
      onMessage(`Turbo cancelled for ${device.hostname || device.mac}`, "info");
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
        onClick={handleCancel}
        disabled={isLoading}
        icon={isLoading ? <Spin indicator={<LoadingOutlined style={{ fontSize: 14 }} />} /> : <ThunderboltOutlined />}
        style={{ height: 26, minWidth: 0, padding: "0 8px", fontSize: 11 }}
      >
        Stop{remaining}
      </Button>
    );
  }

  const menuItems: MenuProps["items"] = TURBO_DURATIONS.map((d) => ({
    key: d.key,
    label: d.label,
  }));

  return (
    <Dropdown
      menu={{
        items: menuItems,
        onClick: ({ key }) => {
          const dur = TURBO_DURATIONS.find((d) => d.key === key);
          if (dur) handleEnable(dur.minutes);
        },
      }}
      trigger={["click"]}
      disabled={isLoading}
    >
      <Button
        size="small"
        disabled={isLoading}
        icon={isLoading ? <Spin indicator={<LoadingOutlined style={{ fontSize: 14 }} />} /> : <ThunderboltOutlined />}
        style={{ height: 26, minWidth: 0, padding: "0 8px", fontSize: 11 }}
      >
        Turbo
      </Button>
    </Dropdown>
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
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ color: "#fff", fontWeight: 500, fontSize: 15, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{name}</div>
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
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: 4, fontSize: 11 }}>
          <span style={{ color: status?.color ?? "#555" }}>
            {status?.text ?? ""}
          </span>
          {device.shaped_down_kbit != null && device.shaped_up_kbit != null && (
            <span style={{ color: "#555" }}>
              Max: {formatLimitPair(device.shaped_down_kbit * 1000, device.shaped_up_kbit * 1000)}
            </span>
          )}
        </div>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "30% 30% 40%",
          fontSize: 12,
          color: "#999",
        }}
      >
        <span>
          <span style={{ color: "#4ade80" }}>&#9650;</span>{" "}
          {formatRateRound(device.rate_up_bps)}
        </span>
        <span>
          <span style={{ color: "#60a5fa" }}>&#9660;</span>{" "}
          {formatRateRound(device.rate_down_bps)}
        </span>
        <span>
          &#8721; {formatBytesRound(device.session_bytes)} / {formatBytesRound(device.cycle_bytes)}
        </span>
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
          gridTemplateColumns: "repeat(auto-fill, minmax(350px, 1fr))",
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
