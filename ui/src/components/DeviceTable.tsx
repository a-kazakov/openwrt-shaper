import { Tag, Progress, Button, Card, Grid } from "antd";
import { ThunderboltOutlined } from "@ant-design/icons";
import type { DeviceSnapshot } from "../types";
import { formatBytes, formatRate, formatDuration } from "../utils";
import { enableTurbo, cancelTurbo } from "../api";

const { useBreakpoint } = Grid;

interface Props {
  devices: DeviceSnapshot[];
  onMessage: (text: string, type: "success" | "error" | "info") => void;
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

function TurboButton({
  device,
  onMessage,
}: {
  device: DeviceSnapshot;
  onMessage: Props["onMessage"];
}) {
  const handleToggle = async () => {
    try {
      if (device.turbo) {
        await cancelTurbo(device.mac);
        onMessage(`Turbo cancelled for ${device.hostname || device.mac}`, "info");
      } else {
        await enableTurbo(device.mac, 15);
        onMessage(`Turbo enabled for ${device.hostname || device.mac}`, "success");
      }
    } catch (e) {
      onMessage(
        `Turbo failed: ${e instanceof Error ? e.message : String(e)}`,
        "error",
      );
    }
  };

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
        icon={<ThunderboltOutlined />}
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
      icon={<ThunderboltOutlined />}
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
          alignItems: "flex-start",
          marginBottom: 10,
        }}
      >
        <div>
          <div style={{ color: "#fff", fontWeight: 500, fontSize: 15 }}>{name}</div>
          <div style={{ color: "#555", fontSize: 11, marginTop: 2 }}>{device.mac}</div>
        </div>
        <Tag
          color={modeColor(device.mode)}
          style={{
            borderRadius: 4,
            fontWeight: 600,
            textTransform: "uppercase",
            fontSize: 10,
            letterSpacing: "0.05em",
            border: `1px solid ${modeColor(device.mode)}`,
            color: modeColor(device.mode),
            background: "transparent",
          }}
        >
          {device.mode}
        </Tag>
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
          <span>Bucket</span>
          <span>{device.bucket_pct}%</span>
        </div>
        <Progress
          percent={device.bucket_pct}
          showInfo={false}
          strokeColor={bucketColor(device.bucket_pct)}
          trailColor="#222"
          size={["100%", 6]}
        />
        {device.bucket_refill_bps > 0 && (
          <div style={{ color: "#555", fontSize: 11, marginTop: 2 }}>
            Refilling at {formatRate(device.bucket_refill_bps)}
          </div>
        )}
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: "6px 16px",
          fontSize: 12,
          marginBottom: 10,
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

      <TurboButton device={device} onMessage={onMessage} />
    </Card>
  );
}

export default function DeviceTable({ devices, onMessage }: Props) {
  const screens = useBreakpoint();
  const isDesktop = screens.lg;

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

  // Desktop: table layout
  if (isDesktop) {
    return (
      <div
        style={{
          background: "#111",
          border: "1px solid #222",
          borderRadius: 8,
          marginTop: 12,
          overflow: "hidden",
        }}
      >
        <div
          style={{
            padding: "12px 16px",
            color: "#666",
            fontSize: 12,
            textTransform: "uppercase",
            letterSpacing: "0.05em",
          }}
        >
          Devices
        </div>
        <div style={{ overflowX: "auto" }}>
          <table
            style={{
              width: "100%",
              borderCollapse: "collapse",
              fontSize: 13,
            }}
          >
            <thead>
              <tr
                style={{
                  borderTop: "1px solid #222",
                  borderBottom: "1px solid #222",
                }}
              >
                {["Device", "Mode", "Bucket", "Down / Up", "Session", "Cycle", "Turbo"].map(
                  (h) => (
                    <th
                      key={h}
                      style={{
                        padding: "10px 14px",
                        textAlign: "left",
                        color: "#555",
                        fontWeight: 500,
                        fontSize: 11,
                        textTransform: "uppercase",
                        letterSpacing: "0.05em",
                      }}
                    >
                      {h}
                    </th>
                  ),
                )}
              </tr>
            </thead>
            <tbody>
              {devices.map((d) => {
                const name = d.hostname || d.ip || d.mac;
                return (
                  <tr
                    key={d.mac}
                    style={{ borderBottom: "1px solid #1a1a1a" }}
                  >
                    <td style={{ padding: "10px 14px" }}>
                      <div style={{ color: "#fff", fontWeight: 500 }}>{name}</div>
                      <div style={{ color: "#444", fontSize: 11 }}>{d.mac}</div>
                    </td>
                    <td style={{ padding: "10px 14px" }}>
                      <Tag
                        color={modeColor(d.mode)}
                        style={{
                          borderRadius: 4,
                          fontWeight: 600,
                          textTransform: "uppercase",
                          fontSize: 10,
                          letterSpacing: "0.05em",
                          border: `1px solid ${modeColor(d.mode)}`,
                          color: modeColor(d.mode),
                          background: "transparent",
                        }}
                      >
                        {d.mode}
                      </Tag>
                    </td>
                    <td style={{ padding: "10px 14px", minWidth: 120 }}>
                      <Progress
                        percent={d.bucket_pct}
                        showInfo={false}
                        strokeColor={bucketColor(d.bucket_pct)}
                        trailColor="#222"
                        size={["100%", 6]}
                      />
                      <div style={{ color: "#555", fontSize: 11, marginTop: 2 }}>
                        {d.bucket_pct}%
                        {d.bucket_refill_bps > 0 && (
                          <span> · {formatRate(d.bucket_refill_bps)}</span>
                        )}
                      </div>
                    </td>
                    <td style={{ padding: "10px 14px" }}>
                      <span style={{ color: "#60a5fa" }}>
                        {formatRate(d.rate_down_bps)}
                      </span>
                      <span style={{ color: "#444" }}> / </span>
                      <span style={{ color: "#4ade80" }}>
                        {formatRate(d.rate_up_bps)}
                      </span>
                    </td>
                    <td style={{ padding: "10px 14px", color: "#999" }}>
                      {formatBytes(d.session_bytes)}
                    </td>
                    <td style={{ padding: "10px 14px", color: "#999" }}>
                      {formatBytes(d.cycle_bytes)}
                    </td>
                    <td style={{ padding: "10px 14px" }}>
                      <TurboButton device={d} onMessage={onMessage} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    );
  }

  // Mobile/tablet: card layout
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
