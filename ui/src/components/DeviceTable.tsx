import { useState, useRef } from "react";
import { Card, Dropdown } from "antd";
import type { MenuProps } from "antd";
import type { DeviceSnapshot } from "../types";
import { formatRate, formatDuration, formatMB, formatBytesRound, formatRateRound, formatLimitPair, modeLabel, modeColor } from "../utils";
import { colors } from "../theme";
import { enableTurbo, cancelTurbo, setDeviceMode, refillBucket } from "../api";

interface Props {
  devices: DeviceSnapshot[];
  onMessage: (text: string, type: "success" | "error" | "info") => void;
}

function bucketColor(pct: number): string {
  if (pct <= 10) return colors.danger;
  if (pct <= 30) return colors.warning;
  return colors.success;
}

/** Compute bucket status from the actual byte-level change between snapshots. */
function bucketStatus(device: DeviceSnapshot, deltaBps: number | null): { text: string; color: string } | null {
  if (device.mode === "disabled") {
    return { text: "Disabled", color: colors.textMuted };
  }
  if (device.mode === "throttled") {
    return { text: "Always throttled", color: colors.danger };
  }
  if (device.bucket_pct >= 100) {
    return { text: "Full", color: colors.success };
  }
  if (device.bucket_pct <= 0 && device.bucket_refill_bps === 0) {
    return { text: "Empty", color: colors.danger };
  }
  if (deltaBps == null) return null;
  if (deltaBps > 0) {
    return { text: `Refilling at ${formatRate(deltaBps)}`, color: colors.success };
  }
  if (deltaBps < 0) {
    return { text: `Draining at ${formatRate(-deltaBps)}`, color: colors.warning };
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

  const showMark = device.mode !== "turbo" && device.mode !== "throttled" && device.mode !== "disabled";
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

/** Build the mode menu items for a device based on its current state. */
function buildModeMenu(
  device: DeviceSnapshot,
): MenuProps["items"] {
  const items: MenuProps["items"] = [];
  const isOverridden = device.mode === "throttled" || device.mode === "disabled";

  if (device.turbo || isOverridden) {
    items.push({
      key: "normal",
      label: device.turbo ? "Cancel Turbo" : "Set Normal",
    });
    items.push({ type: "divider" });
  }

  if (!device.turbo) {
    items.push({
      key: "turbo",
      label: "Enable Turbo",
      children: TURBO_DURATIONS.map((d) => ({
        key: `turbo_${d.key}`,
        label: d.label,
      })),
    });
  }

  if (device.mode !== "throttled") {
    items.push({
      key: "throttled",
      label: "Always Throttled",
    });
  }

  if (device.mode !== "disabled") {
    items.push({
      key: "disabled",
      label: "Disable Device",
    });
  }

  if (!isOverridden && device.bucket_pct < 100) {
    items.push({ type: "divider" });
    items.push({
      key: "refill",
      label: "Refill Burst Budget",
    });
  }

  return items;
}

function StatusBadge({
  device,
  onMessage,
}: {
  device: DeviceSnapshot;
  onMessage: Props["onMessage"];
}) {
  const [loading, setLoading] = useState(false);
  const name = device.hostname || device.mac;

  const handleAction = async (action: string, value?: number) => {
    setLoading(true);
    try {
      if (action === "normal") {
        if (device.turbo) {
          await cancelTurbo(device.mac);
          onMessage(`Turbo cancelled for ${name}`, "info");
        } else {
          await setDeviceMode(device.mac, "normal");
          onMessage(`${name} set to normal`, "success");
        }
      } else if (action === "turbo" && value) {
        await enableTurbo(device.mac, value);
        onMessage(`Turbo enabled for ${name} (${value} min)`, "success");
      } else if (action === "throttled") {
        await setDeviceMode(device.mac, "throttled");
        onMessage(`${name} set to always throttled`, "info");
      } else if (action === "disabled") {
        await setDeviceMode(device.mac, "disabled");
        onMessage(`${name} disabled`, "info");
      } else if (action === "refill") {
        const capacityMb = Math.round(device.bucket_capacity / 1000000);
        await refillBucket(device.mac, capacityMb);
        onMessage(`Burst budget refilled for ${name}`, "success");
      }
    } catch (e) {
      onMessage(
        `Action failed: ${e instanceof Error ? e.message : String(e)}`,
        "error",
      );
    } finally {
      setLoading(false);
    }
  };

  // Turbo countdown text
  let badgeText = modeLabel(device.mode);
  if (device.turbo && device.turbo_expires) {
    const secs = device.turbo_expires - Math.floor(Date.now() / 1000);
    if (secs > 0) badgeText += ` ${formatDuration(secs)}`;
  }

  const menuItems = buildModeMenu(device);

  return (
    <Dropdown
      menu={{
        items: menuItems,
        onClick: ({ key }) => {
          if (key.startsWith("turbo_")) {
            const minutes = parseInt(key.replace("turbo_", ""), 10);
            handleAction("turbo", minutes);
          } else {
            handleAction(key);
          }
        },
      }}
      trigger={["click"]}
      disabled={loading}
    >
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
          cursor: "pointer",
          opacity: loading ? 0.5 : 1,
          userSelect: "none",
        }}
      >
        {badgeText} <span style={{ fontSize: 8, marginLeft: 2 }}>&#9660;</span>
      </span>
    </Dropdown>
  );
}

function DeviceCard({
  device,
  bucketDeltaBps,
  onMessage,
}: {
  device: DeviceSnapshot;
  bucketDeltaBps: number | null;
  onMessage: Props["onMessage"];
}) {
  const name = device.hostname || device.ip || device.mac;
  const bucketMB = formatMB(device.bucket_bytes);
  const capacityMB = formatMB(device.bucket_capacity);
  const status = bucketStatus(device, bucketDeltaBps);

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
        <StatusBadge device={device} onMessage={onMessage} />
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
              Max: {formatLimitPair(device.shaped_up_kbit * 1000, device.shaped_down_kbit * 1000)}
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
  const prevBytesRef = useRef<Record<string, number>>({});
  const prevTsRef = useRef<number>(0);

  const deltaBpsMap: Record<string, number | null> = {};
  const now = Date.now() / 1000;
  const elapsed = prevTsRef.current > 0 ? now - prevTsRef.current : 0;

  for (const d of devices) {
    const prev = prevBytesRef.current[d.mac];
    if (prev != null && elapsed > 0) {
      deltaBpsMap[d.mac] = ((d.bucket_bytes - prev) * 8) / elapsed;
    } else {
      deltaBpsMap[d.mac] = null;
    }
  }

  const nextBytes: Record<string, number> = {};
  for (const d of devices) {
    nextBytes[d.mac] = d.bucket_bytes;
  }
  prevBytesRef.current = nextBytes;
  prevTsRef.current = now;

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
          <DeviceCard
            key={d.mac}
            device={d}
            bucketDeltaBps={deltaBpsMap[d.mac] ?? null}
            onMessage={onMessage}
          />
        ))}
      </div>
    </div>
  );
}
