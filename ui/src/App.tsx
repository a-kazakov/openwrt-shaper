import { useState, useEffect, useCallback } from "react";
import { Layout, message, Row, Col } from "antd";
import { SettingOutlined } from "@ant-design/icons";
import { useWebSocket } from "./useWebSocket";
import { getConfig } from "./api";
import type { ConfigValues } from "./types";
import QuotaBar from "./components/QuotaBar";
import CurveChart from "./components/CurveChart";
import ThroughputChart from "./components/ThroughputChart";
import DeviceTable from "./components/DeviceTable";
import ConfigDrawer from "./components/ConfigDrawer";

const { Content } = Layout;

function showMessage(text: string, type: "success" | "error" | "info") {
  if (type === "success") message.success(text);
  else if (type === "error") message.error(text);
  else message.info(text);
}

export default function App() {
  const { state, connected } = useWebSocket();
  const [configOpen, setConfigOpen] = useState(false);
  const [config, setConfig] = useState<ConfigValues | null>(null);

  const loadConfig = useCallback(() => {
    getConfig()
      .then(setConfig)
      .catch(() => {
        /* will retry on drawer open */
      });
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
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                marginTop: 4,
              }}
            >
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: "50%",
                  background: connected ? "#4ade80" : "#ef4444",
                  display: "inline-block",
                }}
              />
              <span style={{ color: "#666", fontSize: 12 }}>
                {connected ? "Connected" : "Reconnecting..."}
              </span>
            </div>
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
