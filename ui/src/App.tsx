import { useState, useEffect, useCallback } from "react";
import { Layout, message, Row, Col, Popover } from "antd";
import { SettingOutlined } from "@ant-design/icons";
import { useWebSocket } from "./useWebSocket";
import { getConfig } from "./api";
import type { ConfigValues, StateSnapshot, DishStatus } from "./types";
import QuotaBar from "./components/QuotaBar";
import CurveChart from "./components/CurveChart";
import ThroughputChart from "./components/ThroughputChart";
import DeviceTable from "./components/DeviceTable";
import ConfigDrawer from "./components/ConfigDrawer";
import WarningBanner from "./components/WarningBanner";

const { Content } = Layout;

function showMessage(text: string, type: "success" | "error" | "info") {
  if (type === "success") message.success(text);
  else if (type === "error") message.error(text);
  else message.info(text);
}

function DishTooltip({ dish }: { dish: DishStatus }) {
  const row = (label: string, value: string) => (
    <div style={{ display: "flex", justifyContent: "space-between", gap: 16, fontSize: 12 }}>
      <span style={{ color: "#999" }}>{label}</span>
      <span style={{ color: "#fff" }}>{value}</span>
    </div>
  );
  return (
    <div style={{ minWidth: 180 }}>
      {row("Status", dish.connected ? "Connected" : dish.reachable ? "Reachable" : "Unreachable")}
      {dish.uptime > 0 && row("Uptime", `${Math.floor(dish.uptime / 3600)}h ${Math.floor((dish.uptime % 3600) / 60)}m`)}
      {dish.pop_ping_latency_ms > 0 && row("Latency", `${dish.pop_ping_latency_ms.toFixed(1)} ms`)}
      {dish.signal_quality > 0 && row("SNR", dish.signal_quality.toFixed(1))}
      {dish.downlink_bps > 0 && row("Downlink", `${(dish.downlink_bps / 1000000).toFixed(1)} Mbps`)}
      {dish.uplink_bps > 0 && row("Uplink", `${(dish.uplink_bps / 1000000).toFixed(1)} Mbps`)}
      {dish.obstructed && row("Obstructed", `${(dish.fraction_obstructed * 100).toFixed(1)}%`)}
      {dish.software_version && row("Software", dish.software_version)}
      {!dish.reachable && (
        <div style={{ color: "#fbbf24", fontSize: 11, marginTop: 6 }}>
          Dish is not reachable. Quota tracking relies on router counters only.
        </div>
      )}
    </div>
  );
}

function ConnectionStatus({ connected, state }: { connected: boolean; state: StateSnapshot | null }) {
  if (!connected) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 4 }}>
        <span style={{ width: 8, height: 8, borderRadius: "50%", background: "#ef4444", display: "inline-block" }} />
        <span style={{ color: "#666", fontSize: 12 }}>Reconnecting...</span>
      </div>
    );
  }

  const dish = state?.dish;
  const dishReachable = dish?.reachable === true;
  const dotColor = dishReachable ? "#4ade80" : "#fbbf24";
  const label = dishReachable ? "Connected" : "Connected, dish unreachable";

  const indicator = (
    <div
      style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 4, cursor: dish ? "pointer" : "default" }}
    >
      <span style={{ width: 8, height: 8, borderRadius: "50%", background: dotColor, display: "inline-block" }} />
      <span style={{ color: "#666", fontSize: 12 }}>{label}</span>
    </div>
  );

  if (!dish) return indicator;

  return (
    <Popover
      content={<DishTooltip dish={dish} />}
      title={<span style={{ fontSize: 13 }}>Starlink Dish</span>}
      trigger="click"
      overlayInnerStyle={{ background: "#111", border: "1px solid #333" }}
    >
      {indicator}
    </Popover>
  );
}

export default function App() {
  const { state, connected } = useWebSocket();
  const [configOpen, setConfigOpen] = useState(false);
  const [config, setConfig] = useState<ConfigValues | null>(null);

  const loadConfig = useCallback(() => {
    getConfig()
      .then(setConfig)
      // Non-blocking: re-fetched when user opens config drawer
      .catch((e) => console.warn("Config load failed:", e));
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  return (
    <Layout
      style={{
        minHeight: "100vh",
        background: "#000",
      }}
    >
      <Content
        style={{
          maxWidth: 1200,
          margin: "0 auto",
          padding: "env(safe-area-inset-top, 12px) 16px 24px",
          width: "100%",
        }}
      >
        {/* Header */}
        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            padding: "16px 0",
          }}
        >
          <div>
            <h1
              style={{
                margin: 0,
                fontSize: 22,
                fontWeight: 600,
                color: "#fff",
                letterSpacing: "-0.02em",
              }}
            >
              SLQM{" "}
              <span style={{ color: "#666", fontWeight: 400, fontSize: 14 }}>
                Starlink Quota Manager
              </span>
            </h1>
            <ConnectionStatus connected={connected} state={state} />
          </div>
          <button
            onClick={() => setConfigOpen(true)}
            style={{
              background: "none",
              border: "1px solid #333",
              color: "#999",
              padding: "8px 16px",
              borderRadius: 8,
              cursor: "pointer",
              display: "flex",
              alignItems: "center",
              gap: 6,
              fontSize: 14,
              minHeight: 44,
            }}
          >
            <SettingOutlined /> Config
          </button>
        </div>

        {state && (
          <>
            <WarningBanner warnings={state.warnings ?? []} />
            <Row gutter={[10, 10]}>
              <Col xs={24} sm={16}>
                <QuotaBar quota={state.quota} onMessage={showMessage} />
              </Col>
              <Col xs={24} sm={8}>
                <ThroughputChart throughput={state.throughput} />
              </Col>
            </Row>
            <CurveChart
              curve={state.curve}
              quota={state.quota}
              config={config}
            />
            <DeviceTable
              devices={state.devices}
              onMessage={showMessage}
            />
          </>
        )}
      </Content>

      <ConfigDrawer
        open={configOpen}
        onClose={() => setConfigOpen(false)}
        onSaved={(cfg) => {
          setConfig(cfg);
          message.success("Configuration saved");
        }}
      />
    </Layout>
  );
}
